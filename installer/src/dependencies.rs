use crate::{
    platform::{Distro, OperatingSystem, Platform},
    process,
};
use anyhow::{anyhow, bail, Context, Result};
use std::process::{Command, Stdio};

pub fn required(platform: &Platform) -> Vec<&'static str> {
    match platform.distro.as_ref() {
        Some(Distro::Arch) => vec!["ca-certificates", "xdg-utils", "libpulse", "firefox"],
        Some(Distro::Ubuntu) => vec![
            "ca-certificates",
            "xdg-utils",
            "pulseaudio-utils",
            "firefox",
        ],
        Some(Distro::Debian) => vec![
            "ca-certificates",
            "xdg-utils",
            "pulseaudio-utils",
            "firefox-esr",
        ],
        Some(Distro::Fedora) => vec![
            "ca-certificates",
            "xdg-utils",
            "pulseaudio-utils",
            "firefox",
        ],
        None => vec![],
    }
}

fn command_exists(name: &str) -> bool {
    process::exists(name)
}

pub fn runtime_ready(platform: &Platform) -> bool {
    if !matches!(platform.os, OperatingSystem::Linux) {
        return true;
    }
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
    if runtime_ready(platform) {
        return Ok(vec![]);
    }
    if !matches!(platform.os, OperatingSystem::Linux) {
        return Ok(vec![]);
    }
    if !command_exists("sudo") {
        return Err(anyhow!("sudo is required to install system dependencies"));
    }
    let packages = required(platform);
    let mut command = Command::new("sudo");
    match platform.distro.as_ref() {
        Some(Distro::Arch) => {
            command.args(["pacman", "-S", "--needed", "--noconfirm"]);
        }
        Some(Distro::Ubuntu | Distro::Debian) => {
            let update = Command::new("sudo")
                .args(["apt-get", "update"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .context("run apt-get update")?;
            if !update.success() {
                bail!("apt-get update failed");
            }
            command.args(["apt-get", "install", "-y"]);
        }
        Some(Distro::Fedora) => {
            command.args(["dnf", "install", "-y"]);
        }
        None => unreachable!("non-Linux platforms returned before package installation"),
    }
    command.args(&packages);
    let _ = quiet;
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let status = command.status().context("install system dependencies")?;
    if !status.success() {
        bail!("system dependency installation failed with {status}");
    }
    if !runtime_ready(platform) {
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
            os: OperatingSystem::Linux,
            distro: Some(distro),
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
