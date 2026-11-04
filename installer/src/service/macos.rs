use crate::{filesystem, paths::Paths, process};
use anyhow::{anyhow, Context, Result};
use std::{fs, path::Path, process::Command};

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn domain() -> Result<String> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("read user id")?;
    if !output.status.success() {
        return Err(anyhow!("could not determine launchd user domain"));
    }
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&output.stdout).trim()
    ))
}

fn plist(label: &str, program: &Path, arguments: &[String], log: &Path) -> String {
    let args = arguments
        .iter()
        .map(|argument| format!("<string>{}</string>", xml(argument)))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{}</string>
<key>ProgramArguments</key><array><string>{}</string>{}</array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>ProcessType</key><string>Background</string>
<key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(label),
        xml(&program.to_string_lossy()),
        args,
        xml(&log.to_string_lossy()),
        xml(&log.to_string_lossy())
    )
}

fn install(
    paths: &Paths,
    path: &Path,
    label: &str,
    program: &Path,
    arguments: &[String],
) -> Result<()> {
    if paths.is_test() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, label)?;
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let domain = domain()?;
    if !path.is_file()
        && Command::new("launchctl")
            .args(["print", &format!("{domain}/{label}")])
            .output()
            .is_ok_and(|output| output.status.success())
    {
        return Err(anyhow!("refusing to replace unowned launchd agent {label}"));
    }
    let log = paths.logs.join(format!("{label}.log"));
    fs::write(path, plist(label, program, arguments, &log))?;
    filesystem::set_private(path)?;
    let _ = Command::new("launchctl")
        .args(["bootout", &domain])
        .arg(path)
        .status();
    process::run(
        Command::new("launchctl")
            .args(["bootstrap", &domain])
            .arg(path),
        "register launchd agent",
        false,
    )?;
    process::run(
        Command::new("launchctl").args(["kickstart", "-k", &format!("{domain}/{label}")]),
        "start launchd agent",
        false,
    )
}

pub fn user_manager_available(paths: &Paths) -> bool {
    paths.is_test() || process::exists("launchctl")
}
pub fn install_display(paths: &Paths) -> Result<()> {
    install(
        paths,
        &paths.display_service,
        "dev.soloist-display",
        &paths.current.join("bin/soloist-audio-bridge"),
        &[],
    )
}
pub fn install_soloist(paths: &Paths, executable: &Path) -> Result<()> {
    install(
        paths,
        &paths.soloist_service,
        "com.spotify.soloist",
        &paths.current.join("bin/soloist-display"),
        &[
            "engine-service".into(),
            "--executable".into(),
            executable.to_string_lossy().into_owned(),
        ],
    )
}
pub fn start_display(paths: &Paths) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    if !paths.display_service.is_file() {
        return Err(anyhow!(
            "refusing to start an unowned Soloist Display agent"
        ));
    }
    process::run(
        Command::new("launchctl").args([
            "kickstart",
            "-k",
            &format!("{}/dev.soloist-display", domain()?),
        ]),
        "start Soloist Display",
        true,
    )
}
pub fn stop_display(paths: &Paths) -> Result<()> {
    if paths.is_test() || !paths.display_service.is_file() {
        return Ok(());
    }
    let _ = Command::new("launchctl")
        .args([
            "kill",
            "SIGTERM",
            &format!("{}/dev.soloist-display", domain()?),
        ])
        .status();
    Ok(())
}
pub fn active(paths: &Paths, unit: &str) -> bool {
    if paths.is_test() {
        return true;
    }
    let label = if unit.contains("display") {
        "dev.soloist-display"
    } else {
        "com.spotify.soloist"
    };
    domain().is_ok_and(|domain| {
        Command::new("launchctl")
            .args(["print", &format!("{domain}/{label}")])
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .any(|line| line.trim() == "state = running")
            })
    })
}
pub fn disable_owned(paths: &Paths, unit: &str) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    let path = if unit.contains("display") {
        &paths.display_service
    } else {
        &paths.soloist_service
    };
    if !path.is_file() {
        return Err(anyhow!("refusing to stop an unowned launchd agent"));
    }
    let _ = Command::new("launchctl")
        .args(["bootout", &domain()?])
        .arg(path)
        .status();
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
