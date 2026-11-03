use crate::{
    paths::Paths,
    platform::Platform,
    service,
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
    process::Command,
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
        .build()?;
    let url = format!("{CDN}/{}", archive_name(platform));
    let head = client.head(&url).send()?.error_for_status()?;
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
        let response = client.get(&url).send()?.error_for_status()?;
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
        let version = Command::new(&executable)
            .arg("--version")
            .output()
            .context("validate Soloist executable")?;
        if !version.status.success()
            || !String::from_utf8_lossy(&version.stdout)
                .to_lowercase()
                .contains("soloist")
        {
            bail!("downloaded Soloist failed version check");
        }
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
        if !final_dir.exists() {
            fs::create_dir(&final_dir)?;
            fs::rename(&executable, final_dir.join("soloist"))?;
        }
        let previous = fs::read_link(&paths.managed_soloist_current).ok();
        let tmp_link = paths
            .managed_soloist
            .join(format!(".current-renew-{}", std::process::id()));
        symlink(&final_dir, &tmp_link)?;
        fs::rename(&tmp_link, &paths.managed_soloist_current)?;
        let updated =
            service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))
                .and_then(|_| service::restart_soloist(paths));
        if let Err(error) = updated {
            if let Some(previous) = previous {
                let rollback = paths
                    .managed_soloist
                    .join(format!(".rollback-{}", std::process::id()));
                symlink(previous, &rollback)?;
                fs::rename(rollback, &paths.managed_soloist_current)?;
                let _ =
                    service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"));
            }
            return Err(error.context("renewal failed; restored previous Soloist"));
        }
        state.soloist.executable = Some(final_dir.join("soloist"));
        state.soloist.version = Some(format!("build-{}", &digest[..16]));
        state.save(paths)?;
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
        Ok(())
    })();
    fs::remove_dir_all(stage).ok();
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
    #[test]
    fn archive_mapping() {
        assert_eq!(MAX_ARCHIVE, 160 * 1024 * 1024);
    }
}
