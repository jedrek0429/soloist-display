//! Linux/XDG defaults; the installer may apply stricter validation.
use std::{env, path::PathBuf};

pub fn settings_root() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("soloist-display")
}
