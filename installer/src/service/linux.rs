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

const SOLOIST_UNIT: &str = r#"[Unit]
Description=Spotify Soloist playback (managed by Soloist Display)
After=sound.target pipewire.service pipewire-pulse.service pulseaudio.service

[Service]
Type=simple
ExecStart=__MANAGER__ engine-service --executable __SOLOIST__
Restart=on-failure
RestartSec=5
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

fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, contents)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))?;
    fs::rename(temporary, path)?;
    Ok(())
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
            .is_ok_and(|status| status.success())
}

pub fn install_display(paths: &Paths) -> Result<()> {
    let bridge = paths.current.join("bin/soloist-audio-bridge");
    write_if_changed(
        &paths.display_service,
        &DISPLAY_UNIT.replace("__BRIDGE__", &unit_quote(&bridge)),
    )?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(
        paths,
        &["enable", "--now", "soloist-display.service"],
        false,
    )
}

pub fn install_soloist(paths: &Paths, executable: &Path) -> Result<()> {
    let manager = paths.current.join("bin/soloist-display");
    let unit = SOLOIST_UNIT
        .replace("__MANAGER__", &unit_quote(&manager))
        .replace("__SOLOIST__", &unit_quote(executable));
    write_if_changed(&paths.soloist_service, &unit)?;
    systemctl(paths, &["daemon-reload"], true)?;
    systemctl(paths, &["enable", "--now", "soloist.service"], false)
}

pub fn start_display(paths: &Paths) -> Result<()> {
    systemctl(paths, &["start", "soloist-display.service"], true)
}
pub fn stop_display(paths: &Paths) -> Result<()> {
    systemctl(paths, &["stop", "soloist-display.service"], true)
}

pub fn active(paths: &Paths, unit: &str) -> bool {
    paths.is_test()
        || Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", unit])
            .status()
            .is_ok_and(|status| status.success())
}

pub fn disable_owned(paths: &Paths, unit: &str) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    let output = Command::new("systemctl")
        .args(["--user", "disable", "--now", unit])
        .output()
        .context("stop user service")?;
    if !output.status.success() && active(paths, unit) {
        return Err(anyhow!("could not stop {unit}"));
    }
    Ok(())
}

pub fn daemon_reload(paths: &Paths) -> Result<()> {
    systemctl(paths, &["daemon-reload"], true)
}

pub fn start_external_soloist(paths: &Paths) {
    if !paths.is_test() {
        let _ = Command::new("systemctl")
            .args(["--user", "start", "soloist.service"])
            .status();
    }
}

pub fn independent_soloist_service_exists(paths: &Paths) -> bool {
    if paths.soloist_service.exists() {
        return true;
    }
    Command::new("systemctl")
        .args(["--user", "cat", "soloist.service"])
        .output()
        .is_ok_and(|output| output.status.success())
}

pub fn external_soloist_executable() -> Option<std::path::PathBuf> {
    let output = Command::new("systemctl")
        .args([
            "--user",
            "show",
            "soloist.service",
            "--property=ExecStart",
            "--value",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split(|character: char| character.is_whitespace() || character == '{' || character == ';')
        .find(|part| part.ends_with("/soloist"))
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
}
