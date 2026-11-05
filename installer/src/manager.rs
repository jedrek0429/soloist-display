use crate::terminal::Progress;
use crate::{
    dependencies, legacy, migrations, mpris,
    paths::{path_string, Paths},
    platform::{Architecture, Platform},
    process,
    release::{Artifact, ReleaseChannel, ReleaseClient, ReleaseManifest},
    service, soloist,
    state::{
        now, InstallRecovery, InstallState, SoloistOwnership, TransactionJournal, TransactionStatus,
    },
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum SoloistRemoval {
    Restore,
    Keep,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DisplayDataRemoval {
    Keep,
    Remove,
}

pub struct UninstallOptions {
    pub soloist: Option<SoloistRemoval>,
    pub data: Option<DisplayDataRemoval>,
    pub yes: bool,
}

fn artifact_destination(release: &Path, artifact: &Artifact) -> Result<PathBuf> {
    let name = match artifact.kind.as_str() {
        "manager" => "soloist-display",
        "audio-bridge" => "soloist-audio-bridge",
        other => bail!("unexpected executable artifact kind {other}"),
    };
    Ok(release.join("bin").join(name))
}

fn validate_executable(path: &Path, architecture: &Architecture) -> Result<()> {
    let bytes = fs::read(path)?;
    if bytes.len() < 40 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        bail!(
            "{} is not a supported little-endian ELF executable",
            path.display()
        );
    }
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    let valid = match architecture {
        Architecture::X86_64 => bytes[4] == 2 && machine == 62,
        Architecture::Aarch64 => bytes[4] == 2 && machine == 183,
        Architecture::Armv7 => {
            let flags = u32::from_le_bytes(bytes[36..40].try_into()?);
            bytes[4] == 1 && machine == 40 && flags & 0x400 != 0
        }
    };
    if !valid {
        bail!(
            "{} is incompatible with the detected architecture",
            path.display()
        );
    }
    Ok(())
}

fn ensure_staging_space(available: u64, required: u64) -> Result<()> {
    if available < required.saturating_add(32 * 1024 * 1024) {
        bail!(
            "insufficient disk space: {} MiB available; {} MiB plus staging space required",
            available / 1024 / 1024,
            required / 1024 / 1024
        );
    }
    Ok(())
}

fn atomic_link(target: &Path, link: &Path) -> Result<()> {
    let parent = link.parent().context("activation link has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".current.{}", std::process::id()));
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    symlink(target, &temporary)?;
    fs::rename(&temporary, link).context("activate release")?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn install_artifacts(
    paths: &Paths,
    platform: &Platform,
    client: &ReleaseClient,
    manifest: &ReleaseManifest,
    journal: &mut TransactionJournal,
    journal_path: &Path,
) -> Result<PathBuf> {
    let required = ["manager", "audio-bridge"]
        .iter()
        .map(|kind| {
            manifest
                .artifact(kind, platform)
                .map(|artifact| artifact.size)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<u64>();
    let available = fs2::available_space(&paths.releases)?;
    ensure_staging_space(available, required)?;
    let release = paths
        .releases
        .join(format!("{}-{}", manifest.version, platform.target));
    if release.exists() {
        for kind in ["manager", "audio-bridge"] {
            let artifact = manifest.artifact(kind, platform)?;
            let destination = artifact_destination(&release, artifact)?;
            if !destination.is_file()
                || crate::release::sha256_file(&destination)? != artifact.sha256
            {
                bail!(
                    "existing release {} is incomplete or corrupt",
                    release.display()
                );
            }
            validate_executable(&destination, &platform.architecture)?;
        }
        return Ok(release);
    }
    let staging = paths.releases.join(format!(
        ".staging-{}-{}",
        manifest.version,
        std::process::id()
    ));
    fs::create_dir_all(staging.join("bin"))?;
    journal.record_created(journal_path, staging.clone())?;
    let result = (|| {
        for kind in ["manager", "audio-bridge"] {
            let artifact = manifest.artifact(kind, platform)?;
            let destination = artifact_destination(&staging, artifact)?;
            client.download(artifact, &destination)?;
            validate_executable(&destination, &platform.architecture)?;
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
            let metadata = fs::metadata(&destination)?;
            if metadata.len() == 0 {
                bail!("downloaded {} is empty", artifact.name);
            }
        }
        fs::rename(&staging, &release)?;
        Ok::<_, anyhow::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    journal.created.retain(|path| path != &staging);
    journal.record_created(journal_path, release.clone())?;
    Ok(release)
}

fn desktop_files(paths: &Paths) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(&paths.applications)?;
    let executable = path_string(&paths.visible_command)
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    let entries = [
        (
            "soloist-display.desktop",
            "Soloist Display",
            format!("\"{executable}\" launch"),
        ),
        (
            "soloist-display-settings.desktop",
            "Soloist Display Settings",
            format!("\"{executable}\" settings"),
        ),
    ];
    let mut created = vec![];
    for (name, title, command) in entries {
        let path = paths.applications.join(name);
        fs::write(&path, format!("[Desktop Entry]\nType=Application\nName={title}\nExec={command}\nIcon=multimedia-player\nTerminal=false\nCategories=AudioVideo;Audio;\n"))?;
        created.push(path);
    }
    Ok(created)
}

const ZSH_START: &str = "# >>> soloist-display completion >>>";
const ZSH_END: &str = "# <<< soloist-display completion <<<";

fn completion_directory(paths: &Paths) -> PathBuf {
    paths.data_home.join("zsh/site-functions")
}

fn completion_paths(paths: &Paths) -> [(&'static str, PathBuf); 3] {
    [
        (
            "bash",
            paths
                .data_home
                .join("bash-completion/completions/soloist-display"),
        ),
        ("zsh", completion_directory(paths).join("_soloist-display")),
        (
            "fish",
            paths
                .data_home
                .join("fish/vendor_completions.d/soloist-display.fish"),
        ),
    ]
}

fn shell_available(shell: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            fs::metadata(directory.join(shell)).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn completion_snippet(directory: &Path, activate: bool) -> String {
    let quoted = directory.to_string_lossy().replace('\'', "'\\''");
    let mut snippet = format!("{ZSH_START}\nfpath=('{quoted}' $fpath)\n");
    if activate {
        snippet.push_str("autoload -Uz compinit\ncompinit\n");
    }
    snippet.push_str(&format!("{ZSH_END}\n"));
    snippet
}

fn strip_owned_zshrc_block(existing: &str, directory: &Path) -> String {
    // Preserve unrelated content and refuse to delete edited installer blocks.
    for activate in [false, true] {
        let snippet = completion_snippet(directory, activate);
        if existing.contains(&snippet) {
            return existing.replacen(&snippet, "", 1);
        }
    }
    let old = format!(
        "\n{ZSH_START}\nfpath=(\"{}\" $fpath)\nautoload -Uz compinit\ncompinit\n{ZSH_END}\n",
        directory.display()
    );
    if existing.ends_with(&old) {
        return existing[..existing.len() - old.len()].to_owned();
    }
    existing.to_owned()
}

fn integrate_zshrc(existing: &str, directory: &Path) -> String {
    let cleaned = strip_owned_zshrc_block(existing, directory);
    if cleaned.contains(ZSH_START) {
        return cleaned;
    }
    let lines: Vec<&str> = cleaned.split_inclusive('\n').collect();
    let insertion = lines.iter().position(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('#')
            && (trimmed.starts_with("compinit")
                || (trimmed.starts_with("autoload") && trimmed.contains("compinit"))
                || trimmed.contains("oh-my-zsh.sh")
                || trimmed.contains("prezto/init.zsh"))
    });
    let mut output = String::new();
    if let Some(index) = insertion {
        for line in &lines[..index] {
            output.push_str(line);
        }
        output.push_str(&completion_snippet(directory, false));
        for line in &lines[index..] {
            output.push_str(line);
        }
    } else {
        output.push_str(&cleaned);
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&completion_snippet(directory, true));
    }
    output
}

fn completion_script(binary: &Path, shell: &str) -> Result<Vec<u8>> {
    let output = Command::new(binary)
        .args(["completions", shell])
        .output()
        .with_context(|| format!("generate {shell} completions from {}", binary.display()))?;
    if !output.status.success() {
        bail!(
            "{shell} completion generation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn install_completions_with(paths: &Paths, available: impl Fn(&str) -> bool) -> Result<()> {
    let binary = paths.current.join("bin/soloist-display");
    for (shell, file) in completion_paths(paths) {
        if !available(shell) {
            continue;
        }
        let script = completion_script(&binary, shell)?;
        fs::create_dir_all(file.parent().context("missing completion directory")?)?;
        fs::write(file, script)?;

        if shell == "zsh" {
            let rc = paths.home.join(".zshrc");
            if rc.is_symlink() {
                bail!("refusing to modify symlinked .zshrc");
            }
            let existing = match fs::read_to_string(&rc) {
                Ok(value) => value,
                Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
                Err(error) => return Err(error.into()),
            };
            let updated = integrate_zshrc(&existing, &completion_directory(paths));
            if updated != existing {
                fs::write(rc, updated)?;
            }
        }
    }
    Ok(())
}

fn install_completions(paths: &Paths) -> Result<()> {
    install_completions_with(paths, shell_available)
}

fn remove_completions(paths: &Paths) -> Result<()> {
    for (_, file) in completion_paths(paths) {
        if file.exists() {
            fs::remove_file(file)?;
        }
    }
    let rc = paths.home.join(".zshrc");
    if rc.is_symlink() {
        return Ok(());
    }
    let content = match fs::read_to_string(&rc) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let cleaned = strip_owned_zshrc_block(&content, &completion_directory(paths));
    if cleaned != content {
        fs::write(&rc, cleaned)?;
    }
    Ok(())
}

fn install_visible_command(paths: &Paths) -> Result<()> {
    fs::create_dir_all(&paths.bin_dir)?;
    atomic_link(
        &paths.current.join("bin/soloist-display"),
        &paths.visible_command,
    )?;
    if let Err(error) = install_completions(paths) {
        eprintln!("warning: could not configure shell completions: {error:#}");
    }
    Ok(())
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn completion_uses_the_active_binary() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("soloist-display");
        fs::write(&binary, "#!/bin/sh\necho active-release-completions\n").unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();
        assert_eq!(
            completion_script(&binary, "fish").unwrap(),
            b"active-release-completions\n"
        );
    }

    #[test]
    fn creates_only_completions_for_available_shells() {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths {
            home: temp.path().to_path_buf(),
            data_home: temp.path().join("data"),
            config_home: temp.path().join("config"),
            state_home: temp.path().join("state"),
            cache_home: temp.path().join("cache"),
            root: temp.path().join("data/soloist-display"),
            config: temp.path().join("config/soloist-display"),
            state: temp.path().join("state/soloist-display"),
            cache: temp.path().join("cache/soloist-display"),
            releases: temp.path().join("data/soloist-display/releases"),
            current: temp.path().join("data/soloist-display/current"),
            transactions: temp.path().join("state/soloist-display/transactions"),
            logs: temp.path().join("state/soloist-display/logs"),
            lock: temp.path().join("state/soloist-display/installer.lock"),
            install_state: temp.path().join("state/soloist-display/install.json"),
            bin_dir: temp.path().join("bin"),
            visible_command: temp.path().join("bin/soloist-display"),
            display_service: temp
                .path()
                .join("config/systemd/user/soloist-display.service"),
            soloist_service: temp.path().join("config/systemd/user/soloist.service"),
            applications: temp.path().join("data/applications"),
            soloist_env: temp.path().join("config/soloist-display/soloist.env"),
            managed_soloist: temp.path().join("data/soloist-display/soloist"),
            managed_soloist_current: temp.path().join("data/soloist-display/soloist/current"),
            soloist_data: temp.path().join("data/soloist"),
            soloist_cache: temp.path().join("cache/soloist"),
        };
        let binary = paths.current.join("bin/soloist-display");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, "#!/bin/sh\nprintf '%s\\n' \"$2\"\n").unwrap();
        let mut mode = fs::metadata(&binary).unwrap().permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&binary, mode).unwrap();

        install_completions_with(&paths, |shell| shell != "fish").unwrap();

        let files = completion_paths(&paths);
        assert_eq!(fs::read_to_string(&files[0].1).unwrap(), "bash\n");
        assert_eq!(fs::read_to_string(&files[1].1).unwrap(), "zsh\n");
        assert!(!files[2].1.exists());
        assert!(!paths.data_home.join("fish").exists());

        remove_completions(&paths).unwrap();
        assert!(!files[0].1.exists());
        assert!(!files[1].1.exists());
        assert!(!fs::read_to_string(paths.home.join(".zshrc"))
            .unwrap()
            .contains(ZSH_START));
    }

    #[test]
    fn inserts_before_existing_compinit_without_duplicating_it() {
        let path = Path::new("/home/test/.local/share/zsh/site-functions");
        let original = "export EDITOR=vim\nautoload -Uz compinit\ncompinit\n";
        let once = integrate_zshrc(original, path);
        assert!(once.find("fpath=").unwrap() < once.find("autoload -Uz compinit").unwrap());
        assert_eq!(once.matches("\ncompinit\n").count(), 1);
        assert_eq!(once, integrate_zshrc(&once, path));
    }

    #[test]
    fn initializes_once_when_no_completion_system_exists() {
        let path = Path::new("/tmp/site-functions");
        let once = integrate_zshrc("export FOO=bar\n", path);
        assert_eq!(once.matches("\ncompinit\n").count(), 1);
        assert_eq!(once, integrate_zshrc(&once, path));
    }

    #[test]
    fn upgrades_legacy_snippet_without_repeating_compinit() {
        let path = Path::new("/home/test/.local/share/zsh/site-functions");
        let original = format!("autoload -Uz compinit\ncompinit\n\n{ZSH_START}\nfpath=(\"{}\" $fpath)\nautoload -Uz compinit\ncompinit\n{ZSH_END}\n", path.display());
        let updated = integrate_zshrc(&original, path);
        assert_eq!(updated.matches("\ncompinit\n").count(), 1);
        assert_eq!(updated.matches(ZSH_START).count(), 1);
        assert_eq!(updated, integrate_zshrc(&updated, path));
    }

    #[test]
    fn uninstaller_preserves_edits_to_owned_section() {
        let path = Path::new("/tmp/zsh");
        let owned = completion_snippet(path, false);
        let content = format!("export FOO=1\n{owned}export BAR=2\n");
        assert_eq!(
            strip_owned_zshrc_block(&content, path),
            "export FOO=1\nexport BAR=2\n"
        );
        let edited = content.replace("fpath=", "# custom\nfpath=");
        assert_eq!(strip_owned_zshrc_block(&edited, path), edited);
    }

    #[test]
    fn preserves_existing_framework_and_handles_quoted_paths() {
        let path = Path::new("/home/it's me/zsh");
        let updated = integrate_zshrc("source $ZSH/oh-my-zsh.sh\n", path);
        assert!(updated.contains("it'\\''s me"));
        assert!(updated.find("fpath=").unwrap() < updated.find("source ").unwrap());
        assert!(!updated.contains("compinit\n"));
    }
}

fn port_available(port: u16) -> bool {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).is_ok()
}

fn wait_http(paths: &Paths) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    for _ in 0..30 {
        if TcpStream::connect_timeout(
            &"127.0.0.1:8080".parse().unwrap(),
            Duration::from_millis(150),
        )
        .is_ok()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    bail!("display health check failed: http://127.0.0.1:8080 did not become available")
}

fn audio_monitor_available() -> bool {
    let Ok(sink) = Command::new("pactl").arg("get-default-sink").output() else {
        return false;
    };
    if !sink.status.success() {
        return false;
    }
    let Ok(name) = String::from_utf8(sink.stdout) else {
        return false;
    };
    let monitor = format!("{}.monitor", name.trim());
    let Ok(sources) = Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
    else {
        return false;
    };
    sources.status.success()
        && String::from_utf8_lossy(&sources.stdout)
            .lines()
            .any(|line| line.split_whitespace().nth(1) == Some(monitor.as_str()))
}

fn wait_for_install_health(
    paths: &Paths,
    soloist_state: &crate::state::SoloistState,
) -> Result<()> {
    if std::env::var("SOLOIST_DISPLAY_FAULT").as_deref() == Ok("health_check") {
        bail!("injected health check failure");
    }
    if paths.is_test() {
        return Ok(());
    }
    wait_http(paths)?;
    for _ in 0..30 {
        if soloist::external_healthy(soloist_state)
            && TcpStream::connect_timeout(
                &"127.0.0.1:9091".parse().unwrap(),
                Duration::from_millis(150),
            )
            .is_ok()
        {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    if !soloist::external_healthy(soloist_state) {
        bail!(
            "Soloist health check failed: WebSocket {} is unavailable",
            soloist_state
                .websocket_url
                .as_deref()
                .unwrap_or("not configured")
        );
    }
    if TcpStream::connect("127.0.0.1:9091").is_err() {
        bail!("audio bridge health check failed: WebSocket 127.0.0.1:9091 is unavailable");
    }
    // PipeWire may expose its PulseAudio monitor after the bridge's socket opens.
    // Use the same probe as doctor, but allow device discovery to settle.
    let mut monitor_ready = false;
    for _ in 0..30 {
        if audio_monitor_available() {
            monitor_ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    if !monitor_ready {
        let sink = Command::new("pactl")
            .arg("get-default-sink")
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_else(|| "<pactl unavailable>".to_owned());
        bail!(
            "audio health check failed after 6s: no monitor found for default sink {sink:?}; inspect `pactl list sinks` and `pactl list short sources`"
        );
    }
    Ok(())
}

fn restore_snapshot_file(path: &Path, previous: Option<&[u8]>, mode: u32) -> Result<()> {
    if path.is_symlink() {
        fs::remove_file(path)?;
    }
    if let Some(bytes) = previous {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn recover_install(paths: &Paths, previous: &InstallRecovery) -> Result<()> {
    let service_dir = paths
        .soloist_service
        .parent()
        .context("missing systemd directory")?;
    if !paths.is_test() {
        service::disable_owned(paths, "soloist-display.service")?;
    }
    service::remove_renewal_timer(paths)?;
    restore_snapshot_file(
        &paths.config.join("config.json"),
        previous.previous_settings.as_deref(),
        0o600,
    )?;
    restore_snapshot_file(
        &paths.state.join("config-schema.json"),
        previous.previous_config_schema.as_deref(),
        0o600,
    )?;
    restore_snapshot_file(
        &paths.state.join("soloist-runtime.json"),
        previous.previous_runtime.as_deref(),
        0o600,
    )?;
    restore_snapshot_file(
        &paths.install_state,
        previous.previous_metadata.as_deref(),
        0o600,
    )?;
    restore_snapshot_file(
        &paths.display_service,
        previous.previous_display_unit.as_deref(),
        0o644,
    )?;
    if let Some(original) = &previous.previous_soloist_link {
        atomic_link(original, &paths.managed_soloist_current)?;
    } else if paths.managed_soloist_current.is_symlink() {
        fs::remove_file(&paths.managed_soloist_current)?;
    }
    if previous.restore_existing_service {
        let snapshot = service::SoloistUnitSnapshot::load(paths)?
            .ok_or_else(|| anyhow!("previous Soloist unit snapshot is missing"))?;
        snapshot.restore(paths)?;
        if !previous.unit_backup_existed {
            service::SoloistUnitSnapshot::remove_backup(paths)?;
        }
    } else {
        if previous.previous_soloist_unit.is_none() && paths.soloist_service.exists() {
            service::disable_owned(paths, "soloist.service")?;
        }
        restore_snapshot_file(&paths.soloist_env, previous.previous_env.as_deref(), 0o600)?;
        restore_snapshot_file(
            &paths.soloist_service,
            previous.previous_soloist_unit.as_deref(),
            0o644,
        )?;
        if previous.previous_soloist_unit.is_some() {
            service::daemon_reload(paths)?;
            service::restore_unit_state(
                paths,
                "soloist.service",
                previous.previous_soloist_enabled,
                previous.previous_soloist_active,
            )?;
        }
    }
    restore_snapshot_file(
        &service_dir.join("soloist-display-renew.service"),
        previous.previous_renewal_service.as_deref(),
        0o644,
    )?;
    restore_snapshot_file(
        &service_dir.join("soloist-display-renew.timer"),
        previous.previous_renewal_timer.as_deref(),
        0o644,
    )?;
    service::daemon_reload(paths)?;
    if previous.previous_renewal_timer.is_some() {
        service::restore_unit_state(
            paths,
            "soloist-display-renew.timer",
            previous.previous_timer_enabled,
            previous.previous_timer_active,
        )?;
    }
    if previous.previous_display_unit.is_some() {
        service::restore_unit_state(
            paths,
            "soloist-display.service",
            previous.previous_display_enabled,
            previous.previous_display_active,
        )?;
    }
    Ok(())
}

pub fn recover(paths: &Paths) -> Result<()> {
    if !paths.transactions.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&paths.transactions)? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)?;
        let mut transaction: TransactionJournal = serde_json::from_slice(&bytes)?;
        if matches!(
            transaction.status,
            TransactionStatus::Committed | TransactionStatus::RolledBack
        ) {
            continue;
        }
        transaction.status = TransactionStatus::RollingBack;
        transaction.persist(&path)?;
        for created in transaction.created.iter().rev() {
            if created.starts_with(&paths.root) {
                if created.is_dir() {
                    fs::remove_dir_all(created).ok();
                } else {
                    fs::remove_file(created).ok();
                }
            }
        }
        for backup in transaction.backups.iter().rev() {
            if backup.backup.exists() {
                if backup.original.exists() {
                    if backup.original.is_dir() {
                        fs::remove_dir_all(&backup.original)?;
                    } else {
                        fs::remove_file(&backup.original)?;
                    }
                }
                fs::rename(&backup.backup, &backup.original)?;
            }
        }
        if let Some(previous) = &transaction.previous_current {
            atomic_link(previous, &paths.current)?;
        } else if transaction.operation == "install" && paths.current.is_symlink() {
            fs::remove_file(&paths.current)?;
        }
        if let Some(snapshot) = &transaction.install_recovery {
            recover_install(paths, snapshot)?;
        }
        transaction.status = TransactionStatus::RolledBack;
        transaction.persist(&path)?;
    }
    Ok(())
}

fn select_external_soloist(discovered: &crate::state::SoloistState, yes: bool) -> Result<bool> {
    if yes || !io::stdin().is_terminal() || discovered.ownership != SoloistOwnership::Independent {
        return Ok(false);
    }
    eprintln!("\n  Existing Spotify Soloist installation detected.");
    eprintln!("  Managed Soloist installs its own binary and systemd user service.");
    eprintln!("  It automatically renews builds before they expire.");
    eprintln!("  1  Use managed Soloist (recommended)");
    eprintln!("  2  Keep existing Soloist (manually maintained)");
    eprint!("  Choose [1]: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    match answer.trim() {
        "" | "1" => Ok(false),
        "2" => Ok(true),
        _ => bail!("invalid Soloist installation choice"),
    }
}

fn save_soloist_websocket(paths: &Paths, endpoint: &str) -> Result<()> {
    let config_path = paths.config.join("config.json");
    if config_path.is_symlink() {
        bail!("refusing to replace symlinked display settings");
    }
    let mut settings: serde_json::Value = match fs::read(&config_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("parse display settings")?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error.into()),
    };
    let object = settings
        .as_object_mut()
        .ok_or_else(|| anyhow!("display settings must be a JSON object"))?;
    object.insert(
        "soloistWebSocket".to_owned(),
        serde_json::Value::String(endpoint.to_owned()),
    );
    crate::state::atomic_json(&config_path, &settings, Some(0o600))
}

pub struct InstallOptions {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub external_soloist: bool,
    pub soloist_ws: Option<String>,
    pub start_external: bool,
}

pub fn install(
    paths: &Paths,
    platform: &Platform,
    yes: bool,
    quiet: bool,
    options: InstallOptions,
) -> Result<()> {
    let InstallOptions {
        name,
        api_key,
        external_soloist,
        soloist_ws,
        start_external,
    } = options;
    paths.ensure_management_dirs()?;
    recover(paths)?;
    if !service::user_manager_available(paths) {
        bail!("a working systemd user manager is required; log in normally and retry");
    }
    if !yes && !io::stdin().is_terminal() {
        bail!("non-interactive installation requires --yes");
    }
    let previous_state = InstallState::load(paths)?;
    let discovered_soloist =
        soloist::discover(paths, previous_state.as_ref().map(|state| &state.soloist));
    let use_external = external_soloist || select_external_soloist(&discovered_soloist, yes)?;
    if use_external
        && previous_state
            .as_ref()
            .is_some_and(|state| state.soloist.ownership == SoloistOwnership::Installer)
    {
        bail!("switching a managed installation to external Soloist requires stopping the managed service first");
    }
    let verified_external = if use_external {
        Some(soloist::verify_external(
            paths,
            &discovered_soloist,
            soloist_ws.as_deref(),
            start_external,
        )?)
    } else {
        None
    };
    let existing_unit =
        if use_external || discovered_soloist.ownership == SoloistOwnership::Installer {
            None
        } else {
            Some(service::SoloistUnitSnapshot::capture(paths)?)
        };
    let existing_settings =
        if !use_external && discovered_soloist.ownership == SoloistOwnership::Independent {
            legacy::inspect(paths)?
        } else {
            None
        };
    let original_runtime = fs::read(paths.state.join("soloist-runtime.json")).ok();
    let original_current = fs::read_link(&paths.managed_soloist_current).ok();
    let original_settings = fs::read(paths.config.join("config.json")).ok();
    let original_metadata = fs::read(&paths.install_state).ok();
    let original_unit_backup_exists = service::SoloistUnitSnapshot::load(paths)?.is_some();
    let progress = Progress::new(quiet, "Installing Soloist Display");
    if !quiet && !io::stderr().is_terminal() {
        let status = match (
            &discovered_soloist.ownership,
            soloist::healthy(&discovered_soloist),
        ) {
            (SoloistOwnership::None, _) => "not installed",
            (SoloistOwnership::Installer, true) => "installer-owned and running",
            (SoloistOwnership::Installer, false) => "installer-owned but stopped",
            (SoloistOwnership::Independent, true) => "existing installation is running",
            (SoloistOwnership::Independent, false) => {
                "existing installation is stopped or incomplete"
            }
        };
        eprintln!("  Soloist Display · {}", platform.label);
        eprintln!("  Soloist: {status}");
    }
    // A failed first install can leave our systemd unit running while the
    // transaction rolls back before writing installation metadata. Reclaim
    // only our recognizable unit; never stop an unrelated port owner.
    if previous_state.is_none() && (!port_available(8080) || !port_available(9091)) {
        let owned_unit = fs::read_to_string(&paths.display_service)
            .ok()
            .is_some_and(|unit| {
                unit.contains("Description=Soloist Display local server and audio spectrum")
                    && unit.contains("soloist-audio-bridge")
                    && unit.contains(&paths.current.display().to_string())
            });
        if owned_unit && !paths.is_test() {
            let output = Command::new("systemctl")
                .args(["--user", "stop", "soloist-display.service"])
                .output()
                .context("stop orphaned Soloist Display service")?;
            if !output.status.success() {
                bail!("could not stop the orphaned Soloist Display service");
            }
            // Wait briefly for the bridge to release its listeners.
            for _ in 0..20 {
                if port_available(8080) && port_available(9091) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    if !port_available(8080) && previous_state.is_none() {
        bail!("port 8080 is already in use; stop its owner and retry");
    }
    if !port_available(9091) && previous_state.is_none() {
        bail!("port 9091 is already in use; stop its owner and retry");
    }
    if !use_external && !port_available(9090) && !service::soloist_unit_present(paths) {
        bail!(
            "port 9090 belongs to a process outside soloist.service; stop it before switching to managed Soloist"
        );
    }
    let (mut transaction, journal_path) = TransactionJournal::begin(paths, "install")?;
    if let Some(unit) = &existing_unit {
        unit.persist(paths)?;
    }
    let service_dir = paths
        .soloist_service
        .parent()
        .context("missing systemd user unit directory")?;
    transaction.install_recovery = Some(InstallRecovery {
        previous_settings: original_settings.clone(),
        previous_config_schema: fs::read(paths.state.join("config-schema.json")).ok(),
        previous_metadata: original_metadata.clone(),
        previous_env: fs::read(&paths.soloist_env).ok(),
        previous_runtime: original_runtime.clone(),
        previous_display_unit: fs::read(&paths.display_service).ok(),
        previous_soloist_unit: fs::read(&paths.soloist_service).ok(),
        previous_renewal_service: fs::read(service_dir.join("soloist-display-renew.service")).ok(),
        previous_renewal_timer: fs::read(service_dir.join("soloist-display-renew.timer")).ok(),
        previous_soloist_link: original_current.clone(),
        previous_display_active: !paths.is_test()
            && service::active(paths, "soloist-display.service"),
        previous_display_enabled: service::enabled(paths, "soloist-display.service"),
        previous_soloist_active: !paths.is_test() && service::active(paths, "soloist.service"),
        previous_soloist_enabled: service::enabled(paths, "soloist.service"),
        previous_timer_active: !paths.is_test()
            && service::active(paths, "soloist-display-renew.timer"),
        previous_timer_enabled: service::enabled(paths, "soloist-display-renew.timer"),
        restore_existing_service: existing_unit.is_some(),
        unit_backup_existed: original_unit_backup_exists,
    });
    transaction.previous_current = previous_state
        .as_ref()
        .map(|state| state.active_release.clone());
    if !dependencies::runtime_ready() {
        transaction.package_changes = dependencies::required(platform)
            .into_iter()
            .map(str::to_owned)
            .collect();
    }
    transaction.persist(&journal_path)?;
    if !quiet {
        progress.step(1, "Checking dependencies");
    }
    let package_result = {
        let _pause = (!dependencies::runtime_ready()).then(|| progress.pause());
        dependencies::install(platform, quiet)
    };
    let packages = match package_result {
        Ok(packages) => packages,
        Err(error) => {
            transaction.fail(&journal_path, error.to_string()).ok();
            return Err(error);
        }
    };
    if !quiet {
        progress.step(2, "Downloading & verifying");
    }
    let client = ReleaseClient::new()?;
    let (manifest, _) = match client.latest() {
        Ok(release) => release,
        Err(error) => {
            transaction.fail(&journal_path, error.to_string()).ok();
            return Err(error);
        }
    };
    if let Some(installed) = &previous_state {
        if manifest.version < semver::Version::parse(&installed.version)?
            && env::var("SOLOIST_DISPLAY_ALLOW_DOWNGRADE").as_deref() != Ok("1")
        {
            bail!(
                "refusing to downgrade from {} to {}",
                installed.version,
                manifest.version
            );
        }
    }
    let result = (|| {
        let release = install_artifacts(
            paths,
            platform,
            &client,
            &manifest,
            &mut transaction,
            &journal_path,
        )?;
        transaction.set_status(&journal_path, TransactionStatus::Activating)?;
        atomic_link(&release, &paths.current)?;
        install_visible_command(paths)?;
        migrations::migrate(paths)?;
        let soloist_state = if let Some(external) = &verified_external {
            external.clone()
        } else if discovered_soloist.ownership == SoloistOwnership::Installer {
            let existing = discovered_soloist.clone();
            {
                let _pause = progress.pause();
                soloist::configure(paths, name.clone(), api_key.clone())?;
            }
            soloist::start_existing(paths, &existing)?;
            existing
        } else {
            if let Some(settings) = &existing_settings {
                service::stop_soloist_existing(paths)?;
                if !paths.is_test() {
                    let endpoint = url::Url::parse(&settings.runtime.websocket_url)?;
                    let port = endpoint
                        .port_or_known_default()
                        .ok_or_else(|| anyhow!("previous Soloist WebSocket has no port"))?;
                    let mut stopped = false;
                    for _ in 0..30 {
                        if !service::active(paths, "soloist.service") && port_available(port) {
                            stopped = true;
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                    if !stopped {
                        bail!("previous Soloist did not release its WebSocket listener; refusing to copy a potentially live session");
                    }
                }
                let mut runtime = settings.runtime.clone();
                if settings.runtime.data_dir.exists() {
                    let cloned_data = paths.managed_soloist.join("session");
                    transaction.record_created(&journal_path, cloned_data.clone())?;
                    runtime.data_dir =
                        soloist::copy_existing_session(paths, &settings.runtime.data_dir)?;
                } else {
                    crate::terminal::log_event(
                        "migration",
                        "warning",
                        "existing Soloist session directory is missing; a new session will be created",
                    );
                    runtime.data_dir = paths.soloist_data.clone();
                }
                runtime.save(paths)?;
            }
            let soloist_name = name.or_else(|| {
                existing_settings
                    .as_ref()
                    .and_then(|settings| settings.name.clone())
            });
            let soloist_key = api_key.or_else(|| {
                existing_settings
                    .as_ref()
                    .and_then(|settings| settings.api_key.clone())
            });
            {
                let _pause = progress.pause();
                soloist::configure(paths, soloist_name.clone(), soloist_key.clone())?;
            }
            soloist::install(
                paths,
                platform,
                &manifest.version.to_string(),
                soloist_name,
                soloist_key,
            )?
        };
        save_soloist_websocket(
            paths,
            soloist_state
                .websocket_url
                .as_deref()
                .ok_or_else(|| anyhow!("Soloist WebSocket is not configured"))?,
        )?;
        soloist::ensure_cli_discovery(paths, &soloist_state)?;
        transaction.set_status(&journal_path, TransactionStatus::Services)?;
        if !quiet {
            progress.step(3, "Starting services");
        }
        service::install_display(paths)?;
        let desktop = desktop_files(paths)?;
        transaction.set_status(&journal_path, TransactionStatus::HealthCheck)?;
        if !quiet {
            progress.step(4, "Checking health");
        }
        wait_for_install_health(paths, &soloist_state)?;
        let mut managed = vec![paths.visible_command.clone(), paths.display_service.clone()];
        managed.extend(desktop);
        if soloist_state.ownership == SoloistOwnership::Installer {
            managed.extend([
                paths.soloist_service.clone(),
                paths.soloist_env.clone(),
                paths.managed_soloist.clone(),
            ]);
        }
        InstallState {
            schema: crate::state::STATE_SCHEMA,
            version: manifest.version.to_string(),
            channel: Some(
                previous_state
                    .as_ref()
                    .map(|state| state.effective_channel())
                    .unwrap_or_else(|| ReleaseChannel::for_version(&manifest.version)),
            ),
            suppress_updates: previous_state
                .as_ref()
                .is_some_and(|state| state.suppress_updates),
            target: platform.target.clone(),
            active_release: release,
            previous_release: previous_state.as_ref().map(|s| s.active_release.clone()),
            installed_at: now(),
            soloist: soloist_state.clone(),
            managed_paths: managed,
            dependency_packages: packages,
        }
        .save(paths)?;
        if soloist_state.ownership == SoloistOwnership::Installer {
            service::install_renewal_timer(paths)?;
        }
        transaction.set_status(&journal_path, TransactionStatus::Committed)?;
        Ok::<_, anyhow::Error>(())
    })();
    if let Err(error) = result {
        drop(progress);
        transaction.fail(&journal_path, error.to_string()).ok();
        recover(paths)
            .with_context(|| format!("installation failed: {error:#}; rollback incomplete"))?;
        return Err(error.context("installation failed; changes were rolled back"));
    }
    drop(progress);
    if !quiet {
        let on_path = env::var_os("PATH")
            .is_some_and(|value| env::split_paths(&value).any(|path| path == paths.bin_dir));
        if on_path {
            println!(
                "Soloist Display {} is ready. Run `soloist-display`.",
                manifest.version
            );
        } else {
            println!(
                "Soloist Display {} is ready. Open a new terminal or run `{}`.",
                manifest.version,
                paths.visible_command.display()
            );
        }
    }
    Ok(())
}

pub fn update(
    paths: &Paths,
    platform: &Platform,
    quiet: bool,
    channel: Option<ReleaseChannel>,
    version: Option<&str>,
    allow_downgrade: bool,
) -> Result<()> {
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    let client = ReleaseClient::new()?;
    let requested_channel = channel.unwrap_or_else(|| state.effective_channel());
    let (manifest, _) = client.select(Some(requested_channel), version)?;
    let installed_version =
        semver::Version::parse(&state.version).context("installed version metadata is invalid")?;
    if manifest.version < installed_version
        && !allow_downgrade
        && env::var("SOLOIST_DISPLAY_ALLOW_DOWNGRADE").as_deref() != Ok("1")
    {
        bail!(
            "refusing to downgrade from {} to {}",
            state.version,
            manifest.version
        );
    }
    if manifest.version.to_string() == state.version {
        if (channel.is_some() || version.is_some())
            && state.channel
                != Some(if version.is_some() && channel.is_none() {
                    ReleaseChannel::for_version(&manifest.version)
                } else {
                    requested_channel
                })
        {
            let mut next = state.clone();
            next.channel = Some(if version.is_some() && channel.is_none() {
                ReleaseChannel::for_version(&manifest.version)
            } else {
                requested_channel
            });
            next.save(paths)?;
        }
        if let Err(error) = install_completions(paths) {
            eprintln!("warning: could not refresh shell completions: {error:#}");
        }
        crate::terminal::success(format!("Soloist Display {} is up to date.", state.version));
        return Ok(());
    }
    let progress = Progress::new(quiet, "Updating Soloist Display");
    progress.step(1, "Checking release");
    progress.step(2, "Downloading & verifying");
    let (mut tx, tx_path) = TransactionJournal::begin(paths, "update")?;
    tx.previous_current = Some(state.active_release.clone());
    tx.persist(&tx_path)?;
    let result = (|| {
        let release = install_artifacts(paths, platform, &client, &manifest, &mut tx, &tx_path)?;
        tx.set_status(&tx_path, TransactionStatus::Activating)?;
        atomic_link(&release, &paths.current)?;
        migrations::migrate(paths)?;
        let mut next_soloist = state.soloist.clone();
        if state.soloist.ownership == SoloistOwnership::Installer
            && !state
                .soloist
                .executable
                .as_ref()
                .is_some_and(|path| path.is_file())
        {
            {
                let _pause = progress.pause();
                soloist::configure(paths, None, None)?;
            }
            next_soloist =
                soloist::install(paths, platform, &manifest.version.to_string(), None, None)?;
        }
        progress.step(3, "Starting services");
        service::install_display(paths)?;
        progress.step(4, "Checking health");
        wait_for_install_health(paths, &next_soloist)?;
        let mut next = state.clone();
        next.previous_release = Some(state.active_release.clone());
        next.active_release = release;
        next.version = manifest.version.to_string();
        next.channel = Some(if channel.is_some() {
            requested_channel
        } else if version.is_some() {
            ReleaseChannel::for_version(&manifest.version)
        } else {
            requested_channel
        });
        next.target = platform.target.clone();
        next.soloist = next_soloist;
        next.save(paths)?;
        if next.soloist.ownership == SoloistOwnership::Installer {
            service::install_renewal_timer(paths)?;
        }
        tx.set_status(&tx_path, TransactionStatus::Committed)
    })();
    if let Err(error) = result {
        drop(progress);
        atomic_link(&state.active_release, &paths.current).ok();
        if state.soloist.ownership == SoloistOwnership::Installer {
            if let Some(previous_executable) = &state.soloist.executable {
                if let Some(previous_directory) = previous_executable.parent() {
                    atomic_link(previous_directory, &paths.managed_soloist_current).ok();
                    service::install_soloist(paths, previous_executable).ok();
                }
            }
        }
        service::install_display(paths).ok();
        tx.fail(&tx_path, error.to_string()).ok();
        recover(paths).context("update failed and recovery could not finish")?;
        return Err(error.context("update failed; restored the previous release"));
    }
    drop(progress);
    if let Err(error) = install_completions(paths) {
        eprintln!("warning: could not refresh shell completions: {error:#}");
    }
    if !quiet {
        println!("Updated Soloist Display to {}.", manifest.version);
        if state.suppress_updates {
            println!("Update notifications are off. Re-enable: soloist-display notifications on");
        }
    }
    Ok(())
}

pub fn repair(paths: &Paths, platform: &Platform, quiet: bool) -> Result<()> {
    let progress = Progress::new(quiet, "Repairing Soloist Display");
    progress.step(1, "Checking files");
    let mut state = InstallState::load(paths)?
        .ok_or_else(|| anyhow!("Soloist Display is not installed; run the installer first"))?;
    migrations::migrate(paths)?;
    let client = ReleaseClient::new()?;
    let (manifest, _) = client.select(None, Some(&state.version))?;
    let release_valid = ["manager", "audio-bridge"].iter().all(|kind| {
        manifest
            .artifact(kind, platform)
            .ok()
            .is_some_and(|artifact| {
                artifact_destination(&state.active_release, artifact)
                    .ok()
                    .filter(|path| path.is_file())
                    .and_then(|path| crate::release::sha256_file(&path).ok())
                    .is_some_and(|digest| digest == artifact.sha256)
            })
    });
    if !release_valid {
        progress.step(2, "Restoring files");
        let original_release = state.active_release.clone();
        let backup = paths
            .releases
            .join(format!(".repair-backup-{}", std::process::id()));
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        let had_original = original_release.exists();
        if had_original {
            fs::rename(&original_release, &backup).context("back up corrupt active release")?;
        }
        let (mut tx, tx_path) = TransactionJournal::begin(paths, "repair")?;
        tx.previous_current = Some(original_release.clone());
        if had_original {
            tx.backups.push(crate::state::Backup {
                original: original_release.clone(),
                backup: backup.clone(),
            });
        }
        tx.persist(&tx_path)?;
        let repaired = (|| {
            let release =
                install_artifacts(paths, platform, &client, &manifest, &mut tx, &tx_path)?;
            atomic_link(&release, &paths.current)?;
            state.active_release = release;
            state.save(paths)?;
            service::install_display(paths)?;
            wait_for_install_health(paths, &state.soloist)?;
            tx.set_status(&tx_path, TransactionStatus::Committed)?;
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = repaired {
            fs::remove_dir_all(&state.active_release).ok();
            if had_original {
                fs::rename(&backup, &original_release).ok();
            }
            state.active_release = original_release;
            state.save(paths).ok();
            atomic_link(&state.active_release, &paths.current).ok();
            tx.fail(&tx_path, error.to_string()).ok();
            recover(paths).context("repair failed and recovery could not finish")?;
            return Err(error.context("repair failed; restored the previous files"));
        }
        if had_original {
            fs::remove_dir_all(backup).ok();
        }
    }
    progress.step(3, "Restoring services");
    atomic_link(&state.active_release, &paths.current)?;
    install_visible_command(paths)?;
    desktop_files(paths)?;
    service::install_display(paths)?;
    match state.soloist.ownership {
        SoloistOwnership::Installer => {
            {
                let _pause = progress.pause();
                soloist::configure(paths, None, None)?;
            }
            if !paths.managed_soloist_current.join("soloist").is_file() {
                state.soloist =
                    soloist::install(paths, platform, &manifest.version.to_string(), None, None)?;
            } else {
                service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
            }
            service::install_renewal_timer(paths)?;
        }
        SoloistOwnership::Independent => {
            let _pause = progress.pause();
            state.soloist = soloist::verify_external(
                paths,
                &state.soloist,
                state.soloist.websocket_url.as_deref(),
                false,
            )?;
        }
        SoloistOwnership::None => {
            bail!("Soloist mode is not configured; reinstall Soloist Display");
        }
    }
    save_soloist_websocket(
        paths,
        state
            .soloist
            .websocket_url
            .as_deref()
            .ok_or_else(|| anyhow!("Soloist WebSocket endpoint is missing"))?,
    )?;
    progress.step(4, "Checking health");
    wait_for_install_health(paths, &state.soloist)?;
    state.save(paths)?;
    drop(progress);
    if !quiet {
        println!("Repair complete.");
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct UpdateCache {
    checked_at: u64,
    channel: ReleaseChannel,
    latest: String,
}

fn is_newer(candidate: &str, installed: &str) -> bool {
    match (
        semver::Version::parse(candidate),
        semver::Version::parse(installed),
    ) {
        (Ok(candidate), Ok(installed)) => candidate > installed,
        _ => false,
    }
}

pub fn check_update_briefly(paths: &Paths, state: &InstallState) -> Option<String> {
    if state.suppress_updates {
        return None;
    }
    let channel = state.effective_channel();
    let cache_path = paths.cache.join("update.json");
    if let Ok(bytes) = fs::read(&cache_path) {
        if let Ok(cache) = serde_json::from_slice::<UpdateCache>(&bytes) {
            if cache.channel == channel && now().saturating_sub(cache.checked_at) < 86_400 {
                return is_newer(&cache.latest, &state.version).then_some(cache.latest);
            }
        }
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let result = ReleaseClient::new()
            .and_then(|client| client.select(Some(channel), None))
            .map(|(manifest, _)| manifest.version.to_string());
        let _ = sender.send(result);
    });
    if let Ok(Ok(latest)) = receiver.recv_timeout(Duration::from_millis(750)) {
        fs::create_dir_all(&paths.cache).ok();
        crate::state::atomic_json(
            &cache_path,
            &UpdateCache {
                checked_at: now(),
                channel,
                latest: latest.clone(),
            },
            Some(0o600),
        )
        .ok();
        if is_newer(&latest, &state.version) {
            return Some(latest);
        }
    }
    None
}

pub fn set_update_notifications(paths: &Paths, enabled: bool) -> Result<()> {
    let mut state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    state.suppress_updates = !enabled;
    state.save(paths)?;
    println!(
        "Update notifications {}.",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

pub fn update_status(paths: &Paths) -> Result<()> {
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    let client = ReleaseClient::new()?;
    println!("Installed: {}", state.version);
    println!("Channel: {:?}", state.effective_channel());
    println!(
        "Notifications: {}",
        if state.suppress_updates { "off" } else { "on" }
    );
    for channel in [
        ReleaseChannel::Stable,
        ReleaseChannel::Alpha,
        ReleaseChannel::Beta,
    ] {
        match client.select(Some(channel), None) {
            Ok((manifest, _)) => {
                let version = manifest.version.to_string();
                let relation = match semver::Version::parse(&state.version) {
                    Ok(current) if manifest.version > current => "upgrade available",
                    Ok(current) if manifest.version < current => "older than installed",
                    Ok(_) => "current",
                    Err(_) => "installed version unknown",
                };
                println!("{channel:?}: {version} ({relation})");
            }
            Err(error) => println!("{channel:?}: unavailable ({error:#})"),
        }
    }
    Ok(())
}

pub fn launch(paths: &Paths, settings: bool) -> Result<()> {
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    service::start_display(paths)?;
    wait_http(paths)?;
    let mut browser_update_notice = None;
    if let Some(latest) = check_update_briefly(paths, &state) {
        eprintln!(
            "Soloist Display {latest} is available on the {:?} channel.",
            state.effective_channel()
        );
        if io::stdin().is_terminal() {
            eprint!("Update now? [y/N] ");
            io::stderr().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                let platform = Platform::detect()?;
                let _lock =
                    crate::lock::InstallLock::acquire(&paths.lock, Duration::from_secs(15))?;
                update(paths, &platform, false, None, None, false)?;
            }
        } else {
            eprintln!("Run `soloist-display update` to install it.");
            browser_update_notice = Some(latest);
        }
    }
    let mut url = if settings {
        "http://127.0.0.1:8080/settings".to_owned()
    } else {
        "http://127.0.0.1:8080/".to_owned()
    };
    if let Some(version) = browser_update_notice {
        url.push_str("?update=");
        url.push_str(&version);
    }
    if paths.is_test() {
        return Ok(());
    }
    // xdg-open may remain attached to a browser process. Do not block the CLI
    // or forward Ctrl+C to the browser through the terminal's process group.
    use std::process::Stdio;
    Command::new("xdg-open")
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("could not launch the default browser")?;
    Ok(())
}

pub fn doctor(paths: &Paths) -> Result<bool> {
    let mut ok = true;
    crate::cli_output::heading("Linux installation");
    crate::cli_output::section("Diagnostics");
    let state = InstallState::load(paths)?;
    macro_rules! check {
        ($label:expr, $value:expr, $help:expr) => {{
            let value = $value;
            let hint = $help;
            if !crate::cli_output::check($label, value, hint.as_ref()) {
                ok = false;
            }
        }};
    }
    check!(
        "Installation metadata",
        state.is_some(),
        "Run the one-command installer."
    );
    check!(
        "Display executable",
        paths.current.join("bin/soloist-audio-bridge").is_file(),
        "Run `soloist-display repair`."
    );
    check!(
        "systemd user manager",
        service::user_manager_available(paths),
        "Use a normal graphical/user login with systemd."
    );
    check!(
        "Display service",
        service::active(paths, "soloist-display.service"),
        "Run `soloist-display repair`; inspect journalctl --user -u soloist-display."
    );
    check!(
        "Display HTTP :8080",
        paths.is_test() || TcpStream::connect("127.0.0.1:8080").is_ok(),
        "Port 8080 is unavailable; inspect the display service and check for a conflict."
    );
    check!(
        "Audio WebSocket :9091",
        paths.is_test() || TcpStream::connect("127.0.0.1:9091").is_ok(),
        "Port 9091 is unavailable; inspect the display service and check for a conflict."
    );
    if let Some(state) = state {
        let endpoint = state
            .soloist
            .websocket_url
            .as_deref()
            .unwrap_or("(missing)");
        let mode = match state.soloist.ownership {
            SoloistOwnership::Installer => "managed",
            SoloistOwnership::Independent => "external",
            SoloistOwnership::None => "unconfigured",
        };
        crate::cli_output::field("Soloist mode", mode);
        crate::cli_output::field("Soloist endpoint", endpoint);
        check!(
            "Soloist WebSocket",
            paths.is_test() || soloist::external_healthy(&state.soloist),
            format!(
                "No valid Soloist WebSocket response at {endpoint}; check its listener and service."
            )
        );
        match state.soloist.ownership {
            SoloistOwnership::Installer => {
                let runtime_path = paths.state.join("soloist-runtime.json");
                let runtime = service::ManagedRuntime::load(paths);
                check!(
                    "Runtime configuration",
                    runtime_path.is_file() && runtime.is_ok(),
                    "Managed Soloist runtime metadata is unavailable; check the state directory before repairing."
                );
                if let Ok(runtime) = runtime {
                    crate::cli_output::field("Soloist data directory", runtime.data_dir.display());
                    crate::cli_output::field(
                        "Soloist cache directory",
                        runtime.cache_dir.display(),
                    );
                    check!(
                        "WebSocket configuration",
                        Some(runtime.websocket_url.as_str())
                            == state.soloist.websocket_url.as_deref(),
                        "The managed service and Display WebSocket endpoints disagree."
                    );
                }
                check!(
                    "Owned service file",
                    fs::read_to_string(&paths.soloist_service)
                        .ok()
                        .is_some_and(|unit| unit
                            .contains(&paths.managed_soloist_current.display().to_string())),
                    "soloist.service does not point at the managed Soloist binary."
                );
                check!(
                    "Managed Soloist",
                    paths.managed_soloist_current.join("soloist").is_file(),
                    "Run `soloist-display repair` to restore the managed executable."
                );
                check!(
                    "Soloist service",
                    service::active(paths, "soloist.service"),
                    "Run `soloist-display repair` or inspect journalctl --user -u soloist."
                );
                check!(
                    "Renewal timer",
                    service::active(paths, "soloist-display-renew.timer"),
                    "Run `soloist-display repair` to enable automatic renewal."
                );
                let secure = fs::metadata(&paths.soloist_env)
                    .map(|metadata| metadata.permissions().mode() & 0o077 == 0)
                    .unwrap_or(false);
                check!(
                    "Soloist credentials",
                    secure,
                    "Run chmod 600 ~/.config/soloist-display/soloist.env and repair."
                );
            }
            SoloistOwnership::Independent => {
                let present = service::soloist_unit_present(paths);
                crate::cli_output::field(
                    "External systemd unit",
                    if present {
                        "detected (not managed)"
                    } else {
                        "absent (optional)"
                    },
                );
                crate::cli_output::field("Automatic renewal", "disabled (external mode)");
            }
            SoloistOwnership::None => {
                check!(
                    "Soloist configuration",
                    false,
                    "Reinstall with managed or external Soloist."
                );
            }
        }
    }
    check!(
        "Audio tools",
        process::exists("pactl") && process::exists("parec"),
        "Install PipeWire PulseAudio compatibility tools."
    );
    if process::exists("pactl") {
        let monitor = audio_monitor_available();
        check!(
            "Audio monitor",
            monitor,
            "Start PipeWire/PulseAudio and select a default output sink."
        );
    }
    Ok(ok)
}

fn choose_uninstall_options(
    options: UninstallOptions,
    managed: bool,
    previous: bool,
) -> Result<(Option<SoloistRemoval>, DisplayDataRemoval)> {
    if !managed && options.soloist.is_some() {
        bail!("external Soloist is not managed by Display; --soloist cannot be used");
    }
    let soloist = if !managed {
        None
    } else if let Some(action) = options.soloist {
        Some(action)
    } else if options.yes {
        Some(if previous {
            SoloistRemoval::Restore
        } else {
            SoloistRemoval::Keep
        })
    } else {
        if !io::stdin().is_terminal() {
            bail!("non-interactive uninstall requires --soloist and --data, or --yes");
        }
        if previous {
            eprintln!("Soloist: 1 Restore previous installation (recommended)  2 Keep managed");
        } else {
            eprintln!("Soloist: 1 Keep managed (recommended)  2 Remove managed");
        }
        eprint!("Choose [1]: ");
        io::stderr().flush()?;
        let mut value = String::new();
        io::stdin().read_line(&mut value)?;
        Some(match (previous, value.trim()) {
            (true, "" | "1") => SoloistRemoval::Restore,
            (true, "2") | (false, "" | "1") => SoloistRemoval::Keep,
            (false, "2") => SoloistRemoval::Remove,
            _ => bail!("invalid Soloist uninstall choice"),
        })
    };
    if matches!(soloist, Some(SoloistRemoval::Restore)) && !previous {
        bail!("cannot restore Soloist: there is no usable previous service snapshot");
    }
    if matches!(soloist, Some(SoloistRemoval::Remove)) && previous {
        bail!(
            "a previous Soloist installation exists; choose --soloist restore to avoid losing it"
        );
    }
    let data = if let Some(action) = options.data {
        action
    } else if options.yes {
        DisplayDataRemoval::Keep
    } else {
        if !io::stdin().is_terminal() {
            bail!("non-interactive uninstall requires --data, or --yes");
        }
        eprintln!("Display settings: 1 Keep (recommended)  2 Remove settings and cache");
        eprint!("Choose [1]: ");
        io::stderr().flush()?;
        let mut value = String::new();
        io::stdin().read_line(&mut value)?;
        match value.trim() {
            "" | "1" => DisplayDataRemoval::Keep,
            "2" => DisplayDataRemoval::Remove,
            _ => bail!("invalid Display data choice"),
        }
    };
    Ok((soloist, data))
}

pub fn uninstall(paths: &Paths, options: UninstallOptions, quiet: bool) -> Result<()> {
    let Some(state) = InstallState::load(paths)? else {
        if !paths.display_service.exists() && !paths.current.exists() && !paths.current.is_symlink()
        {
            return Ok(());
        }
        bail!("Soloist Display metadata is missing; recover or repair before uninstalling");
    };
    let managed = state.soloist.ownership == SoloistOwnership::Installer;
    let previous = if managed {
        service::SoloistUnitSnapshot::load(paths)?
    } else {
        None
    };
    let has_previous = previous
        .as_ref()
        .is_some_and(|snapshot| snapshot.has_previous());
    let (soloist_action, data_action) = choose_uninstall_options(options, managed, has_previous)?;
    let progress = Progress::new(quiet, "Removing Soloist Display");
    let mut transaction = crate::uninstall_tx::UninstallTransaction::begin(paths, managed)?;
    let result = (|| -> Result<()> {
        progress.step(1, "Stopping services");
        service::disable_owned(paths, "soloist-display.service")?;
        if mpris::is_installed(paths)? {
            mpris::stop_for_uninstall(paths)?;
        }
        match soloist_action {
            Some(SoloistRemoval::Restore) => {
                previous
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing previous Soloist service"))?
                    .restore(paths)
                    .context("restore previous Soloist before removing managed binary")?;
            }
            Some(SoloistRemoval::Remove) => {
                service::disable_owned(paths, "soloist.service")?;
            }
            Some(SoloistRemoval::Keep) | None => {}
        }
        service::remove_renewal_timer(paths)?;
        progress.step(2, "Removing files");
        if mpris::is_installed(paths)? {
            transaction.stage(paths, &mpris::unit_path(paths))?;
            transaction.stage(paths, &paths.root.join("addons/mpris"))?;
            transaction.stage(paths, &paths.state.join("mpris.json"))?;
        }
        for (_, file) in completion_paths(paths) {
            transaction.stage(paths, &file)?;
        }
        for path in [
            &paths.display_service,
            &paths.visible_command,
            &paths.applications.join("soloist-display.desktop"),
            &paths.applications.join("soloist-display-settings.desktop"),
        ] {
            transaction.stage(paths, path)?;
        }
        if matches!(soloist_action, Some(SoloistRemoval::Remove)) {
            transaction.stage(paths, &paths.soloist_service)?;
            transaction.stage(paths, &paths.soloist_env)?;
        }
        if matches!(
            soloist_action,
            Some(SoloistRemoval::Remove | SoloistRemoval::Restore)
        ) {
            transaction.stage(paths, &paths.managed_soloist)?;
        }
        if data_action == DisplayDataRemoval::Remove {
            transaction.stage(paths, &paths.config.join("config.json"))?;
            transaction.stage(paths, &paths.cache)?;
        }
        transaction.stage(paths, &paths.current)?;
        transaction.stage(paths, &paths.releases)?;
        progress.step(3, "Checking services");
        service::daemon_reload(paths)?;
        if matches!(soloist_action, Some(SoloistRemoval::Restore))
            && previous
                .as_ref()
                .is_some_and(|snapshot| snapshot.was_active())
            && !service::active(paths, "soloist.service")
        {
            bail!("restored Soloist service is not active; uninstall was cancelled");
        }
        progress.step(4, "Finishing");
        transaction.commit(paths)
    })();
    if let Err(error) = result {
        crate::uninstall_tx::recover(paths)
            .context("uninstall failed and recovery could not complete")?;
        return Err(error.context("uninstall did not complete"));
    }
    if let Err(error) = remove_completions(paths) {
        eprintln!("warning: could not remove shell completion integration: {error:#}");
    }
    drop(progress);
    if !quiet {
        if matches!(soloist_action, Some(SoloistRemoval::Keep)) {
            eprintln!("  Managed Soloist was kept; automatic binary renewal is disabled.");
        }
        println!("Soloist Display removed.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SoloistState, TransactionStatus};
    use serial_test::serial;

    fn test_paths() -> (tempfile::TempDir, Paths) {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var(
            "SOLOIST_DISPLAY_TEST_ROOT",
            directory.path().join("root with spaces"),
        );
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        (directory, paths)
    }

    fn installed_state(paths: &Paths, soloist_state: SoloistState) -> InstallState {
        let release = paths.releases.join("1.0.0-test");
        fs::create_dir_all(release.join("bin")).unwrap();
        fs::write(release.join("bin/soloist-display"), b"manager").unwrap();
        fs::write(release.join("bin/soloist-audio-bridge"), b"bridge").unwrap();
        InstallState {
            schema: crate::state::STATE_SCHEMA,
            version: "1.0.0".into(),
            channel: None,
            suppress_updates: false,
            target: "test".into(),
            active_release: release,
            previous_release: None,
            installed_at: now(),
            soloist: soloist_state,
            managed_paths: vec![],
            dependency_packages: vec![],
        }
    }
    #[test]
    fn ports_detect_conflicts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(!port_available(listener.local_addr().unwrap().port()));
    }

    #[test]
    fn update_prompt_only_accepts_newer_semantic_versions() {
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.8.0", "1.9.0"));
        assert!(!is_newer("invalid", "1.9.0"));
    }

    #[test]
    fn insufficient_disk_space_fails_before_staging() {
        assert!(ensure_staging_space(16 * 1024 * 1024, 8 * 1024 * 1024).is_err());
        assert!(ensure_staging_space(64 * 1024 * 1024, 8 * 1024 * 1024).is_ok());
    }

    #[test]
    #[serial]
    fn failed_health_check_is_injectable_at_transaction_boundary() {
        let (_directory, paths) = test_paths();
        std::env::set_var("SOLOIST_DISPLAY_FAULT", "health_check");
        assert!(wait_for_install_health(&paths, &SoloistState::default()).is_err());
        std::env::remove_var("SOLOIST_DISPLAY_FAULT");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn crash_during_activation_recovers_previous_release() {
        let (_directory, paths) = test_paths();
        let previous = paths.releases.join("previous");
        let incomplete = paths.releases.join("incomplete");
        fs::create_dir_all(&previous).unwrap();
        fs::create_dir_all(&incomplete).unwrap();
        atomic_link(&incomplete, &paths.current).unwrap();
        let (mut transaction, journal_path) = TransactionJournal::begin(&paths, "update").unwrap();
        transaction.previous_current = Some(previous.clone());
        transaction.created.push(incomplete.clone());
        transaction.status = TransactionStatus::Activating;
        transaction.persist(&journal_path).unwrap();
        recover(&paths).unwrap();
        assert_eq!(fs::read_link(&paths.current).unwrap(), previous);
        assert!(!incomplete.exists());
        let journal: TransactionJournal =
            serde_json::from_slice(&fs::read(journal_path).unwrap()).unwrap();
        assert!(matches!(journal.status, TransactionStatus::RolledBack));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn interrupted_install_restores_settings_and_removes_new_service() {
        let (_directory, paths) = test_paths();
        fs::create_dir_all(&paths.config).unwrap();
        let config = paths.config.join("config.json");
        fs::write(&config, b"original settings").unwrap();
        let (mut tx, journal) = TransactionJournal::begin(&paths, "install").unwrap();
        tx.install_recovery = Some(InstallRecovery {
            previous_settings: Some(b"original settings".to_vec()),
            previous_config_schema: None,
            previous_metadata: None,
            previous_env: None,
            previous_runtime: None,
            previous_display_unit: None,
            previous_soloist_unit: None,
            previous_renewal_service: None,
            previous_renewal_timer: None,
            previous_soloist_link: None,
            previous_display_active: false,
            previous_display_enabled: false,
            previous_soloist_active: false,
            previous_soloist_enabled: false,
            previous_timer_active: false,
            previous_timer_enabled: false,
            restore_existing_service: false,
            unit_backup_existed: false,
        });
        tx.status = TransactionStatus::Services;
        tx.persist(&journal).unwrap();
        fs::write(&config, b"partial settings").unwrap();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        fs::write(&paths.soloist_service, b"new managed service").unwrap();
        fs::write(paths.state.join("soloist-runtime.json"), b"new runtime").unwrap();
        recover(&paths).unwrap();
        assert_eq!(fs::read(&config).unwrap(), b"original settings");
        assert!(!paths.soloist_service.exists());
        assert!(!paths.state.join("soloist-runtime.json").exists());
        recover(&paths).unwrap();
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn failed_migration_recovers_original_service_and_environment() {
        let (_directory, paths) = test_paths();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        fs::create_dir_all(&paths.config).unwrap();
        let unit = b"[Service]\nExecStart=/usr/bin/soloist\n";
        let env = b"SOLOIST_API_KEY=original\n";
        fs::write(&paths.soloist_service, unit).unwrap();
        fs::write(&paths.soloist_env, env).unwrap();
        let snapshot = service::SoloistUnitSnapshot::capture(&paths).unwrap();
        snapshot.persist(&paths).unwrap();
        let (mut tx, journal) = TransactionJournal::begin(&paths, "install").unwrap();
        tx.install_recovery = Some(InstallRecovery {
            previous_settings: None,
            previous_config_schema: None,
            previous_metadata: None,
            previous_env: Some(env.to_vec()),
            previous_runtime: None,
            previous_display_unit: None,
            previous_soloist_unit: Some(unit.to_vec()),
            previous_renewal_service: None,
            previous_renewal_timer: None,
            previous_soloist_link: None,
            previous_display_active: false,
            previous_display_enabled: false,
            previous_soloist_active: false,
            previous_soloist_enabled: false,
            previous_timer_active: false,
            previous_timer_enabled: false,
            restore_existing_service: true,
            unit_backup_existed: false,
        });
        tx.status = TransactionStatus::Failed;
        tx.persist(&journal).unwrap();
        fs::write(
            &paths.soloist_service,
            b"[Service]\nExecStart=/managed/soloist\n",
        )
        .unwrap();
        fs::write(&paths.soloist_env, b"SOLOIST_API_KEY=changed\n").unwrap();
        recover(&paths).unwrap();
        assert_eq!(fs::read(&paths.soloist_service).unwrap(), unit);
        assert_eq!(fs::read(&paths.soloist_env).unwrap(), env);
        assert!(service::SoloistUnitSnapshot::load(&paths)
            .unwrap()
            .is_none());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn interrupted_migration_restores_missing_credentials_as_missing() {
        let (_directory, paths) = test_paths();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        let original_unit =
            b"[Service]\nEnvironmentFile=/missing/soloist.env\nExecStart=/usr/bin/soloist\n";
        fs::write(&paths.soloist_service, original_unit).unwrap();
        assert!(!paths.soloist_env.exists());
        let snapshot = service::SoloistUnitSnapshot::capture(&paths).unwrap();
        snapshot.persist(&paths).unwrap();

        let (mut transaction, journal) = TransactionJournal::begin(&paths, "install").unwrap();
        transaction.install_recovery = Some(InstallRecovery {
            previous_settings: None,
            previous_config_schema: None,
            previous_metadata: None,
            previous_env: None,
            previous_runtime: None,
            previous_display_unit: None,
            previous_soloist_unit: Some(original_unit.to_vec()),
            previous_renewal_service: None,
            previous_renewal_timer: None,
            previous_soloist_link: None,
            previous_display_active: false,
            previous_display_enabled: false,
            previous_soloist_active: false,
            previous_soloist_enabled: false,
            previous_timer_active: false,
            previous_timer_enabled: false,
            restore_existing_service: true,
            unit_backup_existed: false,
        });
        transaction.status = TransactionStatus::Services;
        transaction.persist(&journal).unwrap();

        fs::write(&paths.soloist_service, b"new managed service").unwrap();
        fs::write(&paths.soloist_env, b"SOLOIST_API_KEY=temporary\n").unwrap();
        recover(&paths).unwrap();
        assert_eq!(fs::read(&paths.soloist_service).unwrap(), original_unit);
        assert!(!paths.soloist_env.exists());
        recover(&paths).unwrap();
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    fn uninstall_choices_validate_ownership() {
        let keep = UninstallOptions {
            soloist: Some(SoloistRemoval::Keep),
            data: Some(DisplayDataRemoval::Keep),
            yes: true,
        };
        assert!(choose_uninstall_options(
            UninstallOptions {
                soloist: keep.soloist,
                data: keep.data,
                yes: true
            },
            false,
            false
        )
        .is_err());
        assert!(choose_uninstall_options(
            UninstallOptions {
                soloist: Some(SoloistRemoval::Restore),
                data: Some(DisplayDataRemoval::Keep),
                yes: true
            },
            true,
            false
        )
        .is_err());
        assert!(choose_uninstall_options(
            UninstallOptions {
                soloist: Some(SoloistRemoval::Remove),
                data: Some(DisplayDataRemoval::Keep),
                yes: true
            },
            true,
            true
        )
        .is_err());
    }

    #[test]
    #[serial]
    fn interrupted_rollback_is_idempotent() {
        let (_directory, paths) = test_paths();
        let previous = paths.releases.join("previous");
        fs::create_dir_all(&previous).unwrap();
        fs::write(previous.join("still-here"), b"safe").unwrap();
        let (mut transaction, journal_path) = TransactionJournal::begin(&paths, "repair").unwrap();
        transaction.previous_current = Some(previous.clone());
        transaction.backups.push(crate::state::Backup {
            original: previous.clone(),
            backup: paths.releases.join("already-restored-backup"),
        });
        transaction.status = TransactionStatus::RollingBack;
        transaction.persist(&journal_path).unwrap();
        recover(&paths).unwrap();
        assert_eq!(fs::read(previous.join("still-here")).unwrap(), b"safe");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn external_to_managed_migration_preserves_custom_session_and_restores_unit() {
        let (_directory, paths) = test_paths();
        let data = paths.home.join("old soloist data");
        let cache = paths.home.join("old soloist cache");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("session"), b"existing session").unwrap();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        let original_unit = format!(
            "[Service]\nExecStart=/usr/bin/soloist --device-name Speaker --api-key secret --data-dir '{}' --cache-dir '{}' --ws 127.0.0.1:9190\n",
            data.display(),
            cache.display()
        );
        fs::write(&paths.soloist_service, &original_unit).unwrap();
        let snapshot = service::SoloistUnitSnapshot::capture(&paths).unwrap();
        snapshot.persist(&paths).unwrap();
        let settings = legacy::inspect(&paths).unwrap().unwrap();
        assert_eq!(settings.runtime.data_dir, data);
        let copy = soloist::copy_existing_session(&paths, &data).unwrap();
        let mut runtime = settings.runtime;
        runtime.data_dir = copy.clone();
        runtime.save(&paths).unwrap();
        soloist::configure(&paths, settings.name, settings.api_key).unwrap();
        let managed = paths.managed_soloist.join("current");
        fs::create_dir_all(&managed).unwrap();
        fs::write(managed.join("soloist"), b"managed").unwrap();
        service::install_soloist(&paths, &managed.join("soloist")).unwrap();
        let unit = fs::read_to_string(&paths.soloist_service).unwrap();
        assert!(unit.contains(&copy.to_string_lossy().to_string()));
        assert!(unit.contains("127.0.0.1:9190"));
        fs::write(copy.join("session"), b"managed session").unwrap();
        snapshot.restore(&paths).unwrap();
        assert_eq!(
            fs::read_to_string(&paths.soloist_service).unwrap(),
            original_unit
        );
        assert_eq!(fs::read(data.join("session")).unwrap(), b"existing session");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn live_uninstall_restores_original_service() {
        if std::env::var("SOLOIST_DISPLAY_RUN_SYSTEMD_TEST").as_deref() != Ok("1") {
            return;
        }
        let paths = Paths::discover().unwrap();
        assert!(!paths.is_test());
        assert!(service::user_manager_available(&paths));
        assert!(!paths.soloist_service.exists());
        assert!(!paths.install_state.exists());
        paths.ensure_management_dirs().unwrap();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        let original = "[Unit]\nDescription=Original Soloist\n[Service]\nExecStart=/usr/bin/sleep 3600\n[Install]\nWantedBy=default.target\n";
        fs::write(&paths.soloist_service, original).unwrap();
        let exercise = (|| -> Result<()> {
            service::daemon_reload(&paths)?;
            service::restore_unit_state(&paths, "soloist.service", true, true)?;
            let snapshot = service::SoloistUnitSnapshot::capture(&paths)?;
            snapshot.persist(&paths)?;
            let managed_unit = "[Unit]\nDescription=Managed Soloist\n[Service]\nExecStart=/usr/bin/sleep 3600\n[Install]\nWantedBy=default.target\n";
            fs::write(&paths.soloist_service, managed_unit)?;
            service::daemon_reload(&paths)?;
            service::restore_unit_state(&paths, "soloist.service", true, true)?;
            let managed_dir = paths.managed_soloist.join("current-build");
            fs::create_dir_all(&managed_dir)?;
            fs::write(managed_dir.join("soloist"), b"owned")?;
            let state = installed_state(
                &paths,
                SoloistState {
                    ownership: SoloistOwnership::Installer,
                    executable: Some(managed_dir.join("soloist")),
                    service_owned: true,
                    data_owned: false,
                    websocket_url: Some("ws://127.0.0.1:9090/".into()),
                    version: Some("build".into()),
                },
            );
            atomic_link(&state.active_release, &paths.current)?;
            state.save(&paths)?;
            uninstall(
                &paths,
                UninstallOptions {
                    soloist: Some(SoloistRemoval::Restore),
                    data: Some(DisplayDataRemoval::Keep),
                    yes: true,
                },
                true,
            )?;
            assert_eq!(fs::read_to_string(&paths.soloist_service)?, original);
            assert!(service::active(&paths, "soloist.service"));
            assert!(!paths.managed_soloist.exists());
            Ok(())
        })();
        let _ = service::disable_owned(&paths, "soloist.service");
        fs::remove_file(&paths.soloist_service).ok();
        service::daemon_reload(&paths).ok();
        exercise.unwrap();
    }

    #[test]
    #[serial]
    fn uninstall_display_preserves_installer_owned_soloist_and_settings() {
        let (_directory, paths) = test_paths();
        fs::create_dir_all(&paths.managed_soloist).unwrap();
        fs::write(paths.managed_soloist.join("soloist"), b"owned").unwrap();
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(paths.config.join("config.json"), b"{}").unwrap();
        let soloist_state = SoloistState {
            ownership: SoloistOwnership::Installer,
            executable: Some(paths.managed_soloist.join("soloist")),
            service_owned: true,
            data_owned: true,
            websocket_url: Some("ws://127.0.0.1:9090".into()),
            version: Some("test".into()),
        };
        installed_state(&paths, soloist_state).save(&paths).unwrap();
        atomic_link(&paths.releases.join("1.0.0-test"), &paths.current).unwrap();
        uninstall(
            &paths,
            UninstallOptions {
                soloist: Some(SoloistRemoval::Keep),
                data: Some(DisplayDataRemoval::Keep),
                yes: true,
            },
            false,
        )
        .unwrap();
        assert!(paths.managed_soloist.join("soloist").exists());
        assert!(paths.config.join("config.json").exists());
        assert!(!paths.releases.exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn uninstall_all_restores_existing_service_credentials_and_session() {
        let (_directory, paths) = test_paths();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        fs::create_dir_all(paths.soloist_env.parent().unwrap()).unwrap();
        fs::create_dir_all(&paths.soloist_data).unwrap();
        fs::write(paths.soloist_data.join("session"), b"existing session").unwrap();
        let original_service = b"[Service]\nExecStart=/opt/soloist\n";
        let original_credentials = b"SOLOIST_API_KEY=existing\n";
        fs::write(&paths.soloist_service, original_service).unwrap();
        fs::write(&paths.soloist_env, original_credentials).unwrap();
        let prior = service::SoloistUnitSnapshot::capture(&paths).unwrap();
        prior.persist(&paths).unwrap();

        fs::write(
            &paths.soloist_service,
            b"[Service]\nExecStart=/managed/soloist\n",
        )
        .unwrap();
        fs::write(&paths.soloist_env, b"SOLOIST_API_KEY=managed\n").unwrap();
        fs::create_dir_all(&paths.managed_soloist).unwrap();
        let managed = paths.managed_soloist.join("soloist");
        fs::write(&managed, b"managed").unwrap();
        installed_state(
            &paths,
            SoloistState {
                ownership: SoloistOwnership::Installer,
                executable: Some(managed),
                service_owned: true,
                data_owned: false,
                websocket_url: Some("ws://127.0.0.1:9090".into()),
                version: Some("test".into()),
            },
        )
        .save(&paths)
        .unwrap();
        atomic_link(&paths.releases.join("1.0.0-test"), &paths.current).unwrap();

        uninstall(
            &paths,
            UninstallOptions {
                soloist: Some(SoloistRemoval::Restore),
                data: Some(DisplayDataRemoval::Remove),
                yes: true,
            },
            true,
        )
        .unwrap();
        assert_eq!(fs::read(&paths.soloist_service).unwrap(), original_service);
        assert_eq!(fs::read(&paths.soloist_env).unwrap(), original_credentials);
        assert_eq!(
            fs::read(paths.soloist_data.join("session")).unwrap(),
            b"existing session"
        );
        assert!(!paths.managed_soloist.exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn uninstall_all_never_removes_independent_soloist() {
        let (directory, paths) = test_paths();
        let independent = directory.path().join("independent-soloist");
        fs::write(&independent, b"independent").unwrap();
        let soloist_state = SoloistState {
            ownership: SoloistOwnership::Independent,
            executable: Some(independent.clone()),
            service_owned: false,
            data_owned: false,
            websocket_url: Some("ws://127.0.0.1:9090".into()),
            version: None,
        };
        installed_state(&paths, soloist_state).save(&paths).unwrap();
        atomic_link(&paths.releases.join("1.0.0-test"), &paths.current).unwrap();
        uninstall(
            &paths,
            UninstallOptions {
                soloist: None,
                data: Some(DisplayDataRemoval::Remove),
                yes: true,
            },
            false,
        )
        .unwrap();
        assert_eq!(fs::read(independent).unwrap(), b"independent");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn uninstall_data_removes_display_data_but_preserves_soloist() {
        let (directory, paths) = test_paths();
        let independent = directory.path().join("independent-soloist");
        fs::write(&independent, b"independent").unwrap();
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(paths.config.join("config.json"), b"{}").unwrap();
        fs::write(&paths.soloist_env, b"SOLOIST_API_KEY=preserved").unwrap();
        let soloist_state = SoloistState {
            ownership: SoloistOwnership::Independent,
            executable: Some(independent.clone()),
            service_owned: false,
            data_owned: false,
            websocket_url: Some("ws://127.0.0.1:9090".into()),
            version: None,
        };
        installed_state(&paths, soloist_state).save(&paths).unwrap();
        atomic_link(&paths.releases.join("1.0.0-test"), &paths.current).unwrap();
        uninstall(
            &paths,
            UninstallOptions {
                soloist: None,
                data: Some(DisplayDataRemoval::Remove),
                yes: true,
            },
            false,
        )
        .unwrap();
        assert!(!paths.config.join("config.json").exists());
        assert!(paths.soloist_env.exists());
        assert!(independent.exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
