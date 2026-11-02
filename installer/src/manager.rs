use crate::{
    dependencies, migrations,
    paths::{path_string, Paths},
    platform::{Architecture, Platform},
    process,
    release::{Artifact, ReleaseClient, ReleaseManifest},
    service, soloist,
    state::{now, InstallState, SoloistOwnership, TransactionJournal, TransactionStatus},
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

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum RemoveMode {
    Display,
    Data,
    All,
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

fn install_visible_command(paths: &Paths) -> Result<()> {
    fs::create_dir_all(&paths.bin_dir)?;
    atomic_link(
        &paths.current.join("bin/soloist-display"),
        &paths.visible_command,
    )
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
        if soloist::healthy(soloist_state)
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
    if !soloist::healthy(soloist_state) {
        bail!("Soloist health check failed: WebSocket 127.0.0.1:9090 is unavailable");
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
        let sink = Command::new("pactl").arg("get-default-sink").output()
            .ok().map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_else(|| "<pactl unavailable>".to_owned());
        bail!(
            "audio health check failed after 6s: no monitor found for default sink {sink:?}; inspect `pactl list sinks` and `pactl list short sources`"
        );
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
            TransactionStatus::Committed
                | TransactionStatus::RolledBack
                | TransactionStatus::Failed
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
        }
        transaction.status = TransactionStatus::RolledBack;
        transaction.persist(&path)?;
    }
    Ok(())
}

fn install_step(quiet: bool, step: usize, label: &str) {
    if quiet { return; }
    let color = std::io::stderr().is_terminal()
        && std::env::var_os("NO_COLOR").is_none();
    let width = 24usize;
    let filled = step * width / 4;
    let bar = format!("{}{}", "━".repeat(filled), "─".repeat(width - filled));
    if color {
        eprintln!("  \x1b[36m{bar}\x1b[0m  {step}/4  {label}");
    } else {
        eprintln!("  [{bar}]  {step}/4  {label}");
    }
}

pub fn install(
    paths: &Paths,
    platform: &Platform,
    yes: bool,
    quiet: bool,
    name: Option<String>,
    api_key: Option<String>,
) -> Result<()> {
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
    if !quiet {
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
            let status = Command::new("systemctl")
                .args(["--user", "stop", "soloist-display.service"])
                .status()
                .context("stop orphaned Soloist Display service")?;
            if !status.success() {
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
    if discovered_soloist.ownership == SoloistOwnership::None && !port_available(9090) {
        bail!("port 9090 is occupied by a service that is not a working Soloist WebSocket; stop its owner and retry");
    }
    let (mut transaction, journal_path) = TransactionJournal::begin(paths, "install")?;
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
        install_step(false, 1, "Checking dependencies");
    }
    let packages = match dependencies::install(platform, quiet) {
        Ok(packages) => packages,
        Err(error) => {
            transaction.fail(&journal_path, error.to_string()).ok();
            return Err(error);
        }
    };
    if !quiet {
        install_step(false, 2, "Downloading & verifying");
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
        if manifest.version < semver::Version::parse(&installed.version)? {
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
        let mut soloist_state = discovered_soloist.clone();
        if soloist_state.ownership == SoloistOwnership::None {
            soloist_state = soloist::install(
                paths,
                &client,
                manifest.soloist(platform)?,
                &manifest.version.to_string(),
                name,
                api_key,
            )?;
        } else {
            soloist::start_existing(paths, &soloist_state)?;
        }
        soloist::ensure_cli_discovery(paths, &soloist_state)?;
        transaction.set_status(&journal_path, TransactionStatus::Services)?;
        if !quiet {
            install_step(false, 3, "Starting services");
        }
        service::install_display(paths)?;
        let desktop = desktop_files(paths)?;
        transaction.set_status(&journal_path, TransactionStatus::HealthCheck)?;
        if !quiet {
            install_step(false, 4, "Checking health");
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
            target: platform.target.clone(),
            active_release: release,
            previous_release: previous_state.as_ref().map(|s| s.active_release.clone()),
            installed_at: now(),
            soloist: soloist_state,
            managed_paths: managed,
            dependency_packages: packages,
        }
        .save(paths)?;
        transaction.set_status(&journal_path, TransactionStatus::Committed)?;
        Ok::<_, anyhow::Error>(())
    })();
    if let Err(error) = result {
        // On a failed first install, do not leave a running bridge behind
        // without committed installation metadata.
        if previous_state.is_none() && !paths.is_test() {
            let _ = Command::new("systemctl")
                .args(["--user", "stop", "soloist-display.service"])
                .status();
        }
        if let Some(previous) = transaction.previous_current.as_ref() {
            atomic_link(previous, &paths.current).ok();
        }
        transaction.fail(&journal_path, error.to_string()).ok();
        return Err(error.context("installation failed; the previous active release was restored"));
    }
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

pub fn update(paths: &Paths, platform: &Platform, quiet: bool) -> Result<()> {
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    let client = ReleaseClient::new()?;
    let (manifest, _) = client.latest()?;
    let installed_version =
        semver::Version::parse(&state.version).context("installed version metadata is invalid")?;
    if manifest.version < installed_version {
        bail!(
            "refusing to downgrade from {} to {}",
            state.version,
            manifest.version
        );
    }
    if manifest.version.to_string() == state.version {
        if !quiet {
            println!("Soloist Display {} is already current.", state.version);
        }
        return Ok(());
    }
    let (mut tx, tx_path) = TransactionJournal::begin(paths, "update")?;
    tx.previous_current = Some(state.active_release.clone());
    tx.persist(&tx_path)?;
    let result = (|| {
        let release = install_artifacts(paths, platform, &client, &manifest, &mut tx, &tx_path)?;
        tx.set_status(&tx_path, TransactionStatus::Activating)?;
        atomic_link(&release, &paths.current)?;
        migrations::migrate(paths)?;
        let mut next_soloist = state.soloist.clone();
        if state.soloist.ownership == SoloistOwnership::Installer {
            next_soloist = soloist::install(
                paths,
                &client,
                manifest.soloist(platform)?,
                &manifest.version.to_string(),
                None,
                None,
            )?;
        }
        service::install_display(paths)?;
        wait_for_install_health(paths, &next_soloist)?;
        let mut next = state.clone();
        next.previous_release = Some(state.active_release.clone());
        next.active_release = release;
        next.version = manifest.version.to_string();
        next.target = platform.target.clone();
        next.soloist = next_soloist;
        next.save(paths)?;
        tx.set_status(&tx_path, TransactionStatus::Committed)
    })();
    if let Err(error) = result {
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
        return Err(error.context("update failed; restored the previous release"));
    }
    if !quiet {
        println!("Updated Soloist Display to {}.", manifest.version);
    }
    Ok(())
}

pub fn repair(paths: &Paths, platform: &Platform, quiet: bool) -> Result<()> {
    let mut state = InstallState::load(paths)?
        .ok_or_else(|| anyhow!("Soloist Display is not installed; run the installer first"))?;
    migrations::migrate(paths)?;
    let client = ReleaseClient::new()?;
    let (manifest, _) = client.latest()?;
    if manifest.version.to_string() != state.version {
        return update(paths, platform, quiet);
    }
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
            return Err(error.context("repair failed; restored the previous files"));
        }
        if had_original {
            fs::remove_dir_all(backup).ok();
        }
    }
    atomic_link(&state.active_release, &paths.current)?;
    install_visible_command(paths)?;
    desktop_files(paths)?;
    service::install_display(paths)?;
    if state.soloist.ownership == SoloistOwnership::Installer {
        let executable = state
            .soloist
            .executable
            .as_ref()
            .ok_or_else(|| anyhow!("installer-owned Soloist executable is missing"))?;
        service::install_soloist(paths, executable)?;
    } else {
        soloist::start_existing(paths, &state.soloist)?;
    }
    wait_for_install_health(paths, &state.soloist)?;
    if !quiet {
        println!("Repair complete.");
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct UpdateCache {
    checked_at: u64,
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

pub fn check_update_briefly(paths: &Paths, installed: &str) -> Option<String> {
    let cache_path = paths.cache.join("update.json");
    if let Ok(bytes) = fs::read(&cache_path) {
        if let Ok(cache) = serde_json::from_slice::<UpdateCache>(&bytes) {
            if now().saturating_sub(cache.checked_at) < 86_400 {
                return is_newer(&cache.latest, installed).then_some(cache.latest);
            }
        }
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let result = ReleaseClient::new()
            .and_then(|client| client.latest())
            .map(|(manifest, _)| manifest.version.to_string());
        let _ = sender.send(result);
    });
    if let Ok(Ok(latest)) = receiver.recv_timeout(Duration::from_millis(750)) {
        fs::create_dir_all(&paths.cache).ok();
        crate::state::atomic_json(
            &cache_path,
            &UpdateCache {
                checked_at: now(),
                latest: latest.clone(),
            },
            Some(0o600),
        )
        .ok();
        if is_newer(&latest, installed) {
            return Some(latest);
        }
    }
    None
}

pub fn launch(paths: &Paths, settings: bool) -> Result<()> {
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    service::start_display(paths)?;
    wait_http(paths)?;
    let mut browser_update_notice = None;
    if let Some(latest) = check_update_briefly(paths, &state.version) {
        eprintln!("Soloist Display {latest} is available.");
        if io::stdin().is_terminal() {
            eprint!("Update now? [y/N] ");
            io::stderr().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                update(paths, &Platform::detect()?, false)?;
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
    process::run(
        Command::new("xdg-open").arg(&url),
        "open Soloist Display in the default browser",
        false,
    )
}

pub fn doctor(paths: &Paths) -> Result<bool> {
    let mut ok = true;
    let state = InstallState::load(paths)?;
    macro_rules! check {
        ($label:expr, $value:expr, $help:expr) => {{
            let value = $value;
            println!("{:<22} {}", $label, if value { "ok" } else { "FAILED" });
            if !value {
                eprintln!("  {}", $help);
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
        check!(
            "Soloist WebSocket",
            soloist::healthy(&state.soloist),
            "Start Soloist and verify loopback WebSocket port 9090."
        );
        if state.soloist.ownership == SoloistOwnership::Installer {
            let secure = fs::metadata(&paths.soloist_env)
                .map(|metadata| metadata.permissions().mode() & 0o077 == 0)
                .unwrap_or(false);
            check!(
                "Soloist credentials",
                secure,
                "Run `chmod 600 ~/.config/soloist-display/soloist.env`, then repair."
            );
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

fn choose_remove_mode() -> Result<RemoveMode> {
    if !io::stdin().is_terminal() {
        bail!("uninstall requires --mode in non-interactive mode");
    }
    eprintln!("1  Display only (keep data and Soloist)\n2  Display and its data\n3  Display, data, and installer-owned Soloist");
    eprint!("Choose [1-3]: ");
    io::stderr().flush()?;
    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    match choice.trim() {
        "1" => Ok(RemoveMode::Display),
        "2" => Ok(RemoveMode::Data),
        "3" => Ok(RemoveMode::All),
        _ => bail!("invalid uninstall mode"),
    }
}

fn safe_remove(path: &Path, allowed: &[&Path]) -> Result<()> {
    if !allowed.iter().any(|root| path.starts_with(root)) {
        bail!("refusing to remove unmanaged path {}", path.display());
    }
    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn uninstall(paths: &Paths, requested: Option<RemoveMode>) -> Result<()> {
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    let mode = requested.map(Ok).unwrap_or_else(choose_remove_mode)?;
    service::disable_owned(paths, "soloist-display.service")?;
    for path in [
        &paths.display_service,
        &paths.visible_command,
        &paths.applications.join("soloist-display.desktop"),
        &paths.applications.join("soloist-display-settings.desktop"),
    ] {
        if path.exists() || path.is_symlink() {
            safe_remove(
                path,
                &[&paths.config_home, &paths.data_home, &paths.bin_dir],
            )?;
        }
    }
    if matches!(mode, RemoveMode::All) && state.soloist.ownership == SoloistOwnership::Installer {
        service::disable_owned(paths, "soloist.service")?;
        for path in [
            &paths.soloist_service,
            &paths.soloist_env,
            &paths.managed_soloist,
            &paths.soloist_data,
            &paths.soloist_cache,
        ] {
            if path.exists() || path.is_symlink() {
                safe_remove(
                    path,
                    &[&paths.config_home, &paths.data_home, &paths.cache_home],
                )?;
            }
        }
    }
    if matches!(mode, RemoveMode::Data | RemoveMode::All) {
        for path in [&paths.config.join("config.json"), &paths.cache] {
            if path.exists() {
                safe_remove(path, &[&paths.config_home, &paths.cache_home])?;
            }
        }
        if paths.config.exists() && fs::read_dir(&paths.config)?.next().is_none() {
            fs::remove_dir(&paths.config)?;
        }
    }
    for path in [&paths.current, &paths.releases] {
        if path.exists() || path.is_symlink() {
            safe_remove(path, &[&paths.data_home])?;
        }
    }
    if paths.root.exists() && fs::read_dir(&paths.root)?.next().is_none() {
        fs::remove_dir(&paths.root)?;
    }
    if paths.state.exists() {
        safe_remove(&paths.state, &[&paths.state_home])?;
    }
    service::daemon_reload(paths)?;
    println!(
        "Soloist Display removed{}.",
        if matches!(mode, RemoveMode::Display) {
            "; settings and Soloist were preserved"
        } else {
            ""
        }
    );
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
    fn removal_rejects_outside_managed_roots() {
        assert!(safe_remove(Path::new("/etc/passwd"), &[Path::new("/tmp/example")]).is_err());
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
        uninstall(&paths, Some(RemoveMode::Display)).unwrap();
        assert!(paths.managed_soloist.join("soloist").exists());
        assert!(paths.config.join("config.json").exists());
        assert!(!paths.releases.exists());
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
        uninstall(&paths, Some(RemoveMode::All)).unwrap();
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
        uninstall(&paths, Some(RemoveMode::Data)).unwrap();
        assert!(!paths.config.join("config.json").exists());
        assert!(paths.soloist_env.exists());
        assert!(independent.exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
