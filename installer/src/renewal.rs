use crate::{
    paths::Paths,
    platform::Platform,
    service, soloist,
    state::{atomic_json, InstallState, SoloistOwnership},
};
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt},
    path::{Component, Path},
    time::Duration,
};

const CDN: &str = "https://soloist-builds.spotifycdn.com";
const MAX_ARCHIVE: u64 = 160 * 1024 * 1024;
const MAX_EXECUTABLE: u64 = 320 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct BuildRecord {
    etag: Option<String>,
    last_modified: Option<String>,
    content_length: Option<u64>,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
struct RenewalPending {
    previous_link: std::path::PathBuf,
    previous_state: InstallState,
    previous_record: Option<Vec<u8>>,
    #[serde(default)]
    new_build: Option<std::path::PathBuf>,
    #[serde(default)]
    committed: bool,
}

fn remove_staged_build(paths: &Paths, path: &Path) -> Result<()> {
    if path.parent() != Some(paths.managed_soloist.as_path())
        || !path.file_name().and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("build-"))
    {
        bail!("refusing to clean up a build outside managed Soloist");
    }
    if fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        bail!("refusing to follow a staged build symlink");
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
        fs::File::open(&paths.managed_soloist)?.sync_all()?;
    }
    Ok(())
}

fn cleanup_obsolete_builds(paths: &Paths, active: &Path, previous: &Path) -> Result<()> {
    let previous = if previous.is_absolute() {
        previous.to_path_buf()
    } else {
        paths.managed_soloist.join(previous)
    };
    for entry in fs::read_dir(&paths.managed_soloist)? {
        let entry = entry?;
        let path = entry.path();
        if path == active || path == previous {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("build-"))
            && entry.file_type()?.is_dir()
        {
            remove_staged_build(paths, &path)?;
        }
    }
    Ok(())
}

fn ensure_official_response(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    if !response.status().is_success() {
        bail!("Spotify Soloist CDN returned HTTP {}", response.status());
    }
    if response.url().scheme() != "https" || response.url().host_str() != Some("soloist-builds.spotifycdn.com") {
        bail!("unexpected Soloist archive origin");
    }
    Ok(response)
}

fn pending_path(paths: &Paths) -> std::path::PathBuf {
    paths.state.join("soloist-renew-pending.json")
}

pub fn recover(paths: &Paths) -> Result<()> {
    let path = pending_path(paths);
    if !path.exists() {
        return Ok(());
    }
    let pending: RenewalPending = serde_json::from_slice(&fs::read(&path)?)?;
    if pending.committed {
        fs::remove_file(&path)?;
        fs::File::open(&paths.state)?.sync_all()?;
        return Ok(());
    }
    if pending.previous_state.soloist.ownership != SoloistOwnership::Installer {
        bail!("refusing to restore a renewal not owned by Soloist Display");
    }
    let rollback = paths
        .managed_soloist
        .join(format!(".rollback-{}", std::process::id()));
    symlink(&pending.previous_link, &rollback)?;
    fs::rename(&rollback, &paths.managed_soloist_current)?;
    pending.previous_state.save(paths)?;
    let record_path = paths.state.join("soloist-build.json");
    match &pending.previous_record {
        Some(bytes) => {
            let record: BuildRecord = serde_json::from_slice(bytes)?;
            atomic_json(&record_path, &record, Some(0o600))?;
        }
        None if record_path.exists() => fs::remove_file(&record_path)?,
        None => {}
    }
    service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
    service::restart_soloist(paths)?;
    if !paths.is_test() {
        let mut healthy = false;
        for _ in 0..24 {
            if soloist::healthy(&pending.previous_state.soloist) {
                healthy = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if !healthy {
            bail!("previous Soloist is not healthy; renewal recovery remains pending");
        }
    }
    if let Some(new_build) = pending.new_build.as_deref() {
        remove_staged_build(paths, new_build)?;
    }
    fs::remove_file(path)?;
    fs::File::open(&paths.state)?.sync_all()?;
    Ok(())
}

fn archive_name(platform: &Platform) -> &'static str {
    match platform.soloist_archive() {
        "x86_64" => "soloist_release_x86_64.tar.gz",
        "arm64" => "soloist_release_arm64.tar.gz",
        _ => "soloist_release_arm32.tar.gz",
    }
}

fn valid_elf(path: &Path, target: &str) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut header = [0u8; 40];
    file.read_exact(&mut header)?;
    if &header[..4] != b"\x7fELF" || header[5] != 1 {
        bail!("invalid Soloist ELF");
    }
    let machine = u16::from_le_bytes([header[18], header[19]]);
    let valid = match target {
        "x86_64" => header[4] == 2 && machine == 62,
        "arm64" => header[4] == 2 && machine == 183,
        "arm32" => {
            header[4] == 1
                && machine == 40
                && u32::from_le_bytes(header[36..40].try_into()?) & 0x400 != 0
        }
        _ => false,
    };
    if !valid {
        bail!("Soloist architecture mismatch");
    }
    Ok(())
}

fn extract(archive_path: &Path, destination: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut found = false;
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!("unsafe archive path");
        }
        if path.as_ref() != Path::new("soloist") {
            continue;
        }
        if found || !entry.header().entry_type().is_file() || entry.size() > MAX_EXECUTABLE {
            bail!("invalid Soloist archive entry");
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(destination)?;
        let copied = io::copy(&mut entry.take(MAX_EXECUTABLE + 1), &mut output)?;
        if copied > MAX_EXECUTABLE {
            bail!("Soloist executable too large");
        }
        output.sync_all()?;
        found = true;
    }
    if !found {
        bail!("Soloist executable missing");
    }
    Ok(())
}

pub fn renew(paths: &Paths, platform: &Platform, force: bool) -> Result<()> {
    let mut state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    if state.soloist.ownership != SoloistOwnership::Installer {
        bail!("Soloist is independently installed; refusing to replace it");
    }
    let record_path = paths.state.join("soloist-build.json");
    let old = fs::read(&record_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BuildRecord>(&bytes).ok());
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let url = format!("{CDN}/{}", archive_name(platform));
    let head = ensure_official_response(client.head(&url).send()?)?;
    let etag = head
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let last_modified = head
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let length = head.content_length();
    if length.is_some_and(|n| n > MAX_ARCHIVE) {
        bail!("Soloist archive exceeds size limit");
    }
    if !force
        && old.as_ref().is_some_and(|record| {
            (etag.is_some() && etag == record.etag)
                || (etag.is_none()
                    && last_modified.is_some()
                    && last_modified == record.last_modified
                    && length == record.content_length)
        })
        && (paths.is_test() || soloist::healthy(&state.soloist))
        && state
            .soloist
            .executable
            .as_ref()
            .is_some_and(|p| p.is_file())
    {
        return Ok(());
    }
    fs::create_dir_all(&paths.cache)?;
    fs::create_dir_all(&paths.managed_soloist)?;
    let stage = paths
        .managed_soloist
        .join(format!(".renew-{}", std::process::id()));
    fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        let archive_path = stage.join("soloist.tar.gz");
        let response = ensure_official_response(client.get(&url).send()?)?;
        if response.content_length().is_some_and(|n| n > MAX_ARCHIVE) {
            bail!("Soloist archive exceeds size limit");
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&archive_path)?;
        let size = io::copy(&mut response.take(MAX_ARCHIVE + 1), &mut output)?;
        if size > MAX_ARCHIVE {
            bail!("Soloist archive exceeds size limit");
        }
        output.sync_all()?;
        let executable = stage.join("soloist");
        extract(&archive_path, &executable)?;
        valid_elf(&executable, platform.soloist_archive())?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let mut hasher = Sha256::new();
        io::copy(
            &mut fs::File::open(&executable)?,
            &mut hasher_writer(&mut hasher),
        )?;
        let digest = hex::encode(hasher.finalize());
        if old.as_ref().is_some_and(|record| record.sha256 == digest) && !force {
            return Ok(());
        }
        let final_dir = paths
            .managed_soloist
            .join(format!("build-{}", &digest[..16]));
        let fresh_build = !final_dir.exists();
        if !fresh_build {
            let installed = final_dir.join("soloist");
            if !installed.is_file() || crate::release::sha256_file(&installed)? != digest {
                bail!("existing managed Soloist build has a checksum mismatch");
            }
        }
        let previous = fs::read_link(&paths.managed_soloist_current)
            .context("managed Soloist has no previous version to restore")?;
        let previous_state = state.clone();
        let previous_record = fs::read(&record_path).ok();
        atomic_json(
            &pending_path(paths),
            &RenewalPending {
                previous_link: previous.clone(),
                previous_state: previous_state.clone(),
                previous_record: previous_record.clone(),
                new_build: fresh_build.then(|| final_dir.clone()),
                committed: false,
            },
            Some(0o600),
        )?;
        if fresh_build {
            fs::create_dir(&final_dir)?;
            fs::rename(&executable, final_dir.join("soloist"))?;
            fs::File::open(&final_dir)?.sync_all()?;
            fs::File::open(&paths.managed_soloist)?.sync_all()?;
        }
        let tmp_link = paths
            .managed_soloist
            .join(format!(".current-renew-{}", std::process::id()));
        symlink(&final_dir, &tmp_link)?;
        fs::rename(&tmp_link, &paths.managed_soloist_current)?;
        let activated = (|| -> Result<()> {
            service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
            service::restart_soloist(paths)?;
            if !paths.is_test() {
                let mut ready = false;
                for _ in 0..24 {
                    if soloist::healthy(&state.soloist) {
                        ready = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                if !ready {
                    bail!("renewed Soloist did not expose a working WebSocket");
                }
            }
            state.soloist.executable = Some(final_dir.join("soloist"));
            state.soloist.version = Some(format!("build-{}", &digest[..16]));
            atomic_json(
                &record_path,
                &BuildRecord {
                    etag,
                    last_modified,
                    content_length: length,
                    sha256: digest,
                },
                Some(0o600),
            )?;
            state.save(paths)?;
            Ok(())
        })();
        if let Err(error) = activated {
            recover(paths).context("renewal failed and previous Soloist could not be restored")?;
            return Err(error.context("renewal failed; previous Soloist restored"));
        }
        atomic_json(
            &pending_path(paths),
            &RenewalPending {
                previous_link: previous.clone(),
                previous_state,
                previous_record,
                new_build: fresh_build.then(|| final_dir.clone()),
                committed: true,
            },
            Some(0o600),
        )?;
        fs::remove_file(pending_path(paths))?;
        fs::File::open(&paths.state)?.sync_all()?;
        if let Err(error) = cleanup_obsolete_builds(paths, &final_dir, &previous) {
            eprintln!("warning: could not remove obsolete Soloist builds: {error:#}");
        }
        Ok(())
    })();
    fs::remove_dir_all(stage).ok();
    if result.is_err() && pending_path(paths).exists() {
        recover(paths).context("recover interrupted Soloist renewal")?;
    }
    result
}

struct HashWriter<'a>(&'a mut Sha256);
impl io::Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn hasher_writer(hasher: &mut Sha256) -> HashWriter<'_> {
    HashWriter(hasher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{now, SoloistState, STATE_SCHEMA};
    use serial_test::serial;

    #[test]
    #[serial]
    fn interrupted_renewal_restores_previous_binary_and_metadata() {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", directory.path().join("root"));
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        let previous_dir = paths.managed_soloist.join("previous");
        let current_dir = paths.managed_soloist.join("build-replacement");
        fs::create_dir_all(&previous_dir).unwrap();
        fs::create_dir_all(&current_dir).unwrap();
        fs::write(previous_dir.join("soloist"), b"previous").unwrap();
        fs::write(current_dir.join("soloist"), b"replacement").unwrap();
        symlink(&current_dir, &paths.managed_soloist_current).unwrap();
        let state = InstallState {
            schema: STATE_SCHEMA,
            version: "1.0.0".into(),
            channel: None,
            suppress_updates: false,
            target: "test".into(),
            active_release: paths.releases.join("1.0.0"),
            previous_release: None,
            installed_at: now(),
            soloist: SoloistState {
                ownership: SoloistOwnership::Installer,
                executable: Some(previous_dir.join("soloist")),
                service_owned: true,
                data_owned: false,
                websocket_url: Some("ws://127.0.0.1:9090/".into()),
                version: Some("previous".into()),
            },
            managed_paths: vec![],
            dependency_packages: vec![],
        };
        state.save(&paths).unwrap();
        atomic_json(
            &pending_path(&paths),
            &RenewalPending {
                previous_link: previous_dir.clone(),
                previous_state: state,
                previous_record: None,
                new_build: Some(current_dir.clone()),
                committed: false,
            },
            Some(0o600),
        )
        .unwrap();
        recover(&paths).unwrap();
        assert_eq!(
            fs::read_link(&paths.managed_soloist_current).unwrap(),
            previous_dir
        );
        assert!(!pending_path(&paths).exists());
        assert!(!current_dir.exists());
        assert!(InstallState::load(&paths).unwrap().is_some());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn finished_renewal_is_not_rolled_back_by_stale_journal() {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", directory.path().join("root"));
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        let previous_dir = paths.managed_soloist.join("build-previous");
        let current_dir = paths.managed_soloist.join("build-current");
        fs::create_dir_all(&previous_dir).unwrap();
        fs::create_dir_all(&current_dir).unwrap();
        fs::write(current_dir.join("soloist"), b"current").unwrap();
        symlink(&current_dir, &paths.managed_soloist_current).unwrap();
        let state = InstallState {
            schema: STATE_SCHEMA,
            version: "1.0.0".into(),
            channel: None,
            suppress_updates: false,
            target: "test".into(),
            active_release: paths.releases.join("1.0.0"),
            previous_release: None,
            installed_at: now(),
            soloist: SoloistState {
                ownership: SoloistOwnership::Installer,
                executable: Some(current_dir.join("soloist")),
                service_owned: true,
                data_owned: false,
                websocket_url: Some("ws://127.0.0.1:9090/".into()),
                version: Some("current".into()),
            },
            managed_paths: vec![],
            dependency_packages: vec![],
        };
        state.save(&paths).unwrap();
        atomic_json(
            &pending_path(&paths),
            &RenewalPending {
                previous_link: previous_dir,
                previous_state: state,
                previous_record: None,
                new_build: Some(current_dir.clone()),
                committed: true,
            },
            Some(0o600),
        )
        .unwrap();
        recover(&paths).unwrap();
        assert!(current_dir.exists());
        assert_eq!(fs::read_link(&paths.managed_soloist_current).unwrap(), current_dir);
        assert!(!pending_path(&paths).exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    fn archive_mapping() {
        assert_eq!(MAX_ARCHIVE, 160 * 1024 * 1024);
    }
}
