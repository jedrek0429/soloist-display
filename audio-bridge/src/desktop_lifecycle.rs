//! External-only desktop installation lifecycle.
//! Uses the same signed release manifest and verifier as the Linux manager.
//! Never installs, starts, kills, renews or alters Spotify Soloist.
use crate::release::{sha256_file, ReleaseChannel, ReleaseClient};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

#[cfg(target_os = "windows")]
const TARGET: &str = "x86_64-pc-windows-msvc";
#[cfg(target_os = "macos")]
const TARGET: &str = "aarch64-apple-darwin";
#[cfg(target_os = "windows")]
const EXECUTABLE: &str = "soloist-display.exe";
#[cfg(target_os = "macos")]
const EXECUTABLE: &str = "soloist-display";
const MANAGED_MARKER: &str = "Managed by Soloist Display; do not edit";
const STATE_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Installed {
    schema: u32,
    version: String,
    channel: ReleaseChannel,
    suppress_updates: bool,
    #[serde(default)]
    previous_version: Option<String>,
}

#[derive(Clone, Debug)]
struct Paths {
    releases: PathBuf,
    shim: PathBuf,
    state: PathBuf,
}

fn settings_root() -> PathBuf {
    if let Some(custom) = env::var_os("SOLOIST_DISPLAY_CONFIG_DIR") {
        return PathBuf::from(custom);
    }
    #[cfg(target_os = "windows")]
    {
        return env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("soloist-display");
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/soloist-display")
    }
}

impl Paths {
    fn discover() -> Result<Self> {
        let root = if let Some(custom) = env::var_os("SOLOIST_DISPLAY_INSTALL_ROOT") {
            PathBuf::from(custom)
        } else {
            #[cfg(target_os = "windows")]
            {
                env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .ok_or("LOCALAPPDATA is not set")?
                    .join("soloist-display")
            }
            #[cfg(target_os = "macos")]
            {
                PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?)
                    .join("Library/Application Support/soloist-display/app")
            }
        };
        if !root.is_absolute() {
            return Err("installation root must be an absolute path".into());
        }
        let bin = if let Some(custom) = env::var_os("SOLOIST_DISPLAY_BIN_DIR") {
            PathBuf::from(custom)
        } else {
            #[cfg(target_os = "windows")]
            {
                root.join("bin")
            }
            #[cfg(target_os = "macos")]
            {
                PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?).join(".local/bin")
            }
        };
        if !bin.is_absolute() {
            return Err("CLI bin directory must be an absolute path".into());
        }
        #[cfg(target_os = "windows")]
        let shim = bin.join("soloist-display.cmd");
        #[cfg(target_os = "macos")]
        let shim = bin.join("soloist-display");
        let paths = Self {
            releases: root.join("releases"),
            state: root.join("desktop-install.json"),
            shim,
        };
        recover_uninstall(&paths)?;
        Ok(paths)
    }

    fn executable(&self, version: &str) -> PathBuf {
        self.releases.join(version).join(EXECUTABLE)
    }

    fn load(&self) -> Result<Option<Installed>> {
        if !self.state.exists() {
            return Ok(None);
        }
        let state: Installed = serde_json::from_slice(&fs::read(&self.state)?)?;
        if state.schema != STATE_SCHEMA {
            return Err("unsupported desktop installation metadata version".into());
        }
        Version::parse(&state.version)?;
        Ok(Some(state))
    }

    fn write_atomic(&self, path: &Path, data: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("pending-{}", std::process::id()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(data)?;
            file.sync_all()?;
            // Windows rename cannot overwrite an existing file. Keep a backup until
            // the replacement succeeds, restoring the previous state on failure.
            let backup = path.with_extension(format!("backup-{}", std::process::id()));
            let had_old = path.exists();
            if had_old {
                fs::rename(path, &backup)?;
            }
            if let Err(error) = fs::rename(&temp, path) {
                if had_old {
                    let _ = fs::rename(&backup, path);
                }
                return Err(error.into());
            }
            if had_old {
                let _ = fs::remove_file(&backup);
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn shim_content(&self, version: &str) -> Vec<u8> {
        let exe = self.executable(version);
        #[cfg(target_os = "windows")]
        {
            let path = exe.to_string_lossy().replace('%', "%%");
            format!("@echo off\r\nrem {MANAGED_MARKER}\r\n\"{path}\" %*\r\n").into_bytes()
        }
        #[cfg(target_os = "macos")]
        {
            let path = exe.to_string_lossy().replace('\'', "'\\''");
            format!("#!/bin/sh\n# {MANAGED_MARKER}\nexec '{path}' \"$@\"\n").into_bytes()
        }
    }

    fn check_shim_ownership(&self) -> Result<()> {
        if self.shim.exists()
            && !String::from_utf8_lossy(&fs::read(&self.shim)?).contains(MANAGED_MARKER)
        {
            return Err(format!(
                "refusing to replace an unowned command at {}",
                self.shim.display()
            )
            .into());
        }
        Ok(())
    }

    fn point_to(&self, version: &str) -> Result<()> {
        self.check_shim_ownership()?;
        self.write_atomic(&self.shim, &self.shim_content(version))?;
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.shim, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn save(&self, state: &Installed) -> Result<()> {
        self.write_atomic(&self.state, &serde_json::to_vec_pretty(state)?)
    }
}

fn parse_version(value: &str) -> Result<Version> {
    let v = Version::parse(value)?;
    if v.to_string() != value || !v.build.is_empty() {
        return Err("release version must be canonical SemVer".into());
    }
    Ok(v)
}

fn verify_executable(exe: &Path, version: &str) -> Result<()> {
    let output = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err("downloaded executable failed its version smoke check".into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    if !stdout.trim().ends_with(version) {
        return Err(format!("binary version mismatch: {}", stdout.trim()).into());
    }
    Ok(())
}

fn download_version(
    paths: &Paths,
    client: &ReleaseClient,
    version: &Version,
    artifact: &crate::release::Artifact,
) -> Result<()> {
    let directory = paths.releases.join(version.to_string());
    fs::create_dir_all(&directory)?;
    let executable = directory.join(EXECUTABLE);
    if executable.exists() && sha256_file(&executable)?.eq_ignore_ascii_case(&artifact.sha256) {
        if verify_executable(&executable, &version.to_string()).is_ok() {
            return Ok(());
        }
    }
    let staged = directory.join(format!(".download-{}-{EXECUTABLE}", std::process::id()));
    let _ = fs::remove_file(&staged);
    let result = (|| -> Result<()> {
        client.download(artifact, &staged)?;
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o700))?;
        }
        verify_executable(&staged, &version.to_string())?;
        if executable.exists() {
            // Never destroy the only copy of an installed release before replacement.
            // Windows may keep the running EXE locked; in that case retain it and fail.
            let backup = directory.join(format!(".previous-{EXECUTABLE}"));
            let _ = fs::remove_file(&backup);
            fs::rename(&executable, &backup)?;
            if let Err(error) = fs::rename(&staged, &executable) {
                let _ = fs::rename(&backup, &executable);
                return Err(error.into());
            }
            let _ = fs::remove_file(&backup);
        } else {
            fs::rename(&staged, &executable)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

fn select(
    client: &ReleaseClient,
    channel: ReleaseChannel,
    version: Option<&str>,
) -> Result<(Version, crate::release::Artifact)> {
    let (manifest, _) = client.select(Some(channel), version)?;
    let artifact = manifest.artifact_target("desktop", TARGET)?.clone();
    if manifest.version.to_string() != parse_version(&manifest.version.to_string())?.to_string() {
        return Err("non-canonical release version".into());
    }
    Ok((manifest.version, artifact))
}

fn activate(
    paths: &Paths,
    current: Option<Installed>,
    version: Version,
    channel: ReleaseChannel,
) -> Result<()> {
    let version_string = version.to_string();
    let state = Installed {
        schema: STATE_SCHEMA,
        version: version_string.clone(),
        channel,
        suppress_updates: current.as_ref().is_some_and(|s| s.suppress_updates),
        previous_version: current
            .as_ref()
            .map(|s| s.version.clone())
            .filter(|old| old != &version_string),
    };
    // Repoint only after the new signed binary has passed --version.
    paths.point_to(&version_string)?;
    if let Err(error) = paths.save(&state) {
        if let Some(old) = current {
            let _ = paths.point_to(&old.version);
        } else {
            let _ = fs::remove_file(&paths.shim);
        }
        return Err(error);
    }
    println!("Soloist Display {version_string} installed (External Soloist)");
    println!("Command: {}", paths.shim.display());
    println!("Soloist itself was not modified.");
    Ok(())
}

fn configured_channel() -> Result<Option<ReleaseChannel>> {
    match env::var("SOLOIST_DISPLAY_CHANNEL") {
        Ok(value) => Ok(Some(match value.as_str() {
            "stable" => ReleaseChannel::Stable,
            "alpha" => ReleaseChannel::Alpha,
            "beta" => ReleaseChannel::Beta,
            "prerelease" => ReleaseChannel::Prerelease,
            _ => {
                return Err(
                    "SOLOIST_DISPLAY_CHANNEL must be stable, alpha, beta or prerelease".into(),
                );
            }
        })),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn install() -> Result<()> {
    let paths = Paths::discover()?;
    paths.check_shim_ownership()?;
    let current = paths.load()?;
    let client = ReleaseClient::new()?;
    let configured = configured_channel()?;
    let channel = configured
        .or_else(|| current.as_ref().map(|state| state.channel))
        .unwrap_or(ReleaseChannel::Prerelease);
    let (version, artifact) = select(&client, channel, None)?;
    let effective_channel = if current.is_none() && configured.is_none() {
        ReleaseChannel::for_version(&version)
    } else {
        channel
    };
    download_version(&paths, &client, &version, &artifact)?;
    activate(&paths, current, version, effective_channel)
}

pub fn update(
    channel: Option<ReleaseChannel>,
    requested: Option<&str>,
    allow_downgrade: bool,
) -> Result<()> {
    let paths = Paths::discover()?;
    let current = paths
        .load()?
        .ok_or("not installed; run 'soloist-display install' first")?;
    let effective_channel = channel.unwrap_or(current.channel);
    let client = ReleaseClient::new()?;
    let (version, artifact) = select(&client, effective_channel, requested)?;
    let previous = parse_version(&current.version)?;
    if version < previous && !allow_downgrade {
        return Err(
            format!("downgrade from {previous} to {version} requires --allow-downgrade").into(),
        );
    }
    if version == previous {
        let exe = paths.executable(&current.version);
        if exe.exists()
            && sha256_file(&exe)?.eq_ignore_ascii_case(&artifact.sha256)
            && verify_executable(&exe, &current.version).is_ok()
        {
            paths.check_shim_ownership()?;
            if paths.shim.exists() && fs::read(&paths.shim)? == paths.shim_content(&current.version)
            {
                println!("Soloist Display {version} is already installed.");
                return Ok(());
            }
        }
        return repair();
    }
    download_version(&paths, &client, &version, &artifact)?;
    activate(&paths, Some(current), version, effective_channel)
}

pub fn check_updates() -> Result<()> {
    let paths = Paths::discover()?;
    let current = paths.load()?.ok_or("not installed")?;
    let client = ReleaseClient::new()?;
    let (version, _) = select(&client, current.channel, None)?;
    println!("Installed: {}", current.version);
    println!("Available: {version}");
    println!("Channel: {:?}", current.channel);
    println!(
        "Notifications: {}",
        if current.suppress_updates {
            "off"
        } else {
            "on"
        }
    );
    Ok(())
}

/// Checks for updates without blocking the display startup.
pub fn maybe_notify() {
    let result = (|| -> Result<()> {
        let paths = Paths::discover()?;
        let Some(current) = paths.load()? else {
            return Ok(());
        };
        if current.suppress_updates {
            return Ok(());
        }
        let client = ReleaseClient::new()?;
        let (latest, _) = select(&client, current.channel, None)?;
        if latest > parse_version(&current.version)? {
            eprintln!("Soloist Display {latest} is available; run 'soloist-display update'.");
        }
        Ok(())
    })();
    if let Err(error) = result {
        if crate::cli::verbose() {
            eprintln!("Update check unavailable: {error}");
        }
    }
}

pub fn notifications(enabled: bool) -> Result<()> {
    let paths = Paths::discover()?;
    let mut current = paths.load()?.ok_or("not installed")?;
    current.suppress_updates = !enabled;
    paths.save(&current)?;
    println!(
        "Update notifications: {}",
        if enabled { "on" } else { "off" }
    );
    Ok(())
}

pub fn repair() -> Result<()> {
    let paths = Paths::discover()?;
    let current = paths.load()?.ok_or("not installed")?;
    let client = ReleaseClient::new()?;
    let (version, artifact) = select(&client, current.channel, Some(&current.version))?;
    download_version(&paths, &client, &version, &artifact)
        .map_err(|error| format!("repair failed ({error}); close Soloist Display and retry"))?;
    paths.point_to(&current.version)?;
    println!("Repaired Soloist Display {}", current.version);
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct UninstallIntent {
    remove_data: bool,
}

fn uninstall_paths(paths: &Paths) -> (PathBuf, PathBuf, PathBuf) {
    (
        paths.state.with_file_name("desktop-uninstall.json"),
        paths.releases.with_extension("removing"),
        paths.shim.with_extension("removing"),
    )
}

#[cfg(target_os = "windows")]
fn remove_bin_from_path(paths: &Paths) -> Result<()> {
    let bin_dir = paths.shim.parent().ok_or("invalid command shim path")?;
    let script = r#"
        $target = $env:SOLOIST_DISPLAY_REMOVE_BIN
        $existing = [Environment]::GetEnvironmentVariable('Path','User')
        $parts = @($existing -split ';' | Where-Object {
            $_ -and $_.TrimEnd('\\') -ine $target.TrimEnd('\\')
        })
        [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
    "#;
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SOLOIST_DISPLAY_REMOVE_BIN", bin_dir)
        .status()?;
    if !status.success() {
        return Err("unable to remove CLI bin directory from user PATH".into());
    }
    Ok(())
}

fn remove_user_data() -> Result<()> {
    let config = settings_root();
    for name in ["config.json", "audio-device.id"] {
        let file = config.join(name);
        if file.exists() {
            fs::remove_file(file)?;
        }
    }
    Ok(())
}

// The installation state is the commit marker: present -> rollback,
// absent -> finish deletion. The intent survives process termination.
fn recover_uninstall(paths: &Paths) -> Result<()> {
    let (journal, releases, shim) = uninstall_paths(paths);
    if !journal.exists() {
        return Ok(());
    }
    let intent: UninstallIntent = serde_json::from_slice(&fs::read(&journal)?)?;
    if paths.state.exists() {
        if releases.exists() {
            if paths.releases.exists() {
                return Err("uninstall recovery conflict: releases already exist".into());
            }
            fs::rename(&releases, &paths.releases)?;
        }
        if shim.exists() {
            if paths.shim.exists() {
                return Err("uninstall recovery conflict: launcher already exists".into());
            }
            fs::rename(&shim, &paths.shim)?;
        }
    } else {
        if releases.exists() {
            fs::remove_dir_all(&releases)?;
        }
        if shim.exists() {
            fs::remove_file(&shim)?;
        }
        if intent.remove_data {
            remove_user_data()?;
        }
        #[cfg(target_os = "windows")]
        remove_bin_from_path(paths)?;
    }
    fs::remove_file(journal)?;
    Ok(())
}

fn remove_installation(
    paths: &Paths,
    remove_data: bool,
    staged_check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let (journal, releases, shim) = uninstall_paths(paths);
    if journal.exists() || releases.exists() || shim.exists() {
        return Err("unfinished uninstall staging exists".into());
    }
    paths.write_atomic(
        &journal,
        &serde_json::to_vec(&UninstallIntent { remove_data })?,
    )?;
    let result = (|| -> Result<()> {
        if paths.releases.exists() {
            fs::rename(&paths.releases, &releases)?;
        }
        if paths.shim.exists() {
            fs::rename(&paths.shim, &shim)?;
        }
        staged_check()?;
        fs::remove_file(&paths.state)?;
        Ok(())
    })();
    if let Err(error) = result {
        recover_uninstall(paths)?;
        return Err(error);
    }
    // Cleanup errors leave the journal for the next invocation.
    recover_uninstall(paths)
}

pub fn uninstall(remove_data: bool) -> Result<()> {
    let paths = Paths::discover()?;
    paths.load()?.ok_or("not installed")?;
    paths.check_shim_ownership()?;
    remove_installation(&paths, remove_data, || Ok(()))?;
    println!("Removed Soloist Display CLI. External Soloist was not changed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_uninstall_restores_installation() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            releases: dir.path().join("releases"),
            shim: dir.path().join("bin").join("soloist-display"),
            state: dir.path().join("desktop-install.json"),
        };
        fs::create_dir_all(paths.executable("1.2.3").parent().unwrap()).unwrap();
        fs::write(paths.executable("1.2.3"), b"release").unwrap();
        paths.point_to("1.2.3").unwrap();
        let state = Installed {
            schema: STATE_SCHEMA,
            version: "1.2.3".into(),
            channel: ReleaseChannel::Prerelease,
            suppress_updates: false,
            previous_version: None,
        };
        paths.save(&state).unwrap();
        assert!(remove_installation(&paths, false, || Err("injected failure".into())).is_err());
        assert_eq!(fs::read(paths.executable("1.2.3")).unwrap(), b"release");
        assert!(paths.shim.exists());
        assert!(paths.load().unwrap().is_some());
    }
    #[test]
    fn interrupted_uninstall_recovers_on_both_sides_of_commit() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            releases: dir.path().join("releases"),
            shim: dir.path().join("bin").join("soloist-display"),
            state: dir.path().join("desktop-install.json"),
        };
        let exe = paths.executable("1.2.3");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"release").unwrap();
        paths.point_to("1.2.3").unwrap();
        paths
            .save(&Installed {
                schema: STATE_SCHEMA,
                version: "1.2.3".into(),
                channel: ReleaseChannel::Prerelease,
                suppress_updates: false,
                previous_version: None,
            })
            .unwrap();
        let (journal, staged_releases, staged_shim) = uninstall_paths(&paths);
        paths
            .write_atomic(&journal, br#"{"remove_data":false}"#)
            .unwrap();
        fs::rename(&paths.releases, &staged_releases).unwrap();
        recover_uninstall(&paths).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"release");
        assert!(paths.shim.exists());
        assert!(!journal.exists());

        paths
            .write_atomic(&journal, br#"{"remove_data":false}"#)
            .unwrap();
        fs::rename(&paths.releases, &staged_releases).unwrap();
        fs::rename(&paths.shim, &staged_shim).unwrap();
        fs::remove_file(&paths.state).unwrap();
        recover_uninstall(&paths).unwrap();
        assert!(!journal.exists());
        assert!(!staged_releases.exists());
        assert!(!staged_shim.exists());
        assert!(!paths.state.exists());
        assert!(!exe.exists());
    }

    #[test]
    fn canonical_versions_and_channels() {
        assert!(parse_version("1.2.0-alpha.3").is_ok());
        assert!(parse_version("1.02.0").is_err());
        assert!(matches!(
            ReleaseChannel::for_version(&parse_version("1.2.0-beta.3").unwrap()),
            ReleaseChannel::Beta
        ));
    }
    #[test]
    fn ownership_marker_is_not_empty() {
        assert!(MANAGED_MARKER.len() > 10);
    }
    #[test]
    fn launcher_is_owned_and_repoints_without_touching_release_binary() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            releases: dir.path().join("releases"),
            shim: dir.path().join("bin").join(if cfg!(windows) {
                "soloist-display.cmd"
            } else {
                "soloist-display"
            }),
            state: dir.path().join("desktop-install.json"),
        };
        fs::create_dir_all(paths.executable("1.2.3").parent().unwrap()).unwrap();
        fs::write(paths.executable("1.2.3"), b"old").unwrap();
        paths.point_to("1.2.3").unwrap();
        assert!(String::from_utf8(fs::read(&paths.shim).unwrap())
            .unwrap()
            .contains(MANAGED_MARKER));
        paths.point_to("1.2.4").unwrap();
        assert!(paths.executable("1.2.3").exists());
        let contents = String::from_utf8(fs::read(&paths.shim).unwrap()).unwrap();
        assert!(contents.contains("1.2.4"));
        fs::write(&paths.shim, b"unowned launcher").unwrap();
        assert!(paths.point_to("1.2.5").is_err());
    }
}
