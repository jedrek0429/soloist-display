use crate::platform::{Distro, Platform};
use anyhow::{anyhow, bail, Context, Result};
use std::process::Command;

pub fn required(platform: &Platform) -> Vec<&'static str> {
    match platform.distro {
        Distro::Arch => vec!["ca-certificates", "xdg-utils", "libpulse", "firefox"],
        Distro::Ubuntu => vec![
            "ca-certificates",
            "xdg-utils",
            "pulseaudio-utils",
            "firefox",
        ],
        Distro::Debian => vec![
            "ca-certificates",
            "xdg-utils",
            "pulseaudio-utils",
            "firefox-esr",
        ],
        Distro::Fedora => vec![
            "ca-certificates",
            "xdg-utils",
            "pulseaudio-utils",
            "firefox",
        ],
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", "command -v \"$1\" >/dev/null 2>&1", "sh", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn runtime_ready() -> bool {
    ["pactl", "parec", "xdg-open", "systemctl"]
        .iter()
        .all(|name| command_exists(name))
        && [
            "firefox",
            "firefox-esr",
            "chromium",
            "chromium-browser",
            "google-chrome",
            "brave-browser",
        ]
        .iter()
        .any(|name| command_exists(name))
}

pub fn install(platform: &Platform, quiet: bool) -> Result<Vec<String>> {
    if std::env::var("SOLOIST_DISPLAY_FAULT").as_deref() == Ok("package_install") {
        bail!("injected package installation failure");
    }
    if std::env::var_os("SOLOIST_DISPLAY_TEST_ROOT").is_some() {
        return Ok(vec![]);
    }
    if runtime_ready() {
        return Ok(vec![]);
    }
    if !command_exists("sudo") {
        return Err(anyhow!("sudo is required to install system dependencies"));
    }
    let packages = required(platform);
    let mut command = Command::new("sudo");
    match platform.distro {
        Distro::Arch => {
            command.args(["pacman", "-S", "--needed", "--noconfirm"]);
        }
        Distro::Ubuntu | Distro::Debian => {
            let mut update = Command::new("sudo");
            update.args(["apt-get", "update"]);
            crate::process::run(&mut update, "apt-get update", quiet)?;
            command.args(["apt-get", "install", "-y"]);
        }
        Distro::Fedora => {
            command.args(["dnf", "install", "-y"]);
        }
    }
    command.args(&packages);
    crate::process::run(&mut command, "install system dependencies", quiet)?;
    if !runtime_ready() {
        bail!("required audio, desktop, or browser commands are still missing after package installation");
    }
    Ok(packages.into_iter().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Architecture;
    use serial_test::serial;

    fn platform(distro: Distro) -> Platform {
        Platform {
            distro,
            architecture: Architecture::X86_64,
            target: "test".into(),
            label: "test".into(),
        }
    }

    #[test]
    fn every_supported_distribution_has_runtime_packages() {
        for distro in [Distro::Arch, Distro::Ubuntu, Distro::Debian, Distro::Fedora] {
            let packages = required(&platform(distro));
            assert!(packages.contains(&"ca-certificates"));
            assert!(packages.iter().any(|package| package.contains("pulse")));
        }
    }

    #[test]
    #[serial]
    fn failed_package_installation_is_reported() {
        std::env::set_var("SOLOIST_DISPLAY_FAULT", "package_install");
        std::env::set_var(
            "SOLOIST_DISPLAY_TEST_ROOT",
            "/tmp/soloist-display-fault-test",
        );
        assert!(install(&platform(Distro::Arch), true).is_err());
        std::env::remove_var("SOLOIST_DISPLAY_FAULT");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
