use crate::{paths::Paths, service, state::atomic_json};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum Phase {
    Preparing,
    Committed,
}

#[derive(Clone, Serialize, Deserialize)]
struct MovedPath {
    original: PathBuf,
    staged: PathBuf,
}

#[derive(Serialize, Deserialize)]
pub struct UninstallTransaction {
    phase: Phase,
    moved: Vec<MovedPath>,
    previous_display_unit: Option<Vec<u8>>,
    previous_soloist_unit: Option<Vec<u8>>,
    previous_soloist_env: Option<Vec<u8>>,
    previous_timer_unit: Option<Vec<u8>>,
    previous_timer_service: Option<Vec<u8>>,
    managed: bool,
    display_active: bool,
    display_enabled: bool,
    soloist_active: bool,
    soloist_enabled: bool,
    timer_active: bool,
    timer_enabled: bool,
}

fn journal_path(paths: &Paths) -> PathBuf {
    paths.state_home.join(".soloist-display-uninstall.json")
}

pub fn pending(paths: &Paths) -> bool {
    journal_path(paths).exists()
}

fn bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn restore_file(path: &Path, contents: Option<&[u8]>, mode: u32) -> Result<()> {
    if let Some(contents) = contents {
        let parent = path.parent().context("missing parent directory")?;
        fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(".restore-{}", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&tmp)?;
        io::Write::write_all(&mut file, contents)?;
        file.sync_all()?;
        fs::rename(tmp, path)?;
        fs::File::open(parent)?.sync_all()?;
    } else if fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)?;
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

fn staged_name(path: &Path) -> Result<PathBuf> {
    let original = path.file_name().context("cannot stage filesystem root")?;
    Ok(path.with_file_name(format!(
        ".soloist-display-remove-{}-{}",
        std::process::id(),
        original.to_string_lossy()
    )))
}

impl UninstallTransaction {
    pub fn begin(paths: &Paths, managed: bool) -> Result<Self> {
        let journal = journal_path(paths);
        if journal.exists() {
            bail!("incomplete uninstall; recovery is required before retrying");
        }
        let unit_dir = paths
            .soloist_service
            .parent()
            .context("missing systemd unit directory")?;
        let result = Self {
            phase: Phase::Preparing,
            moved: Vec::new(),
            previous_display_unit: bytes(&paths.display_service)?,
            previous_soloist_unit: if managed {
                bytes(&paths.soloist_service)?
            } else {
                None
            },
            previous_soloist_env: if managed {
                bytes(&paths.soloist_env)?
            } else {
                None
            },
            previous_timer_unit: bytes(&unit_dir.join("soloist-display-renew.timer"))?,
            previous_timer_service: bytes(&unit_dir.join("soloist-display-renew.service"))?,
            managed,
            display_active: !paths.is_test() && service::active(paths, "soloist-display.service"),
            display_enabled: service::enabled(paths, "soloist-display.service"),
            soloist_active: managed
                && !paths.is_test()
                && service::active(paths, "soloist.service"),
            soloist_enabled: managed && service::enabled(paths, "soloist.service"),
            timer_active: !paths.is_test() && service::active(paths, "soloist-display-renew.timer"),
            timer_enabled: service::enabled(paths, "soloist-display-renew.timer"),
        };
        result.persist(paths)?;
        Ok(result)
    }

    fn persist(&self, paths: &Paths) -> Result<()> {
        atomic_json(&journal_path(paths), self, Some(0o600))
    }

    pub fn stage(&mut self, paths: &Paths, original: &Path) -> Result<()> {
        if ![
            &paths.config_home,
            &paths.data_home,
            &paths.cache_home,
            &paths.bin_dir,
        ]
        .iter()
        .any(|root| original.starts_with(root))
        {
            bail!("refusing to remove unowned path {}", original.display());
        }
        match fs::symlink_metadata(original) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let mut ancestor = original.parent();
        while let Some(directory) = ancestor {
            match fs::symlink_metadata(directory) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("refusing to traverse symlinked directory {}", directory.display());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    bail!("uninstall directory disappeared: {}", directory.display());
                }
                Err(error) => return Err(error.into()),
                _ => {}
            }
            ancestor = directory.parent();
        }
        let staged = staged_name(original)?;
        if fs::symlink_metadata(&staged).is_ok() {
            bail!(
                "uninstall staging location already exists: {}",
                staged.display()
            );
        }
        self.moved.push(MovedPath {
            original: original.to_owned(),
            staged: staged.clone(),
        });
        self.persist(paths)?;
        fs::rename(original, &staged)?;
        fs::File::open(original.parent().context("missing parent")?)?.sync_all()?;
        Ok(())
    }

    pub fn commit(&mut self, paths: &Paths) -> Result<()> {
        self.phase = Phase::Committed;
        self.persist(paths)?;
        finish_cleanup(paths, self)
    }
}

fn restore_moved(tx: &UninstallTransaction) -> Result<()> {
    for moved in tx.moved.iter().rev() {
        let staged = fs::symlink_metadata(&moved.staged).is_ok();
        let original = fs::symlink_metadata(&moved.original).is_ok();
        if staged && original {
            bail!(
                "uninstall recovery found conflicting paths: {}",
                moved.original.display()
            );
        }
        if staged {
            fs::rename(&moved.staged, &moved.original)?;
            fs::File::open(moved.original.parent().context("missing parent")?)?.sync_all()?;
        } else if !original {
            bail!(
                "both original and staged path are missing: {}",
                moved.original.display()
            );
        }
    }
    Ok(())
}

fn restore_services(paths: &Paths, tx: &UninstallTransaction) -> Result<()> {
    let unit_dir = paths
        .soloist_service
        .parent()
        .context("missing systemd unit directory")?;
    if tx.managed {
        restore_file(
            &paths.soloist_service,
            tx.previous_soloist_unit.as_deref(),
            0o644,
        )?;
        restore_file(
            &paths.soloist_env,
            tx.previous_soloist_env.as_deref(),
            0o600,
        )?;
    }
    restore_file(
        &unit_dir.join("soloist-display-renew.timer"),
        tx.previous_timer_unit.as_deref(),
        0o644,
    )?;
    restore_file(
        &unit_dir.join("soloist-display-renew.service"),
        tx.previous_timer_service.as_deref(),
        0o644,
    )?;
    service::daemon_reload(paths)?;
    if tx.managed && tx.previous_soloist_unit.is_some() {
        service::restore_unit_state(
            paths,
            "soloist.service",
            tx.soloist_enabled,
            tx.soloist_active,
        )?;
    }
    if tx.previous_timer_unit.is_some() {
        service::restore_unit_state(
            paths,
            "soloist-display-renew.timer",
            tx.timer_enabled,
            tx.timer_active,
        )?;
    }
    if tx.previous_display_unit.is_some() {
        service::restore_unit_state(
            paths,
            "soloist-display.service",
            tx.display_enabled,
            tx.display_active,
        )?;
    }
    Ok(())
}

fn finish_cleanup(paths: &Paths, tx: &UninstallTransaction) -> Result<()> {
    for moved in &tx.moved {
        let staged = &moved.staged;
        if fs::symlink_metadata(staged).is_err() {
            continue;
        }
        if staged.is_symlink() || staged.is_file() {
            fs::remove_file(staged)?;
        } else if staged.is_dir() {
            fs::remove_dir_all(staged)?;
        }
    }
    if paths.state.exists() {
        fs::remove_dir_all(&paths.state)?;
    }
    for directory in [&paths.config, &paths.root] {
        if directory.exists() && fs::read_dir(directory)?.next().is_none() {
            fs::remove_dir(directory)?;
        }
    }
    fs::remove_file(journal_path(paths))?;
    Ok(())
}

pub fn recover(paths: &Paths) -> Result<()> {
    let path = journal_path(paths);
    if !path.exists() {
        return Ok(());
    }
    let tx: UninstallTransaction =
        serde_json::from_slice(&fs::read(&path)?).context("read interrupted uninstall journal")?;
    if tx.phase == Phase::Committed {
        return finish_cleanup(paths, &tx);
    }
    restore_moved(&tx)?;
    restore_services(paths, &tx)?;
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn setup() -> (tempfile::TempDir, Paths) {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", directory.path().join("root"));
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        (directory, paths)
    }

    #[test]
    #[serial]
    fn interruption_before_commit_restores_every_staged_file() {
        let (_directory, paths) = setup();
        let settings = paths.config.join("config.json");
        fs::write(&settings, b"metadata").unwrap();
        let mut tx = UninstallTransaction::begin(&paths, false).unwrap();
        tx.stage(&paths, &settings).unwrap();
        assert!(!settings.exists());
        recover(&paths).unwrap();
        assert_eq!(fs::read(&settings).unwrap(), b"metadata");
        assert!(!pending(&paths));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn interruption_after_commit_finishes_cleanup() {
        let (_directory, paths) = setup();
        let file = paths.config.join("config.json");
        fs::write(&file, b"settings").unwrap();
        let mut tx = UninstallTransaction::begin(&paths, false).unwrap();
        tx.stage(&paths, &file).unwrap();
        tx.phase = Phase::Committed;
        tx.persist(&paths).unwrap();
        recover(&paths).unwrap();
        assert!(!file.exists());
        assert!(!pending(&paths));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn rollback_preserves_original_shell_configuration_and_permissions() {
        let (_directory, paths) = setup();
        let zshrc = paths.home.join(".zshrc");
        fs::write(&zshrc, b"original shell settings").unwrap();
        fs::set_permissions(&zshrc, fs::Permissions::from_mode(0o644)).unwrap();
        let _tx = UninstallTransaction::begin(&paths, false).unwrap();
        fs::write(&zshrc, b"partial uninstall").unwrap();
        recover(&paths).unwrap();
        assert_eq!(fs::read(&zshrc).unwrap(), b"original shell settings");
        assert_eq!(
            fs::metadata(&zshrc).unwrap().permissions().mode() & 0o777,
            0o644
        );
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn rejects_owned_path_under_symlinked_ancestor() {
        let (_directory, paths) = setup();
        let foreign = paths.home.join("foreign-dir");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("settings"), b"keep this").unwrap();
        let symlinked = paths.config_home.join("via-link");
        std::os::unix::fs::symlink(&foreign, &symlinked).unwrap();
        let mut tx = UninstallTransaction::begin(&paths, false).unwrap();
        assert!(tx.stage(&paths, &symlinked.join("settings")).is_err());
        recover(&paths).unwrap();
        assert_eq!(fs::read(foreign.join("settings")).unwrap(), b"keep this");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn rejects_staging_external_soloist_data() {
        let (_directory, paths) = setup();
        let foreign = paths.home.join("soloist-session");
        fs::write(&foreign, b"session").unwrap();
        let mut tx = UninstallTransaction::begin(&paths, false).unwrap();
        assert!(tx.stage(&paths, &foreign).is_err());
        recover(&paths).unwrap();
        assert_eq!(fs::read(foreign).unwrap(), b"session");
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
