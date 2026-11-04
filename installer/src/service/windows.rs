use crate::{paths::Paths, process};
use anyhow::{anyhow, Context, Result};
use std::{fs, path::Path, process::Command};

const DISPLAY_TASK: &str = "Soloist Display";
const SOLOIST_TASK: &str = "Spotify Soloist (Soloist Display)";

fn quote(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('"', "\\\""))
}

fn task_registered(name: &str) -> Result<bool> {
    let status = Command::new("schtasks")
        .args(["/Query", "/TN", name])
        .status()
        .context("query scheduled task")?;
    Ok(status.success())
}

fn create(paths: &Paths, marker: &Path, name: &str, command: String) -> Result<()> {
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    if paths.is_test() {
        fs::write(marker, &command)?;
        return Ok(());
    }
    if task_registered(name)? && !marker.is_file() {
        return Err(anyhow!("refusing to replace unowned scheduled task {name}"));
    }
    process::run(
        Command::new("schtasks").args([
            "/Create", "/F", "/SC", "ONLOGON", "/RL", "LIMITED", "/TN", name, "/TR", &command,
        ]),
        "register per-user startup task",
        false,
    )?;
    fs::write(marker, &command)?;
    process::run(
        Command::new("schtasks").args(["/Run", "/TN", name]),
        "start per-user task",
        true,
    )
}

pub fn user_manager_available(paths: &Paths) -> bool {
    paths.is_test() || process::exists("schtasks.exe")
}
pub fn install_display(paths: &Paths) -> Result<()> {
    create(
        paths,
        &paths.display_service,
        DISPLAY_TASK,
        quote(&paths.current.join("bin/soloist-audio-bridge.exe")),
    )
}
pub fn install_soloist(paths: &Paths, executable: &Path) -> Result<()> {
    create(
        paths,
        &paths.soloist_service,
        SOLOIST_TASK,
        format!(
            "{} engine-service --executable {}",
            quote(&paths.current.join("bin/soloist-display.exe")),
            quote(executable)
        ),
    )
}
pub fn start_display(paths: &Paths) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    if !paths.display_service.is_file() {
        return Err(anyhow!("refusing to start an unowned Soloist Display task"));
    }
    process::run(
        Command::new("schtasks").args(["/Run", "/TN", DISPLAY_TASK]),
        "start Soloist Display",
        true,
    )
}
pub fn stop_display(paths: &Paths) -> Result<()> {
    if paths.is_test() || !paths.display_service.is_file() {
        return Ok(());
    }
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", DISPLAY_TASK])
        .status();
    Ok(())
}
pub fn active(paths: &Paths, unit: &str) -> bool {
    if paths.is_test() {
        return true;
    }
    let name = if unit.contains("display") {
        DISPLAY_TASK
    } else {
        SOLOIST_TASK
    };
    let command = format!(
        "(Get-ScheduledTask -TaskName '{}' -ErrorAction SilentlyContinue).State",
        name.replace('\'', "''")
    );
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "Running"
        })
}
pub fn disable_owned(paths: &Paths, unit: &str) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    let name = if unit.contains("display") {
        DISPLAY_TASK
    } else {
        SOLOIST_TASK
    };
    let marker = if unit.contains("display") {
        &paths.display_service
    } else {
        &paths.soloist_service
    };
    if !marker.is_file() {
        return Err(anyhow!("refusing to remove unowned scheduled task {name}"));
    }
    if !task_registered(name)? {
        return Ok(());
    }
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", name])
        .status();
    let output = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", name])
        .output()
        .context("remove per-user startup task")?;
    if !output.status.success() && task_registered(name)? {
        return Err(anyhow!("could not remove {name}"));
    }
    Ok(())
}
pub fn daemon_reload(_paths: &Paths) -> Result<()> {
    Ok(())
}
pub fn start_external_soloist(_paths: &Paths) {}
pub fn independent_soloist_service_exists(paths: &Paths) -> bool {
    paths.soloist_service.exists()
}
pub fn external_soloist_executable() -> Option<std::path::PathBuf> {
    None
}
