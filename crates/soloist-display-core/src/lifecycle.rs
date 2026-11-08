//! Durable state and filesystem transaction primitives shared across platforms.
//! Platform integrations and ownership checks remain with their callers.
use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub trait VersionedState {
    fn schema(&self) -> u32;
}

pub fn load_state<T: DeserializeOwned + VersionedState>(
    path: &Path,
    supported_schema: u32,
) -> Result<Option<T>> {
    recover_atomic(path)?;
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let state: T = serde_json::from_slice(&data)
        .with_context(|| format!("parse installation metadata {}", path.display()))?;
    if state.schema() > supported_schema {
        bail!(
            "installation metadata schema {} is newer than supported schema {}",
            state.schema(),
            supported_schema
        );
    }
    Ok(Some(state))
}

pub fn atomic_json<T: Serialize>(path: &Path, value: &T, mode: Option<u32>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes, mode)
}

/// Persist a new value without truncating the previous one in place.
/// On Windows, the backup is recoverable if execution stops between renames.
pub fn atomic_write(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<()> {
    let parent = path.parent().context("metadata path has no parent")?;
    fs::create_dir_all(parent)?;
    recover_atomic(path)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}.{}.pending",
        path.file_name()
            .context("metadata path has no filename")?
            .to_string_lossy(),
        std::process::id(),
        nonce
    ));
    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode.unwrap_or(0o600));
        }
        #[cfg(not(unix))]
        let _ = mode;
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;

        #[cfg(unix)]
        fs::rename(&temporary, path)?;
        #[cfg(not(unix))]
        {
            let backup = backup_path(path);
            let existed = fs::symlink_metadata(path).is_ok();
            if existed {
                fs::rename(path, &backup)?;
            }
            if let Err(error) = fs::rename(&temporary, path) {
                if existed {
                    fs::rename(&backup, path)
                        .context("restore previous state after failed activation")?;
                }
                return Err(error.into());
            }
            if existed {
                fs::remove_file(backup)?;
            }
        }
        sync_parent(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(unix))]
fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("display-atomic-backup")
}

pub fn recover_atomic(path: &Path) -> Result<()> {
    #[cfg(not(unix))]
    {
        let backup = backup_path(path);
        if backup.exists() {
            if path.exists() {
                fs::remove_file(&backup).context("remove committed metadata backup")?;
            } else {
                fs::rename(&backup, path).context("restore interrupted metadata replacement")?;
            }
        }
    }
    #[cfg(unix)]
    let _ = path;
    Ok(())
}

fn sync_parent(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

/// Common journal representation for Linux-managed and desktop lifecycle operations.
/// The platform-specific snapshot is supplied as the generic recovery payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatus {
    Started,
    Activating,
    Services,
    HealthCheck,
    Committed,
    RollingBack,
    RolledBack,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Backup {
    pub original: PathBuf,
    pub backup: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionJournal<S> {
    pub id: String,
    pub operation: String,
    pub status: TransactionStatus,
    pub started_at: u64,
    pub previous_current: Option<PathBuf>,
    pub created: Vec<PathBuf>,
    pub backups: Vec<Backup>,
    pub package_changes: Vec<String>,
    #[serde(default)]
    pub install_recovery: Option<S>,
    pub error: Option<String>,
}

impl<S: Serialize> TransactionJournal<S> {
    fn fresh(operation: &str, install_recovery: Option<S>) -> Self {
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            id: format!("{started_at}-{}", std::process::id()),
            operation: operation.to_owned(),
            status: TransactionStatus::Started,
            started_at,
            previous_current: None,
            created: Vec::new(),
            backups: Vec::new(),
            package_changes: Vec::new(),
            install_recovery,
            error: None,
        }
    }

    pub fn begin(directory: &Path, operation: &str) -> Result<(Self, PathBuf)> {
        fs::create_dir_all(directory)?;
        let tx = Self::fresh(operation, None);
        let path = directory.join(format!("{}.json", tx.id));
        if path.exists() {
            bail!("transaction journal already exists: {}", path.display());
        }
        tx.persist(&path)?;
        Ok((tx, path))
    }

    pub fn begin_at(path: &Path, operation: &str, recovery: Option<S>) -> Result<Self> {
        if path.exists() {
            bail!("incomplete transaction must be recovered before retrying");
        }
        let tx = Self::fresh(operation, recovery);
        tx.persist(path)?;
        Ok(tx)
    }

    pub fn persist(&self, path: &Path) -> Result<()> {
        atomic_json(path, self, Some(0o600))
    }

    pub fn set_status(&mut self, path: &Path, status: TransactionStatus) -> Result<()> {
        self.status = status;
        self.persist(path)
    }

    pub fn record_created(&mut self, path: &Path, created: PathBuf) -> Result<()> {
        if !self.created.contains(&created) {
            self.created.push(created);
        }
        self.persist(path)
    }

    pub fn fail(&mut self, path: &Path, error: impl Into<String>) -> Result<()> {
        self.status = TransactionStatus::Failed;
        self.error = Some(error.into());
        self.persist(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransactionPhase {
    #[default]
    Preparing,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedPath {
    pub original: PathBuf,
    pub staged: PathBuf,
}

/// The caller must first verify that the path is installer-owned, and persist
/// the StagedPath in a journal before calling this function.
pub fn rename_staged(original: &Path, staged: &Path) -> Result<()> {
    if fs::symlink_metadata(staged).is_ok() {
        bail!("staging destination already exists: {}", staged.display());
    }
    fs::rename(original, staged)
        .with_context(|| format!("stage owned path {}", original.display()))?;
    sync_parent(original.parent().context("staged path has no parent")?)
}

pub fn restore_staged(paths: &[StagedPath]) -> Result<()> {
    for entry in paths.iter().rev() {
        let staged = fs::symlink_metadata(&entry.staged).is_ok();
        let original = fs::symlink_metadata(&entry.original).is_ok();
        if staged && original {
            bail!(
                "staging conflicts with original path: {}",
                entry.original.display()
            );
        }
        if staged {
            fs::rename(&entry.staged, &entry.original)?;
            sync_parent(
                entry
                    .original
                    .parent()
                    .context("restored path has no parent")?,
            )?;
        } else if !original {
            bail!(
                "both original and staged path are missing: {}",
                entry.original.display()
            );
        }
    }
    Ok(())
}

pub fn cleanup_staged(paths: &[StagedPath]) -> Result<()> {
    for entry in paths {
        let metadata = match fs::symlink_metadata(&entry.staged) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.is_dir() {
            fs::remove_dir_all(&entry.staged)?;
        } else {
            fs::remove_file(&entry.staged)?;
        }
    }
    Ok(())
}

/// Journal snapshot used to recover an interrupted executable activation.
/// Captured bytes are restricted to installer-owned metadata and launchers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationSnapshot {
    pub schema: u32,
    pub metadata: Option<Vec<u8>>,
    pub launcher: Option<Vec<u8>>,
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn snapshot_activation(metadata: &Path, launcher: &Path) -> Result<ActivationSnapshot> {
    Ok(ActivationSnapshot {
        schema: 1,
        metadata: read_optional(metadata)?,
        launcher: read_optional(launcher)?,
    })
}

fn restore_optional(path: &Path, previous: Option<&[u8]>, mode: Option<u32>) -> Result<()> {
    if let Some(bytes) = previous {
        atomic_write(path, bytes, mode)
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Callers must verify ownership of an existing launcher before restoration.
/// Restoring is idempotent so crashes during rollback can be recovered again.
pub fn restore_activation(
    snapshot: &ActivationSnapshot,
    metadata: &Path,
    launcher: &Path,
    launcher_mode: Option<u32>,
) -> Result<()> {
    if snapshot.schema != 1 {
        bail!("unsupported activation journal schema {}", snapshot.schema);
    }
    restore_optional(launcher, snapshot.launcher.as_deref(), launcher_mode)?;
    restore_optional(metadata, snapshot.metadata.as_deref(), Some(0o600))
}

#[cfg(test)]
mod tests {
    #[test]
    fn activation_snapshot_rolls_back_both_state_and_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = dir.path().join("install.json");
        let launcher = dir.path().join("display.cmd");
        fs::write(&metadata, b"previous metadata").unwrap();
        fs::write(&launcher, b"previous launcher").unwrap();
        let snapshot = snapshot_activation(&metadata, &launcher).unwrap();
        atomic_write(&metadata, b"new metadata", Some(0o600)).unwrap();
        atomic_write(&launcher, b"new launcher", Some(0o700)).unwrap();
        restore_activation(&snapshot, &metadata, &launcher, Some(0o700)).unwrap();
        assert_eq!(fs::read(&metadata).unwrap(), b"previous metadata");
        assert_eq!(fs::read(&launcher).unwrap(), b"previous launcher");
        restore_activation(&snapshot, &metadata, &launcher, Some(0o700)).unwrap();
    }

    use super::*;
    #[derive(Debug, Serialize, Deserialize)]
    struct TestState {
        schema: u32,
        version: String,
    }
    impl VersionedState for TestState {
        fn schema(&self) -> u32 {
            self.schema
        }
    }
    #[test]
    fn state_roundtrip_and_schema_guard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        atomic_json(
            &path,
            &TestState {
                schema: 1,
                version: "1.1.0".into(),
            },
            Some(0o600),
        )
        .unwrap();
        let state: TestState = load_state(&path, 1).unwrap().unwrap();
        assert_eq!(state.version, "1.1.0");
        assert!(load_state::<TestState>(&path, 0).is_err());
    }
    #[test]
    fn interrupted_stage_restores_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original");
        let staged = dir.path().join(".staged");
        fs::write(&path, b"old").unwrap();
        rename_staged(&path, &staged).unwrap();
        restore_staged(&[StagedPath {
            original: path.clone(),
            staged: staged.clone(),
        }])
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), b"old");
        assert!(!staged.exists());
    }
    #[test]
    fn refusing_to_overwrite_new_original_during_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original");
        let staged = dir.path().join(".staged");
        fs::write(&path, b"old").unwrap();
        rename_staged(&path, &staged).unwrap();
        fs::write(&path, b"new").unwrap();
        assert!(restore_staged(&[StagedPath {
            original: path,
            staged
        }])
        .is_err());
    }
    #[test]
    fn committed_cleanup_only_deletes_staging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original");
        let staged = dir.path().join(".staged");
        fs::write(&staged, b"old").unwrap();
        fs::write(&path, b"new").unwrap();
        cleanup_staged(&[StagedPath {
            original: path.clone(),
            staged: staged.clone(),
        }])
        .unwrap();
        assert!(!staged.exists());
        assert_eq!(fs::read(path).unwrap(), b"new");
    }
}
