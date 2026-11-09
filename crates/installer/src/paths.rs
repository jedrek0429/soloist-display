use anyhow::{Context, Result};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    pub data_home: PathBuf,
    pub config_home: PathBuf,
    pub state_home: PathBuf,
    pub cache_home: PathBuf,
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub releases: PathBuf,
    pub current: PathBuf,
    pub transactions: PathBuf,
    pub logs: PathBuf,
    pub lock: PathBuf,
    pub install_state: PathBuf,
    pub bin_dir: PathBuf,
    pub visible_command: PathBuf,
    pub display_service: PathBuf,
    pub soloist_service: PathBuf,
    pub applications: PathBuf,
    pub soloist_env: PathBuf,
    pub managed_soloist: PathBuf,
    pub managed_soloist_current: PathBuf,
    pub soloist_data: PathBuf,
    pub soloist_cache: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let dirs = soloist_display_platform::resolve_linux_paths()?;
        let home = dirs.home;
        let data_home = dirs.data_home;
        let config_home = dirs.config_home;
        let state_home = dirs.state_home;
        let cache_home = dirs.cache_home;
        let bin_dir = dirs.bin_dir;

        let root = data_home.join("soloist-display");
        let config = config_home.join("soloist-display");
        let state = state_home.join("soloist-display");
        let cache = cache_home.join("soloist-display");
        let systemd_user = config_home.join("systemd/user");
        let applications = data_home.join("applications");
        Ok(Self {
            home,
            data_home: data_home.clone(),
            config_home: config_home.clone(),
            state_home: state_home.clone(),
            cache_home: cache_home.clone(),
            releases: root.join("releases"),
            current: root.join("current"),
            transactions: state.join("transactions"),
            logs: state.join("logs"),
            lock: state.join("installer.lock"),
            install_state: state.join("install.json"),
            visible_command: bin_dir.join("soloist-display"),
            display_service: systemd_user.join("soloist-display.service"),
            soloist_service: systemd_user.join("soloist.service"),
            applications,
            soloist_env: config.join("soloist.env"),
            managed_soloist: root.join("soloist"),
            managed_soloist_current: root.join("soloist/current"),
            soloist_data: data_home.join("soloist"),
            soloist_cache: cache_home.join("soloist"),
            root,
            config,
            state,
            cache,
            bin_dir,
        })
    }

    pub fn ensure_management_dirs(&self) -> Result<()> {
        for path in [
            &self.root,
            &self.releases,
            &self.config,
            &self.state,
            &self.cache,
            &self.transactions,
            &self.logs,
        ] {
            std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
        }
        Ok(())
    }

    pub fn is_test(&self) -> bool {
        env::var_os("SOLOIST_DISPLAY_TEST_ROOT").is_some()
    }
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn supports_installation_paths_with_spaces() {
        let directory = tempfile::Builder::new()
            .prefix("soloist display ")
            .tempdir()
            .unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var(
            "SOLOIST_DISPLAY_TEST_ROOT",
            directory.path().join("test root"),
        );
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        assert!(paths.root.to_string_lossy().contains("test root"));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
