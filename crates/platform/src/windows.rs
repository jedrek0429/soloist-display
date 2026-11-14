//! Windows per-user filesystem locations.
use std::{
    env, io,
    path::{Path, PathBuf},
};

pub fn settings_root() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Roaming"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
        .join("soloist-display")
}

pub fn portable_install_root() -> io::Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("soloist-display"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))
}

pub fn portable_bin_dir(root: &Path) -> io::Result<PathBuf> {
    Ok(root.join("bin"))
}

pub fn portable_shim_name() -> &'static str {
    "soloist-display.cmd"
}
