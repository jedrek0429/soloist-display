use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use std::{collections::HashMap, fs, process::Command};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    Linux,
    Windows,
    Macos,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Distro {
    Arch,
    Ubuntu,
    Debian,
    Fedora,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    Aarch64,
    Armv7,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Platform {
    pub os: OperatingSystem,
    pub distro: Option<Distro>,
    pub architecture: Architecture,
    pub target: String,
    pub label: String,
}

impl Platform {
    pub fn detect() -> Result<Self> {
        #[cfg(target_os = "linux")]
        return detect_linux();
        #[cfg(windows)]
        return detect_windows();
        #[cfg(target_os = "macos")]
        return detect_macos();
        #[allow(unreachable_code)]
        Err(anyhow!(
            "Soloist Display does not support this operating system"
        ))
    }

    /// Official Spotify Soloist builds currently exist for every supported
    /// Linux target, Windows x86_64, and Apple Silicon macOS only.
    pub fn managed_soloist_supported(&self) -> bool {
        matches!(self.os, OperatingSystem::Linux)
            || matches!(
                (&self.os, &self.architecture),
                (OperatingSystem::Windows, Architecture::X86_64)
                    | (OperatingSystem::Macos, Architecture::Aarch64)
            )
    }

    pub fn soloist_archive(&self) -> Result<&'static str> {
        match (&self.os, &self.architecture) {
            (OperatingSystem::Linux, Architecture::X86_64) => Ok("linux-x86_64"),
            (OperatingSystem::Linux, Architecture::Aarch64) => Ok("linux-arm64"),
            (OperatingSystem::Linux, Architecture::Armv7) => Ok("linux-arm32"),
            (OperatingSystem::Windows, Architecture::X86_64) => Ok("windows-x86_64"),
            (OperatingSystem::Macos, Architecture::Aarch64) => Ok("macos-arm64"),
            _ => Err(anyhow!(
                "Spotify does not publish a managed Soloist build for {}",
                self.label
            )),
        }
    }
}

#[cfg(target_os = "linux")]
fn os_release() -> Result<HashMap<String, String>> {
    let content = fs::read_to_string("/etc/os-release").context("read /etc/os-release")?;
    let mut values = HashMap::new();
    for line in content.lines() {
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.into(), raw.trim_matches('"').into());
    }
    Ok(values)
}

#[cfg(target_os = "linux")]
fn shell_bitness() -> Result<u8> {
    let bytes = fs::read("/proc/self/exe").context("read /proc/self/exe for ABI detection")?;
    if bytes.len() < 5 || &bytes[..4] != b"\x7fELF" {
        return Err(anyhow!("running executable is not ELF"));
    }
    match bytes[4] {
        1 => Ok(32),
        2 => Ok(64),
        value => Err(anyhow!("unsupported ELF class {value}")),
    }
}

#[cfg(target_os = "linux")]
fn arm_hard_float() -> Result<bool> {
    let bytes = fs::read("/proc/self/exe")?;
    if bytes.len() < 40 || bytes[4] != 1 || bytes[5] != 1 {
        return Ok(false);
    }
    Ok(u32::from_le_bytes(bytes[36..40].try_into().unwrap()) & 0x400 != 0)
}

fn glibc_version(text: &str) -> Option<(u32, u32)> {
    text.split_whitespace().find_map(|token| {
        let token =
            token.trim_matches(|character: char| !character.is_ascii_digit() && character != '.');
        let (major, rest) = token.split_once('.')?;
        let minor = rest
            .split(|character: char| !character.is_ascii_digit())
            .next()?;
        Some((major.parse().ok()?, minor.parse().ok()?))
    })
}

#[cfg(target_os = "linux")]
fn detect_linux() -> Result<Platform> {
    let release = os_release()?;
    let id = release.get("ID").map(String::as_str).unwrap_or("");
    let like = release.get("ID_LIKE").map(String::as_str).unwrap_or("");
    let distro = if id == "arch" || like.split_whitespace().any(|value| value == "arch") {
        Distro::Arch
    } else if id == "ubuntu" {
        Distro::Ubuntu
    } else if id == "debian" || like.split_whitespace().any(|value| value == "debian") {
        Distro::Debian
    } else if id == "fedora" || like.split_whitespace().any(|value| value == "fedora") {
        Distro::Fedora
    } else {
        let name = release.get("PRETTY_NAME").map(String::as_str).unwrap_or(id);
        return Err(anyhow!("unsupported Linux distribution: {name}"));
    };
    let ldd = Command::new("ldd")
        .arg("--version")
        .output()
        .context("run ldd for libc detection")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&ldd.stdout),
        String::from_utf8_lossy(&ldd.stderr)
    );
    if text.to_lowercase().contains("musl") {
        return Err(anyhow!(
            "musl userlands cannot run Spotify's current Soloist builds"
        ));
    }
    let version =
        glibc_version(&text).ok_or_else(|| anyhow!("could not determine glibc version"))?;
    if version < (2, 38) {
        return Err(anyhow!(
            "glibc 2.38 or newer is required (found {}.{})",
            version.0,
            version.1
        ));
    }
    let machine = String::from_utf8(Command::new("uname").arg("-m").output()?.stdout)?
        .trim()
        .to_lowercase();
    let bits = shell_bitness()?;
    let architecture = match (machine.as_str(), bits) {
        ("x86_64" | "amd64", 64) => Architecture::X86_64,
        ("aarch64" | "arm64", 64) => Architecture::Aarch64,
        ("armv7l" | "armv8l" | "armhf", 32) | ("aarch64", 32) => {
            if !arm_hard_float()? {
                return Err(anyhow!("ARMv7 requires a hard-float EABI userland"));
            }
            let cpuinfo = fs::read_to_string("/proc/cpuinfo")
                .unwrap_or_default()
                .to_lowercase();
            if !cpuinfo.contains("vfpv3")
                && !cpuinfo.contains("vfpv4")
                && !cpuinfo.contains(" neon")
            {
                return Err(anyhow!("ARMv7 requires VFPv3 or newer"));
            }
            Architecture::Armv7
        }
        (_, 32) => return Err(anyhow!("unsupported 32-bit userland on {machine}")),
        _ => return Err(anyhow!("unsupported architecture: {machine} ({bits}-bit)")),
    };
    let target = match architecture {
        Architecture::X86_64 => "x86_64-unknown-linux-musl",
        Architecture::Aarch64 => "aarch64-unknown-linux-musl",
        Architecture::Armv7 => "armv7-unknown-linux-musleabihf",
    };
    Ok(Platform {
        os: OperatingSystem::Linux,
        label: format!("{:?} / {:?}", distro, architecture),
        distro: Some(distro),
        architecture,
        target: target.into(),
    })
}

#[cfg(windows)]
fn detect_windows() -> Result<Platform> {
    let (architecture, target) = match std::env::consts::ARCH {
        "x86_64" => (Architecture::X86_64, "x86_64-pc-windows-msvc"),
        "aarch64" => (Architecture::Aarch64, "aarch64-pc-windows-msvc"),
        value => return Err(anyhow!("unsupported Windows architecture: {value}")),
    };
    Ok(Platform {
        os: OperatingSystem::Windows,
        distro: None,
        label: format!("Windows / {:?}", architecture),
        architecture,
        target: target.into(),
    })
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Result<Platform> {
    let (architecture, target) = match std::env::consts::ARCH {
        "x86_64" => (Architecture::X86_64, "x86_64-apple-darwin"),
        "aarch64" => (Architecture::Aarch64, "aarch64-apple-darwin"),
        value => return Err(anyhow!("unsupported macOS architecture: {value}")),
    };
    Ok(Platform {
        os: OperatingSystem::Macos,
        distro: None,
        label: format!("macOS / {:?}", architecture),
        architecture,
        target: target.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_glibc_version_output() {
        assert_eq!(
            glibc_version("ldd (Ubuntu GLIBC 2.39-0ubuntu8) 2.39"),
            Some((2, 39))
        );
        assert_eq!(glibc_version("ldd (GNU libc) 2.41"), Some((2, 41)));
    }

    #[test]
    fn capability_matrix_does_not_invent_official_binaries() {
        let platform = |os, architecture| Platform {
            os,
            distro: None,
            architecture,
            target: "test".into(),
            label: "test".into(),
        };
        assert!(
            platform(OperatingSystem::Windows, Architecture::X86_64).managed_soloist_supported()
        );
        assert!(
            !platform(OperatingSystem::Windows, Architecture::Aarch64).managed_soloist_supported()
        );
        assert!(platform(OperatingSystem::Macos, Architecture::Aarch64).managed_soloist_supported());
        assert!(!platform(OperatingSystem::Macos, Architecture::X86_64).managed_soloist_supported());
    }
}
