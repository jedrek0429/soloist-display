use crate::{
    paths::{path_string, Paths},
    process,
};
use anyhow::{anyhow, Context, Result};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

const DISPLAY_UNIT: &str = r#"[Unit]
Description=Soloist Display local server and audio spectrum
After=sound.target pipewire.service pipewire-pulse.service pulseaudio.service

[Service]
Type=simple
ExecStart=__BRIDGE__
Restart=on-failure
RestartSec=3
StartLimitIntervalSec=60
StartLimitBurst=5

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

const SOLOIST_UNIT: &str = r#"[Unit]
Description=Spotify Soloist playback
After=sound.target pipewire.service pipewire-pulse.service pulseaudio.service

[Service]
Type=simple
EnvironmentFile=__ENV__
ExecStart=__SOLOIST__ --device-name ${SOLOIST_NAME} --api-key ${SOLOIST_API_KEY} --ws 127.0.0.1:9090 --data-dir __DATA__ --cache-dir __CACHE__
Restart=on-failure
RestartSec=5
StartLimitIntervalSec=60
StartLimitBurst=5

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
            .status()
            .is_ok_and(|s| s.success())
}

pub fn install_display(paths: &Paths) -> Result<()> {
    let bridge = paths.current.join("bin/soloist-audio-bridge");
    write_if_changed(
        &paths.display_service,
        &DISPLAY_UNIT.replace("__BRIDGE__", &unit_quote(&bridge)),
        0o644,
    )?;
    systemctl(paths, &["daemon-reload"], true)?;
    // Restart the running bridge so embedded HTML/JS comes from the new release.
    systemctl(paths, &["enable", "soloist-display.service"], true)?;
    systemctl(paths, &["restart", "soloist-display.service"], false)
}

pub fn install_soloist(paths: &Paths, executable: &Path) -> Result<()> {
    let unit = SOLOIST_UNIT
        .replace("__ENV__", &unit_quote(&paths.soloist_env))
        .replace("__SOLOIST__", &unit_quote(executable))
        .replace("__DATA__", &unit_quote(&paths.soloist_data))
        .replace("__CACHE__", &unit_quote(&paths.soloist_cache));
    let changed = write_if_changed(&paths.soloist_service, &unit, 0o644)?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(paths, &["enable", "--now", "soloist.service"], false)?;
    if changed {
        systemctl(paths, &["restart", "soloist.service"], true)?;
    }
    Ok(())
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
    let directory = paths.soloist_service.parent().context("missing systemd user directory")?;
    let service_path = directory.join("soloist-display-renew.service");
    let timer_path = directory.join("soloist-display-renew.timer");
    write_if_changed(&service_path, &RENEWAL_SERVICE.replace("__MANAGER__", &unit_quote(&paths.visible_command)), 0o644)?;
    write_if_changed(&timer_path, RENEWAL_TIMER, 0o644)?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(paths, &["enable", "--now", "soloist-display-renew.timer"], true)
}

pub fn remove_renewal_timer(paths: &Paths) -> Result<()> {
    let directory = paths.soloist_service.parent().context("missing systemd user directory")?;
    systemctl(paths, &["disable", "--now", "soloist-display-renew.timer"], true)?;
    for name in ["soloist-display-renew.service", "soloist-display-renew.timer"] {
        let path = directory.join(name);
        if path.exists() { fs::remove_file(path)?; }
    }
    systemctl(paths, &["daemon-reload"], true)
}

pub fn soloist_unit_present(paths: &Paths) -> bool {
    if paths.is_test() {
        return paths.soloist_service.exists();
    }
    Command::new("systemctl")
        .args(["--user", "show", "soloist.service", "--property=LoadState", "--value"])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim() == "loaded"
        })
}

pub fn start_soloist_existing(paths: &Paths) -> Result<()> {
    if !soloist_unit_present(paths) {
        return Err(anyhow!("no existing Soloist systemd unit was found"));
    }
    systemctl(paths, &["start", "soloist.service"], false)
}

enum SavedUnit {
    Missing,
    File(Vec<u8>, u32),
    Symlink(PathBuf),
}

pub struct SoloistUnitSnapshot {
    unit: SavedUnit,
    was_active: bool,
    was_enabled: bool,
}

impl SoloistUnitSnapshot {
    pub fn capture(paths: &Paths) -> Result<Self> {
        let unit = match fs::symlink_metadata(&paths.soloist_service) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                SavedUnit::Symlink(fs::read_link(&paths.soloist_service)?)
            }
            Ok(metadata) if metadata.is_file() => SavedUnit::File(
                fs::read(&paths.soloist_service)?,
                metadata.permissions().mode() & 0o777,
            ),
            Ok(_) => return Err(anyhow!("unsupported existing Soloist unit file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SavedUnit::Missing,
            Err(error) => return Err(error.into()),
        };
        let was_enabled = !paths.is_test()
            && Command::new("systemctl")
                .args(["--user", "is-enabled", "--quiet", "soloist.service"])
                .status()
                .is_ok_and(|status| status.success());
        Ok(Self {
            unit,
            was_active: active(paths, "soloist.service") && !paths.is_test(),
            was_enabled,
        })
    }

    pub fn restore(&self, paths: &Paths) -> Result<()> {
        if fs::symlink_metadata(&paths.soloist_service).is_ok() {
            fs::remove_file(&paths.soloist_service)?;
        }
        match &self.unit {
            SavedUnit::Missing => {}
            SavedUnit::File(contents, mode) => {
                let temporary = paths
                    .soloist_service
                    .with_extension(format!("restore.{}", std::process::id()));
                fs::write(&temporary, contents)?;
                fs::set_permissions(&temporary, fs::Permissions::from_mode(*mode))?;
                fs::rename(temporary, &paths.soloist_service)?;
            }
            SavedUnit::Symlink(target) => {
                symlink(target, &paths.soloist_service)?;
            }
        }
        systemctl(paths, &["daemon-reload"], true)?;
        if self.was_enabled {
            systemctl(paths, &["enable", "soloist.service"], true)?;
        } else {
            systemctl(paths, &["disable", "soloist.service"], true)?;
        }
        if self.was_active {
            systemctl(paths, &["restart", "soloist.service"], true)?;
        } else {
            systemctl(paths, &["stop", "soloist.service"], true)?;
        }
        Ok(())
    }
}

pub fn restart_soloist(paths: &Paths) -> Result<()> {
    systemctl(paths, &["restart", "soloist.service"], true)
}
