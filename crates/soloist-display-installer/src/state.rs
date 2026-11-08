use crate::{paths::Paths, release::ReleaseChannel};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
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
    #[serde(default)]
    pub channel: Option<ReleaseChannel>,
    #[serde(default)]
    pub suppress_updates: bool,
    pub target: String,
    pub active_release: PathBuf,
    pub previous_release: Option<PathBuf>,
    pub installed_at: u64,
    pub soloist: SoloistState,
    pub managed_paths: Vec<PathBuf>,
    pub dependency_packages: Vec<String>,
}

impl soloist_display_core::lifecycle::VersionedState for InstallState {
    fn schema(&self) -> u32 {
        self.schema
    }
}

impl InstallState {
    pub fn effective_channel(&self) -> ReleaseChannel {
        self.channel.unwrap_or_else(|| {
            semver::Version::parse(&self.version)
                .map(|version| ReleaseChannel::for_version(&version))
                .unwrap_or(ReleaseChannel::Stable)
        })
    }

    pub fn load(paths: &Paths) -> Result<Option<Self>> {
        soloist_display_core::lifecycle::load_state(&paths.install_state, STATE_SCHEMA)
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
pub struct InstallRecovery {
    pub previous_settings: Option<Vec<u8>>,
    pub previous_config_schema: Option<Vec<u8>>,
    pub previous_metadata: Option<Vec<u8>>,
    pub previous_env: Option<Vec<u8>>,
    pub previous_runtime: Option<Vec<u8>>,
    pub previous_display_unit: Option<Vec<u8>>,
    pub previous_soloist_unit: Option<Vec<u8>>,
    pub previous_renewal_service: Option<Vec<u8>>,
    pub previous_renewal_timer: Option<Vec<u8>>,
    pub previous_soloist_link: Option<PathBuf>,
    pub previous_display_active: bool,
    pub previous_display_enabled: bool,
    pub previous_soloist_active: bool,
    pub previous_soloist_enabled: bool,
    pub previous_timer_active: bool,
    pub previous_timer_enabled: bool,
    pub restore_existing_service: bool,
    pub unit_backup_existed: bool,
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
    #[serde(default)]
    pub install_recovery: Option<InstallRecovery>,
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
            install_recovery: None,
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
    soloist_display_core::lifecycle::atomic_json(path, value, mode)
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
