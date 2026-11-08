//! macOS per-user filesystem locations.
use std::{env, io, path::{Path, PathBuf}};

pub fn settings_root() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
        .join("Library/Application Support/soloist-display")
}

pub fn desktop_install_root() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|p| p.join("Library/Application Support/soloist-display/app"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

pub fn desktop_bin_dir(_root: &Path) -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|p| p.join(".local/bin"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

pub fn desktop_shim_name() -> &'static str { "soloist-display" }
