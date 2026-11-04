use crate::{
    paths::{path_string, Paths},
    process,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

const DISPLAY_UNIT: &str = r#"[Unit]
Description=Soloist Display local server and audio spectrum
After=sound.target pipewire.service pipewire-pulse.service pulseaudio.service
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=simple
ExecStart=__BRIDGE__
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
"#;

fn unit_quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path_string(path)
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

fn environment_file_path(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        bail!("Soloist EnvironmentFile path must be absolute");
    }
    let value = path_string(path);
    if value.contains(['\n', '\r', '\0']) {
        bail!("Soloist EnvironmentFile path contains an invalid character");
    }
    Ok(value.replace('%', "%%"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedRuntime {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub websocket_url: String,
    #[serde(default)]
    pub cache_size_mb: Option<u64>,
    #[serde(default)]
    pub pipewire_device: Option<String>,
    #[serde(default)]
    pub initial_volume: Option<u8>,
    #[serde(default)]
    pub verbose: bool,
}

impl ManagedRuntime {
    pub fn default_for(paths: &Paths) -> Self {
        Self {
            data_dir: paths.soloist_data.clone(),
            cache_dir: paths.soloist_cache.clone(),
            websocket_url: "ws://127.0.0.1:9090/".into(),
            cache_size_mb: None,
            pipewire_device: None,
            initial_volume: None,
            verbose: false,
        }
    }

    fn path(paths: &Paths) -> PathBuf {
        paths.state.join("soloist-runtime.json")
    }

    pub fn load(paths: &Paths) -> Result<Self> {
        let path = Self::path(paths);
        if !path.exists() {
            return Ok(Self::default_for(paths));
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        if !self.data_dir.is_absolute() || !self.cache_dir.is_absolute() {
            anyhow::bail!("Soloist runtime paths must be absolute");
        }
        crate::soloist::validate_websocket_url(&self.websocket_url)?;
        self.extra_arguments()?;
        crate::state::atomic_json(&Self::path(paths), self, Some(0o600))
    }

    fn extra_arguments(&self) -> Result<String> {
        let mut args = String::new();
        if let Some(size) = self.cache_size_mb {
            if size != 0 && size < 100 {
                bail!("Soloist cache size must be zero or at least 100 MB");
            }
            args.push_str(&format!(" --cache-size {size}"));
        }
        if let Some(device) = &self.pipewire_device {
            if device.trim().is_empty() || device.contains(['\n', '\r', '\0']) {
                bail!("invalid PipeWire device name");
            }
            args.push_str(&format!(
                " --pipewire-device {}",
                unit_quote(Path::new(device))
            ));
        }
        if let Some(volume) = self.initial_volume {
            if volume > 100 {
                bail!("Soloist initial volume must be between 0 and 100");
            }
            args.push_str(&format!(" --initial-volume {volume}"));
        }
        if self.verbose {
            args.push_str(" --verbose");
        }
        Ok(args)
    }

    fn websocket_bind(&self) -> Result<String> {
        let url = url::Url::parse(&self.websocket_url)?;
        let host = url.host_str().context("missing WebSocket hostname")?;
        let port = url.port().context("missing WebSocket port")?;
        Ok(if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        })
    }
}

const SOLOIST_UNIT: &str = r#"[Unit]
Description=Spotify Soloist playback
After=sound.target pipewire.service pipewire-pulse.service pulseaudio.service
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=simple
EnvironmentFile=__ENV__
ExecStart=__SOLOIST__ --device-name ${SOLOIST_NAME} --api-key ${SOLOIST_API_KEY} --ws __WS__ --data-dir __DATA__ --cache-dir __CACHE____EXTRA__
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#;

fn write_if_changed(path: &Path, contents: &str, mode: u32) -> Result<bool> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, contents)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))?;
    fs::rename(temporary, path)?;
    Ok(true)
}

fn systemctl(paths: &Paths, arguments: &[&str], quiet: bool) -> Result<()> {
    if std::env::var("SOLOIST_DISPLAY_FAULT").as_deref() == Ok("systemd") {
        return Err(anyhow!("injected systemd failure"));
    }
    if paths.is_test() {
        return Ok(());
    }
    process::run(
        Command::new("systemctl").arg("--user").args(arguments),
        &format!("systemctl --user {}", arguments.join(" ")),
        quiet,
    )
}

pub fn user_manager_available(paths: &Paths) -> bool {
    paths.is_test()
        || Command::new("systemctl")
            .args(["--user", "show-environment"])
            .output()
            .is_ok_and(|output| output.status.success())
}

pub fn install_display(paths: &Paths) -> Result<()> {
    let bridge = paths.current.join("bin/soloist-audio-bridge");
    write_if_changed(
        &paths.display_service,
        &DISPLAY_UNIT.replace("__BRIDGE__", &unit_quote(&bridge)),
        0o644,
    )?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(paths, &["enable", "soloist-display.service"], true)?;
    systemctl(paths, &["restart", "soloist-display.service"], false)
}

pub fn install_soloist(paths: &Paths, executable: &Path) -> Result<()> {
    let runtime = ManagedRuntime::load(paths)?;
    let unit = SOLOIST_UNIT
        .replace("__ENV__", &environment_file_path(&paths.soloist_env)?)
        .replace("__SOLOIST__", &unit_quote(executable))
        .replace("__WS__", &unit_quote(Path::new(&runtime.websocket_bind()?)))
        .replace("__DATA__", &unit_quote(&runtime.data_dir))
        .replace("__CACHE__", &unit_quote(&runtime.cache_dir))
        .replace("__EXTRA__", &runtime.extra_arguments()?);
    if !paths.is_test() && !paths.soloist_env.is_file() {
        bail!("Soloist credentials are missing; run the installer interactively to configure them");
    }
    write_if_changed(&paths.soloist_service, &unit, 0o644)?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(paths, &["enable", "soloist.service"], true)?;
    systemctl(paths, &["reset-failed", "soloist.service"], true)?;
    systemctl(paths, &["restart", "soloist.service"], true)
}

pub fn start_display(paths: &Paths) -> Result<()> {
    systemctl(paths, &["start", "soloist-display.service"], true)
}

pub fn active(paths: &Paths, unit: &str) -> bool {
    paths.is_test()
        || Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", unit])
            .status()
            .is_ok_and(|s| s.success())
}

fn service_pid(paths: &Paths, unit: &str) -> Option<u32> {
    if paths.is_test() {
        return None;
    }
    let result = Command::new("systemctl")
        .args(["--user", "show", unit, "--property=MainPID", "--value"])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    let pid = String::from_utf8(result.stdout)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    (pid > 0).then_some(pid)
}

fn owns_listener(pid: u32, endpoint: &str) -> bool {
    use std::collections::HashSet;
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    let fd_dir = format!("/proc/{pid}/fd");
    let Ok(fds) = fs::read_dir(fd_dir) else {
        return false;
    };
    let sockets: HashSet<u64> = fds
        .filter_map(|fd| fd.ok())
        .filter_map(|fd| fs::read_link(fd.path()).ok())
        .filter_map(|link| {
            link.to_str()?
                .strip_prefix("socket:[")?
                .strip_suffix(']')?
                .parse::<u64>()
                .ok()
        })
        .collect();
    if sockets.is_empty() {
        return false;
    }
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(contents) = fs::read_to_string(table) else {
            continue;
        };
        for line in contents.lines().skip(1) {
            let cols: Vec<_> = line.split_whitespace().collect();
            if cols.len() < 10 || cols[3] != "0A" {
                continue;
            }
            let Some((_, hex_port)) = cols[1].rsplit_once(':') else {
                continue;
            };
            if u16::from_str_radix(hex_port, 16).ok() == Some(port)
                && cols[9]
                    .parse::<u64>()
                    .is_ok_and(|inode| sockets.contains(&inode))
            {
                return true;
            }
        }
    }
    false
}

pub fn managed_process_healthy(
    paths: &Paths,
    expected_binary: &Path,
    endpoint: &str,
    websocket_healthy: impl FnOnce() -> bool,
) -> bool {
    if paths.is_test() {
        return true;
    }
    use std::os::unix::fs::MetadataExt;
    if !active(paths, "soloist.service") {
        return false;
    }
    let Some(pid) = service_pid(paths, "soloist.service") else {
        return false;
    };
    let executable = format!("/proc/{pid}/exe");
    let (Ok(actual), Ok(expected)) = (fs::metadata(executable), fs::metadata(expected_binary))
    else {
        return false;
    };
    if actual.dev() != expected.dev() || actual.ino() != expected.ino() {
        return false;
    }
    if !owns_listener(pid, endpoint) {
        return false;
    }
    websocket_healthy()
        && active(paths, "soloist.service")
        && service_pid(paths, "soloist.service") == Some(pid)
}

pub fn enabled(paths: &Paths, unit: &str) -> bool {
    !paths.is_test()
        && Command::new("systemctl")
            .args(["--user", "is-enabled", "--quiet", unit])
            .status()
            .is_ok_and(|status| status.success())
}

pub fn restore_unit_state(
    paths: &Paths,
    unit: &str,
    was_enabled: bool,
    was_active: bool,
) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    if was_enabled {
        systemctl(paths, &["enable", unit], true)?;
    } else {
        systemctl(paths, &["disable", unit], true)?;
    }
    if was_active {
        systemctl(paths, &["restart", unit], true)?;
    } else {
        systemctl(paths, &["stop", unit], true)?;
    }
    Ok(())
}

pub fn disable_owned(paths: &Paths, unit: &str) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    let status = Command::new("systemctl")
        .args(["--user", "disable", "--now", unit])
        .output()
        .context("stop user service")?;
    if !status.status.success() && active(paths, unit) {
        return Err(anyhow!("could not stop {unit}"));
    }
    Ok(())
}

pub fn daemon_reload(paths: &Paths) -> Result<()> {
    systemctl(paths, &["daemon-reload"], true)
}

const RENEWAL_SERVICE: &str = "[Unit]\nDescription=Check Spotify Soloist binary updates\n\n[Service]\nType=oneshot\nExecStart=__MANAGER__ renew-soloist\n";
const RENEWAL_TIMER: &str = "[Unit]\nDescription=Check Spotify Soloist builds every six hours\n\n[Timer]\nOnBootSec=10min\nOnUnitActiveSec=6h\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n";

pub fn install_renewal_timer(paths: &Paths) -> Result<()> {
    let directory = paths
        .soloist_service
        .parent()
        .context("missing systemd user directory")?;
    let service_path = directory.join("soloist-display-renew.service");
    let timer_path = directory.join("soloist-display-renew.timer");
    write_if_changed(
        &service_path,
        &RENEWAL_SERVICE.replace("__MANAGER__", &unit_quote(&paths.visible_command)),
        0o644,
    )?;
    write_if_changed(&timer_path, RENEWAL_TIMER, 0o644)?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(
        paths,
        &["enable", "--now", "soloist-display-renew.timer"],
        true,
    )
}

pub fn remove_renewal_timer(paths: &Paths) -> Result<()> {
    let directory = paths
        .soloist_service
        .parent()
        .context("missing systemd user directory")?;
    let service_path = directory.join("soloist-display-renew.service");
    let timer_path = directory.join("soloist-display-renew.timer");
    if !service_path.exists() && !timer_path.exists() {
        return Ok(());
    }
    systemctl(
        paths,
        &["disable", "--now", "soloist-display-renew.timer"],
        true,
    )?;
    for path in [service_path, timer_path] {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    systemctl(paths, &["daemon-reload"], true)
}

pub fn soloist_unit_present(paths: &Paths) -> bool {
    if paths.is_test() {
        return paths.soloist_service.exists();
    }
    Command::new("systemctl")
        .args([
            "--user",
            "show",
            "soloist.service",
            "--property=LoadState",
            "--value",
        ])
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "loaded"
        })
}

pub fn stop_soloist_existing(paths: &Paths) -> Result<()> {
    if soloist_unit_present(paths) {
        systemctl(paths, &["stop", "soloist.service"], true)?;
    }
    Ok(())
}

pub fn start_soloist_existing(paths: &Paths) -> Result<()> {
    if !soloist_unit_present(paths) {
        return Err(anyhow!("no existing Soloist systemd unit was found"));
    }
    systemctl(paths, &["start", "soloist.service"], false)
}

#[derive(Serialize, Deserialize)]
enum SavedUnit {
    Missing,
    File(Vec<u8>, u32),
    Symlink(PathBuf),
}

#[derive(Serialize, Deserialize)]
pub struct SoloistUnitSnapshot {
    unit: SavedUnit,
    #[serde(default)]
    previous_env: Option<SavedUnit>,
    was_active: bool,
    was_enabled: bool,
}

fn save_original(path: &Path) -> Result<SavedUnit> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Ok(SavedUnit::Symlink(fs::read_link(path)?))
        }
        Ok(metadata) if metadata.is_file() => Ok(SavedUnit::File(
            fs::read(path)?,
            metadata.permissions().mode() & 0o777,
        )),
        Ok(_) => Err(anyhow!("unsupported file type at {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SavedUnit::Missing),
        Err(error) => Err(error.into()),
    }
}

fn restore_original(path: &Path, saved: &SavedUnit) -> Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)?;
    }
    match saved {
        SavedUnit::Missing => {}
        SavedUnit::File(bytes, mode) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let staged = path.with_extension(format!("restore.{}", std::process::id()));
            fs::write(&staged, bytes)?;
            fs::set_permissions(&staged, fs::Permissions::from_mode(*mode))?;
            fs::rename(staged, path)?;
        }
        SavedUnit::Symlink(target) => {
            symlink(target, path)?;
        }
    }
    Ok(())
}

impl SoloistUnitSnapshot {
    fn backup_path(paths: &Paths) -> PathBuf {
        paths.state.join("previous-soloist-unit.json")
    }

    pub fn persist(&self, paths: &Paths) -> Result<()> {
        let path = Self::backup_path(paths);
        if !path.exists() {
            crate::state::atomic_json(&path, self, Some(0o600))?;
        }
        Ok(())
    }

    pub fn load(paths: &Paths) -> Result<Option<Self>> {
        match fs::read(Self::backup_path(paths)) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn remove_backup(paths: &Paths) -> Result<()> {
        let path = Self::backup_path(paths);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn capture(paths: &Paths) -> Result<Self> {
        let unit = save_original(&paths.soloist_service)?;
        let previous_env = Some(save_original(&paths.soloist_env)?);
        let was_enabled = !paths.is_test()
            && Command::new("systemctl")
                .args(["--user", "is-enabled", "--quiet", "soloist.service"])
                .status()
                .is_ok_and(|status| status.success());
        Ok(Self {
            unit,
            previous_env,
            was_active: active(paths, "soloist.service") && !paths.is_test(),
            was_enabled,
        })
    }

    pub fn has_previous(&self) -> bool {
        !matches!(self.unit, SavedUnit::Missing) || self.was_active || self.was_enabled
    }

    pub fn was_active(&self) -> bool {
        self.was_active
    }

    pub fn restore(&self, paths: &Paths) -> Result<()> {
        if !paths.is_test() && active(paths, "soloist.service") {
            systemctl(paths, &["stop", "soloist.service"], true)?;
        }
        if let Some(env) = &self.previous_env {
            restore_original(&paths.soloist_env, env)?;
        }
        restore_original(&paths.soloist_service, &self.unit)?;
        systemctl(paths, &["daemon-reload"], true)?;
        if matches!(self.unit, SavedUnit::Missing) && !self.was_active && !self.was_enabled {
            return Ok(());
        }
        if self.was_enabled {
            systemctl(paths, &["enable", "soloist.service"], true)?;
        } else {
            systemctl(paths, &["disable", "soloist.service"], true)?;
        }
        if self.was_active {
            systemctl(paths, &["restart", "soloist.service"], true)?;
        }
        Ok(())
    }
}

pub fn restart_soloist(paths: &Paths) -> Result<()> {
    systemctl(paths, &["restart", "soloist.service"], true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn environment_file_path_is_absolute_and_not_quoted() {
        assert_eq!(
            environment_file_path(Path::new("/home/admin/.config/soloist-display/soloist.env"))
                .unwrap(),
            "/home/admin/.config/soloist-display/soloist.env"
        );
        assert_eq!(
            environment_file_path(Path::new("/home/user with spaces/100%/soloist.env")).unwrap(),
            "/home/user with spaces/100%%/soloist.env"
        );
        assert!(environment_file_path(Path::new("relative/soloist.env")).is_err());
        assert!(environment_file_path(Path::new("/tmp/unsafe\npath")).is_err());
    }

    #[test]
    #[serial]
    fn generated_units_pass_systemd_analyze_verify() {
        let directory = tempfile::Builder::new()
            .prefix("soloist display ")
            .tempdir()
            .unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var(
            "SOLOIST_DISPLAY_TEST_ROOT",
            directory.path().join("test root"),
        );
        let result = (|| -> Result<()> {
            let paths = Paths::discover()?;
            paths.ensure_management_dirs()?;
            fs::create_dir_all(paths.soloist_env.parent().unwrap())?;
            fs::write(
                &paths.soloist_env,
                b"SOLOIST_NAME=Test\nSOLOIST_API_KEY=test\n",
            )?;
            install_soloist(&paths, Path::new("/bin/true"))?;

            let bridge = paths.current.join("bin/soloist-audio-bridge");
            fs::create_dir_all(bridge.parent().unwrap())?;
            fs::write(&bridge, b"#!/bin/sh\nexit 0\n")?;
            fs::set_permissions(&bridge, fs::Permissions::from_mode(0o755))?;
            install_display(&paths)?;

            for path in [&paths.soloist_service, &paths.display_service] {
                let output = Command::new("systemd-analyze")
                    .arg("verify")
                    .arg(path)
                    .output()?;
                assert!(
                    output.status.success() && output.stderr.is_empty(),
                    "systemd rejected {}: {}",
                    path.display(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Ok(())
        })();
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
        result.unwrap();
    }

    #[test]
    #[serial]
    fn restores_previous_live_systemd_user_service() {
        if std::env::var("SOLOIST_DISPLAY_RUN_SYSTEMD_TEST").as_deref() != Ok("1") {
            return;
        }
        let paths = Paths::discover().unwrap();
        assert!(!paths.is_test());
        assert!(user_manager_available(&paths));
        assert!(!paths.soloist_service.exists());
        paths.ensure_management_dirs().unwrap();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        let original = "[Unit]\nDescription=Original Soloist test unit\n[Service]\nExecStart=/usr/bin/sleep 3600\n[Install]\nWantedBy=default.target\n";
        fs::write(&paths.soloist_service, original).unwrap();
        let test_result = (|| -> Result<()> {
            daemon_reload(&paths)?;
            systemctl(&paths, &["enable", "--now", "soloist.service"], true)?;
            let snapshot = SoloistUnitSnapshot::capture(&paths)?;
            assert!(snapshot.was_active);
            assert!(snapshot.was_enabled);
            fs::write(
                &paths.soloist_service,
                "[Unit]\nDescription=Managed Soloist test unit\n[Service]\nExecStart=/usr/bin/sleep 3600\n[Install]\nWantedBy=default.target\n",
            )?;
            daemon_reload(&paths)?;
            systemctl(&paths, &["restart", "soloist.service"], true)?;
            snapshot.restore(&paths)?;
            assert_eq!(fs::read_to_string(&paths.soloist_service)?, original);
            assert!(active(&paths, "soloist.service"));
            Ok(())
        })();
        let _ = systemctl(&paths, &["disable", "--now", "soloist.service"], true);
        fs::remove_file(&paths.soloist_service).ok();
        let _ = daemon_reload(&paths);
        test_result.unwrap();
    }
}
