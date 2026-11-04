use crate::{
    paths::Paths,
    platform::{Architecture, Platform},
    service,
    state::{atomic_json, InstallState, SoloistOwnership},
};
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};
use tar::Archive;

const VERSION: &str = "0.2.2";
const UPSTREAM: &str = "https://github.com/TobiasVeiga00/soloist-mpris";
const UNIT: &str = "soloist-display-mpris.service";
const BUS_NAME: &str = "org.mpris.MediaPlayer2.soloist";
const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
const MAX_BINARY: u64 = 32 * 1024 * 1024;
const LICENSE: &str = include_str!("../licenses/soloist-mpris.LICENSE");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Pending,
    Installed,
}

#[derive(Debug, Serialize, Deserialize)]
struct MprisState {
    version: String,
    data_dir: PathBuf,
    phase: Phase,
}

pub fn unit_path(paths: &Paths) -> PathBuf {
    paths.config_home.join("systemd/user").join(UNIT)
}

fn addon_dir(paths: &Paths) -> PathBuf {
    paths.root.join("addons/mpris")
}

fn binary_path(paths: &Paths) -> PathBuf {
    addon_dir(paths).join("soloist-mpris")
}

fn state_path(paths: &Paths) -> PathBuf {
    paths.state.join("mpris.json")
}

fn read_state(paths: &Paths) -> Result<Option<MprisState>> {
    let content = match fs::read(state_path(paths)) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(serde_json::from_slice(&content).context("read MPRIS installation state")?))
}

fn save_state(paths: &Paths, state: &MprisState) -> Result<()> {
    atomic_json(&state_path(paths), state, Some(0o600))
}

pub fn is_installed(paths: &Paths) -> Result<bool> {
    Ok(read_state(paths)?.is_some_and(|state| state.phase == Phase::Installed))
}

fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

fn render_unit(paths: &Paths, data_dir: &Path) -> Result<String> {
    let data = data_dir
        .to_str()
        .context("MPRIS data directory must be valid UTF-8")?;
    if !data_dir.is_absolute() || data.contains(['\n', '\r', '\0']) {
        bail!("MPRIS data directory must be an absolute path without control characters");
    }
    let executable = binary_path(paths);
    let binary = executable
        .to_str()
        .context("MPRIS executable path must be valid UTF-8")?;
    Ok(format!(
        "[Unit]\nDescription=Soloist Display MPRIS bridge\nAfter=soloist.service\n\n[Service]\nType=simple\nEnvironment={}\nExecStart={}\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        quote(&format!("SOLOIST_DATA_DIR={data}")),
        quote(binary)
    ))
}

fn systemctl(paths: &Paths, arguments: &[&str]) -> Result<()> {
    if paths.is_test() {
        return Ok(());
    }
    let output = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .output()
        .context("run systemctl --user")?;
    if !output.status.success() {
        bail!(
            "systemctl --user {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn loaded(unit: &str) -> Result<bool> {
    let result = Command::new("systemctl")
        .args(["--user", "show", unit, "--property=LoadState", "--value"])
        .output()
        .context("inspect existing user services")?;
    if !result.status.success() {
        bail!("could not inspect user service {unit}");
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim() == "loaded")
}

fn name_taken() -> Result<bool> {
    let output = Command::new("busctl")
        .args(["--user", "--list", "--no-pager", "--no-legend"])
        .output()
        .context("busctl is required to check the session D-Bus")?;
    if !output.status.success() {
        bail!("session D-Bus is unavailable; log in to a user session");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.split_whitespace().next() == Some(BUS_NAME)))
}

fn check_conflicts(paths: &Paths) -> Result<()> {
    if loaded("soloist-mpris.service")? || name_taken()? {
        bail!("an MPRIS bridge already exists; leave it managed by its current installer");
    }
    if unit_path(paths).exists() || addon_dir(paths).exists() {
        bail!("unrecognised MPRIS files are present; refusing to overwrite them");
    }
    Ok(())
}

fn resolve_data_dir(paths: &Paths, requested: Option<PathBuf>) -> Result<PathBuf> {
    let state = InstallState::load(paths)?
        .ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
    let dir = match state.soloist.ownership {
        SoloistOwnership::Installer => {
            if requested.is_some() {
                bail!("Managed Soloist uses its configured data directory; omit --data-dir");
            }
            service::ManagedRuntime::load(paths)?.data_dir
        }
        SoloistOwnership::Independent => requested.unwrap_or_else(|| paths.soloist_data.clone()),
        SoloistOwnership::None => bail!("Soloist is not configured"),
    };
    if !dir.is_absolute() {
        bail!("Soloist data directory must be an absolute path");
    }
    if !paths.is_test() && (!dir.join("ws.addr").is_file() || !dir.join("ws.port").is_file()) {
        bail!(
            "Soloist WebSocket address files were not found in {}; start Soloist or pass --data-dir for External installations",
            dir.display()
        );
    }
    Ok(dir)
}

fn release_asset(architecture: &Architecture) -> Result<&'static str> {
    match architecture {
        Architecture::X86_64 => Ok("soloist-mpris-x86_64-linux.tar.gz"),
        Architecture::Aarch64 => Ok("soloist-mpris-aarch64-linux.tar.gz"),
        Architecture::Armv7 => bail!("soloist-mpris does not publish an ARMv7 binary"),
    }
}

fn get(client: &reqwest::blocking::Client, url: &str, maximum: u64) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("download {url}"))?
        .error_for_status()
        .with_context(|| format!("download {url}"))?;
    let mut body = Vec::new();
    response.take(maximum + 1).read_to_end(&mut body)?;
    if body.len() as u64 > maximum {
        bail!("MPRIS release asset exceeds its size limit");
    }
    Ok(body)
}

fn verify_checksum(archive: &[u8], content: &str, name: &str) -> Result<()> {
    let mut fields = content.split_whitespace();
    let digest = fields.next().context("missing MPRIS checksum")?;
    let filename = fields.next().context("missing MPRIS checksum filename")?;
    if fields.next().is_some()
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || filename.trim_start_matches('*') != name
    {
        bail!("invalid upstream MPRIS checksum file");
    }
    let actual = hex::encode(Sha256::digest(archive));
    if !actual.eq_ignore_ascii_case(digest) {
        bail!("MPRIS release checksum mismatch");
    }
    Ok(())
}

fn unpack_binary(archive: &[u8], architecture: &Architecture) -> Result<Vec<u8>> {
    let decoder = GzDecoder::new(archive);
    let mut tar = Archive::new(decoder);
    let mut binary = None;
    for entry in tar.entries().context("read upstream MPRIS archive")? {
        let mut entry = entry?;
        if entry.path()?.as_ref() != Path::new("soloist-mpris") {
            continue;
        }
        if !entry.header().entry_type().is_file() || binary.is_some() {
            bail!("invalid or duplicate MPRIS executable in archive");
        }
        if entry.size() > MAX_BINARY {
            bail!("MPRIS executable exceeds its size limit");
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        verify_binary(&bytes, architecture)?;
        binary = Some(bytes);
    }
    binary.context("MPRIS archive does not contain soloist-mpris")
}

fn verify_binary(binary: &[u8], architecture: &Architecture) -> Result<()> {
    if binary.len() < 40 || &binary[..4] != b"\x7fELF" || binary[5] != 1 {
        bail!("upstream MPRIS executable is not a little-endian ELF");
    }
    let machine = u16::from_le_bytes([binary[18], binary[19]]);
    let expected = match architecture {
        Architecture::X86_64 => (2, 62),
        Architecture::Aarch64 => (2, 183),
        Architecture::Armv7 => (1, 40),
    };
    if (binary[4], machine) != expected {
        bail!("upstream MPRIS executable has the wrong architecture");
    }
    Ok(())
}

fn download_binary(architecture: &Architecture) -> Result<Vec<u8>> {
    let asset = release_asset(architecture)?;
    let root = format!("{UPSTREAM}/releases/download/v{VERSION}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(40))
        .build()?;
    let archive = get(&client, &format!("{root}/{asset}"), MAX_ARCHIVE)?;
    let checksum = get(&client, &format!("{root}/{asset}.sha256"), 4096)?;
    verify_checksum(
        &archive,
        std::str::from_utf8(&checksum).context("invalid checksum encoding")?,
        asset,
    )?;
    unpack_binary(&archive, architecture)
}

fn create_file(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn activate(paths: &Paths) -> Result<()> {
    systemctl(paths, &["daemon-reload"])?;
    systemctl(paths, &["enable", "--now", UNIT])?;
    if !paths.is_test() {
        for _ in 0..20 {
            if service::active(paths, UNIT) && name_taken()? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(250));
        }
        bail!("MPRIS bridge did not register with D-Bus; inspect journalctl --user -u soloist-display-mpris");
    }
    Ok(())
}

fn check_ownership(paths: &Paths, data_dir: &Path) -> Result<()> {
    let expected = render_unit(paths, data_dir)?;
    if fs::read_to_string(unit_path(paths)).ok().as_deref() != Some(expected.as_str()) {
        bail!("MPRIS unit was changed outside Soloist Display; refusing to manage it");
    }
    Ok(())
}

fn rollback_pending(paths: &Paths) -> Result<()> {
    systemctl(paths, &["disable", "--now", UNIT])?;
    if unit_path(paths).exists() {
        fs::remove_file(unit_path(paths))?;
    }
    if addon_dir(paths).exists() {
        fs::remove_dir_all(addon_dir(paths))?;
    }
    if state_path(paths).exists() {
        fs::remove_file(state_path(paths))?;
    }
    service::daemon_reload(paths)?;
    Ok(())
}

pub fn recover(paths: &Paths) -> Result<()> {
    if read_state(paths)?.is_some_and(|state| state.phase == Phase::Pending) {
        rollback_pending(paths).context("recover interrupted MPRIS installation")?;
    }
    Ok(())
}

pub fn enable(paths: &Paths, platform: &Platform, requested: Option<PathBuf>) -> Result<()> {
    if !service::user_manager_available(paths) {
        bail!("MPRIS requires a working systemd user session");
    }
    if let Some(installed) = read_state(paths)? {
        if installed.phase == Phase::Pending {
            bail!("MPRIS setup is incomplete; run repair before retrying");
        }
        if requested.is_some() && requested.as_deref() != Some(installed.data_dir.as_path()) {
            bail!("MPRIS is already installed with a different data directory; remove it first");
        }
        check_ownership(paths, &installed.data_dir)?;
        if !binary_path(paths).is_file() {
            bail!("MPRIS binary is missing; remove and reinstall the addon");
        }
        if !paths.is_test() && !service::active(paths, UNIT) && name_taken()? {
            bail!("another player already owns the Soloist MPRIS name");
        }
        activate(paths)?;
        println!("MPRIS bridge enabled.");
        return Ok(());
    }
    let data_dir = resolve_data_dir(paths, requested)?;
    check_conflicts(paths)?;
    let binary = download_binary(&platform.architecture)?;
    let state = MprisState {
        version: VERSION.into(),
        data_dir,
        phase: Phase::Pending,
    };
    save_state(paths, &state)?;
    let result = (|| {
        fs::create_dir_all(addon_dir(paths))?;
        create_file(&binary_path(paths), &binary, 0o755)?;
        create_file(&addon_dir(paths).join("LICENSE"), LICENSE.as_bytes(), 0o644)?;
        let unit = unit_path(paths);
        fs::create_dir_all(unit.parent().context("missing systemd unit directory")?)?;
        create_file(&unit, render_unit(paths, &state.data_dir)?.as_bytes(), 0o644)?;
        activate(paths)?;
        save_state(
            paths,
            &MprisState {
                phase: Phase::Installed,
                ..state
            },
        )
    })();
    if let Err(error) = result {
        rollback_pending(paths).context("MPRIS install failed and rollback failed")?;
        return Err(error.context("MPRIS installation failed; changes were rolled back"));
    }
    println!("MPRIS bridge enabled (soloist-mpris v{VERSION}).");
    Ok(())
}

pub fn stop_for_uninstall(paths: &Paths) -> Result<()> {
    let state = read_state(paths)?
        .ok_or_else(|| anyhow!("MPRIS metadata is missing"))?;
    check_ownership(paths, &state.data_dir)?;
    systemctl(paths, &["disable", "--now", UNIT])
}

pub fn disable(paths: &Paths) -> Result<()> {
    let Some(state) = read_state(paths)? else {
        println!("MPRIS bridge is not installed.");
        return Ok(());
    };
    check_ownership(paths, &state.data_dir)?;
    systemctl(paths, &["disable", "--now", UNIT])?;
    println!("MPRIS bridge disabled; Spotify Soloist is unchanged.");
    Ok(())
}

pub fn remove(paths: &Paths) -> Result<()> {
    let Some(state) = read_state(paths)? else {
        println!("MPRIS bridge is not installed.");
        return Ok(());
    };
    check_ownership(paths, &state.data_dir)?;
    systemctl(paths, &["disable", "--now", UNIT])?;
    fs::remove_file(unit_path(paths))?;
    if addon_dir(paths).exists() {
        fs::remove_dir_all(addon_dir(paths))?;
    }
    fs::remove_file(state_path(paths))?;
    service::daemon_reload(paths)?;
    println!("MPRIS bridge removed; Spotify Soloist is unchanged.");
    Ok(())
}

pub fn status(paths: &Paths) -> Result<()> {
    match read_state(paths)? {
        None => println!("MPRIS bridge: not installed"),
        Some(state) => {
            println!("MPRIS bridge: {}", if state.phase == Phase::Pending { "incomplete" } else { "installed" });
            println!("Upstream version: {}", state.version);
            println!("Soloist data: {}", state.data_dir.display());
            println!("Service enabled: {}", service::enabled(paths, UNIT));
            println!("Service active: {}", service::active(paths, UNIT));
            println!("Executable: {}", binary_path(paths).display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_does_not_take_over_soloist() {
        let paths = Paths {
            home: PathBuf::from("/home/a b"),
            data_home: PathBuf::from("/home/a b/.local/share"),
            config_home: PathBuf::from("/home/a b/.config"),
            state_home: PathBuf::new(),
            cache_home: PathBuf::new(),
            root: PathBuf::from("/home/a b/.local/share/soloist-display"),
            config: PathBuf::new(),
            state: PathBuf::new(),
            cache: PathBuf::new(),
            releases: PathBuf::new(),
            current: PathBuf::new(),
            transactions: PathBuf::new(),
            logs: PathBuf::new(),
            lock: PathBuf::new(),
            install_state: PathBuf::new(),
            bin_dir: PathBuf::new(),
            visible_command: PathBuf::new(),
            display_service: PathBuf::new(),
            soloist_service: PathBuf::new(),
            applications: PathBuf::new(),
            soloist_env: PathBuf::new(),
            managed_soloist: PathBuf::new(),
            managed_soloist_current: PathBuf::new(),
            soloist_data: PathBuf::new(),
            soloist_cache: PathBuf::new(),
        };
        let unit = render_unit(&paths, Path::new("/home/a b/100%/soloist")).unwrap();
        assert!(unit.contains("Environment=\"SOLOIST_DATA_DIR=/home/a b/100%%/soloist\""));
        assert!(unit.contains("ExecStart=\"/home/a b/.local/share/soloist-display/addons/mpris/soloist-mpris\""));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!unit.contains("PartOf=soloist.service"));
        assert!(!unit.contains("Wants=soloist.service"));
    }

    #[test]
    fn checksum_is_exact_and_filename_bound() {
        let archive = b"archive";
        let digest = hex::encode(Sha256::digest(archive));
        assert!(verify_checksum(archive, &format!("{digest}  bridge.tar.gz"), "bridge.tar.gz").is_ok());
        assert!(verify_checksum(archive, &format!("{digest}  other.tar.gz"), "bridge.tar.gz").is_err());
        assert!(verify_checksum(b"wrong", &format!("{digest}  bridge.tar.gz"), "bridge.tar.gz").is_err());
    }

    #[test]
    fn armv7_requires_upstream_support() {
        assert!(release_asset(&Architecture::Armv7).is_err());
        assert!(release_asset(&Architecture::X86_64).is_ok());
        assert!(release_asset(&Architecture::Aarch64).is_ok());
    }


    #[test]
    #[serial_test::serial]
    fn pending_install_recovers_owned_files() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("isolated"));
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        fs::create_dir_all(addon_dir(&paths)).unwrap();
        fs::write(binary_path(&paths), b"partial").unwrap();
        fs::create_dir_all(unit_path(&paths).parent().unwrap()).unwrap();
        fs::write(unit_path(&paths), b"partial").unwrap();
        save_state(
            &paths,
            &MprisState {
                version: VERSION.into(),
                data_dir: paths.soloist_data.clone(),
                phase: Phase::Pending,
            },
        )
        .unwrap();
        recover(&paths).unwrap();
        assert!(!state_path(&paths).exists());
        assert!(!addon_dir(&paths).exists());
        assert!(!unit_path(&paths).exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial_test::serial]
    fn live_mpris_unit_can_start_independently() {
        if std::env::var("SOLOIST_DISPLAY_RUN_SYSTEMD_TEST").as_deref() != Ok("1") {
            return;
        }
        let paths = Paths::discover().unwrap();
        assert!(!paths.is_test());
        assert!(service::user_manager_available(&paths));
        assert!(!unit_path(&paths).exists());
        assert!(!addon_dir(&paths).exists());
        let result = (|| -> Result<()> {
            fs::create_dir_all(addon_dir(&paths))?;
            create_file(
                &binary_path(&paths),
                b"#!/bin/sh\nexec sleep 60\n",
                0o755,
            )?;
            let unit = unit_path(&paths);
            fs::create_dir_all(unit.parent().context("no unit directory")?)?;
            create_file(
                &unit,
                render_unit(&paths, Path::new("/tmp/mock-soloist-data"))?.as_bytes(),
                0o644,
            )?;
            systemctl(&paths, &["daemon-reload"])?;
            systemctl(&paths, &["enable", "--now", UNIT])?;
            if !service::active(&paths, UNIT) {
                bail!("stand-in MPRIS service did not start");
            }
            Ok(())
        })();
        let stopped = systemctl(&paths, &["disable", "--now", UNIT]);
        if unit_path(&paths).exists() {
            fs::remove_file(unit_path(&paths)).unwrap();
        }
        if addon_dir(&paths).exists() {
            fs::remove_dir_all(addon_dir(&paths)).unwrap();
        }
        service::daemon_reload(&paths).unwrap();
        stopped.unwrap();
        result.unwrap();
    }

    #[test]
    fn binary_rejects_wrong_architecture() {
        let mut binary = vec![0; 40];
        binary[..4].copy_from_slice(b"\x7fELF");
        binary[4] = 2;
        binary[5] = 1;
        binary[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert!(verify_binary(&binary, &Architecture::X86_64).is_ok());
        assert!(verify_binary(&binary, &Architecture::Aarch64).is_err());
    }
}
