use crate::{
    paths::{path_string, Paths},
    process,
};
use anyhow::{anyhow, Context, Result};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

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
    write_if_changed(&paths.soloist_service, &unit, 0o644)?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(paths, &["enable", "--now", "soloist.service"], false)
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
