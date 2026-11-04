use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const STATE_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SoloistOwnership {
    None,
    Independent,
    Installer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoloistState {
    pub ownership: SoloistOwnership,
    pub executable: Option<PathBuf>,
    pub service_owned: bool,
    pub data_owned: bool,
    pub websocket_url: Option<String>,
    pub version: Option<String>,
}

impl Default for SoloistState {
    fn default() -> Self {
        Self {
            ownership: SoloistOwnership::None,
            executable: None,
            service_owned: false,
            data_owned: false,
            websocket_url: None,
            version: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallState {
    pub schema: u32,
    pub version: String,
    pub target: String,
    pub active_release: PathBuf,
    pub previous_release: Option<PathBuf>,
    pub installed_at: u64,
    pub soloist: SoloistState,
    pub managed_paths: Vec<PathBuf>,
    pub dependency_packages: Vec<String>,
}

impl InstallState {
    pub fn load(paths: &Paths) -> Result<Option<Self>> {
        if !paths.install_state.exists() {
            return Ok(None);
        }
        let data = fs::read(&paths.install_state).context("read installation metadata")?;
        let state: Self = serde_json::from_slice(&data).context("parse installation metadata")?;
        if state.schema > STATE_SCHEMA {
            anyhow::bail!(
                "installation metadata schema {} is newer than supported schema {}; update the manager before continuing",
                state.schema,
                STATE_SCHEMA
            );
        }
        Ok(Some(state))
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        atomic_json(&paths.install_state, self, Some(0o600))
    }
}

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
pub struct TransactionJournal {
    pub id: String,
    pub operation: String,
    pub status: TransactionStatus,
    pub started_at: u64,
    pub previous_current: Option<PathBuf>,
    pub created: Vec<PathBuf>,
    pub backups: Vec<Backup>,
    pub package_changes: Vec<String>,
    pub error: Option<String>,
}

impl TransactionJournal {
    pub fn begin(paths: &Paths, operation: &str) -> Result<(Self, PathBuf)> {
        fs::create_dir_all(&paths.transactions)?;
        let now = now();
        let id = format!("{now}-{}", std::process::id());
        let path = paths.transactions.join(format!("{id}.json"));
        let journal = Self {
            id,
            operation: operation.into(),
            status: TransactionStatus::Started,
            started_at: now,
            previous_current: None,
            created: vec![],
            backups: vec![],
            package_changes: vec![],
            error: None,
        };
        journal.persist(&path)?;
        Ok((journal, path))
    }

    pub fn persist(&self, path: &Path) -> Result<()> {
        atomic_json(path, self, Some(0o600))
    }
    pub fn set_status(&mut self, path: &Path, status: TransactionStatus) -> Result<()> {
        self.status = status;
        self.persist(path)
    }
    pub fn record_created(&mut self, journal: &Path, path: PathBuf) -> Result<()> {
        if !self.created.contains(&path) {
            self.created.push(path);
        }
        self.persist(journal)
    }
    pub fn fail(&mut self, path: &Path, error: impl Into<String>) -> Result<()> {
        self.status = TransactionStatus::Failed;
        self.error = Some(error.into());
        self.persist(path)
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn atomic_json<T: Serialize>(path: &Path, value: &T, mode: Option<u32>) -> Result<()> {
    let parent = path.parent().context("metadata path has no parent")?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        nonce
    ));
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(mode) = mode {
            options.mode(mode);
        }
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create {}", temporary.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    atomic_replace(&temporary, path).with_context(|| format!("activate {}", path.display()))?;
    #[cfg(windows)]
    if mode.is_some() {
        crate::filesystem::set_private(path)?;
    }
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACEFILE_WRITE_THROUGH,
    };
    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }
    let destination_exists = destination.exists();
    let source = wide(source.as_os_str());
    let destination = wide(destination.as_os_str());
    let success = unsafe {
        if destination_exists {
            ReplaceFileW(
                destination.as_ptr(),
                source.as_ptr(),
                ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                ptr::null(),
                ptr::null(),
            )
        } else {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if success == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_state_replaces_complete_documents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        atomic_json(&path, &serde_json::json!({"state":"before"}), Some(0o600)).unwrap();
        atomic_json(&path, &serde_json::json!({"state":"after"}), Some(0o600)).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["state"], "after");
    }
}
