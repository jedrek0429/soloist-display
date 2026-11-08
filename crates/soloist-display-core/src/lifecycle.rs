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
        path.file_name().context("metadata path has no filename")?.to_string_lossy(),
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
                    fs::rename(&backup, path).context("restore previous state after failed activation")?;
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
            bail!("staging conflicts with original path: {}", entry.original.display());
        }
        if staged {
            fs::rename(&entry.staged, &entry.original)?;
            sync_parent(entry.original.parent().context("restored path has no parent")?)?;
        } else if !original {
            bail!("both original and staged path are missing: {}", entry.original.display());
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

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, Serialize, Deserialize)]
    struct TestState { schema: u32, version: String }
    impl VersionedState for TestState {
        fn schema(&self) -> u32 { self.schema }
    }
    #[test]
    fn state_roundtrip_and_schema_guard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        atomic_json(&path, &TestState { schema: 1, version: "1.1.0".into() }, Some(0o600)).unwrap();
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
        restore_staged(&[StagedPath { original: path.clone(), staged: staged.clone() }]).unwrap();
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
        assert!(restore_staged(&[StagedPath { original: path, staged }]).is_err());
    }
    #[test]
    fn committed_cleanup_only_deletes_staging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original");
        let staged = dir.path().join(".staged");
        fs::write(&staged, b"old").unwrap();
        fs::write(&path, b"new").unwrap();
        cleanup_staged(&[StagedPath { original: path.clone(), staged: staged.clone() }]).unwrap();
        assert!(!staged.exists());
        assert_eq!(fs::read(path).unwrap(), b"new");
    }
}
