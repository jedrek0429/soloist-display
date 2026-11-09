//! Thin platform boundary. Installation decisions and ownership belong in the shared core.
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::path::Path;
use std::{env, path::PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "linux")]
pub use linux::{resolve_linux_paths, Architecture, Distro, LinuxBasePaths, Platform};
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub fn settings_root() -> PathBuf {
    env::var_os("SOLOIST_DISPLAY_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(platform::settings_root)
}

pub fn config_path() -> PathBuf {
    settings_root().join("config.json")
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn desktop_install_root() -> std::io::Result<PathBuf> {
    env::var_os("SOLOIST_DISPLAY_INSTALL_ROOT")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(platform::desktop_install_root)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn desktop_bin_dir(root: &Path) -> std::io::Result<PathBuf> {
    env::var_os("SOLOIST_DISPLAY_BIN_DIR")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| platform::desktop_bin_dir(root))
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn desktop_shim_name() -> &'static str {
    platform::desktop_shim_name()
}
