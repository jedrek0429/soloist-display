use super::{release::{self, ReleaseSpec}, Addon, AddonInstallState, AddonMetadata, AddonStatus, AddonUninstallPlan};
use crate::{
    paths::Paths,
    platform::{Architecture, Platform},
    service,
    state::{atomic_json, InstallState, SoloistOwnership},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

pub struct MprisAddon;

const VERSION: &str = "0.2.2";
const UNIT: &str = "soloist-display-mpris.service";
const BUS_NAME: &str = "org.mpris.MediaPlayer2.soloist";
const RELEASE: ReleaseSpec = ReleaseSpec {
    repository: "TobiasVeiga00/soloist-mpris",
    version: VERSION,
    binary: "soloist-mpris",
    asset: release_asset,
    max_archive_bytes: 64 * 1024 * 1024,
    max_binary_bytes: 32 * 1024 * 1024,
};
const LICENSE: &str = include_str!("../../licenses/soloist-mpris.LICENSE");

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
    Ok(Some(
        serde_json::from_slice(&content).context("read MPRIS installation state")?,
    ))
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
    let state =
        InstallState::load(paths)?.ok_or_else(|| anyhow!("Soloist Display is not installed"))?;
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
    let binary = release::download_binary(&RELEASE, &platform.architecture)?;
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
        create_file(
            &unit,
            render_unit(paths, &state.data_dir)?.as_bytes(),
            0o644,
        )?;
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
    let state = read_state(paths)?.ok_or_else(|| anyhow!("MPRIS metadata is missing"))?;
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

pub fn status(paths: &Paths) -> Result<AddonStatus> {
    let Some(state) = read_state(paths)? else {
        return Ok(AddonStatus {
            installation: AddonInstallState::NotInstalled,
            enabled: false,
            running: None,
            version: None,
            details: vec![],
        });
    };

    Ok(AddonStatus {
        installation: match state.phase {
            Phase::Pending => AddonInstallState::Pending,
            Phase::Installed => AddonInstallState::Installed,
        },
        enabled: service::enabled(paths, UNIT),
        running: Some(service::active(paths, UNIT)),
        version: Some(state.version),
        details: vec![
            ("Soloist data".into(), state.data_dir.display().to_string()),
            (
                "Executable".into(),
                binary_path(paths).display().to_string(),
            ),
        ],
    })
}

fn uninstall_plan(paths: &Paths) -> Result<Option<AddonUninstallPlan>> {
    if !is_installed(paths)? {
        return Ok(None);
    }

    Ok(Some(AddonUninstallPlan {
        files: vec![
            unit_path(paths),
            paths.root.join("addons/mpris"),
            paths.state.join("mpris.json"),
        ],
        services: vec!["soloist-display-mpris.service".into()],
    }))
}

impl Addon for MprisAddon {
    fn metadata(&self) -> AddonMetadata {
        AddonMetadata {
            id: "mpris",
            name: "MPRIS",
            description: "Media keys and playerctl integration",
        }
    }

    fn status(&self, paths: &Paths) -> Result<AddonStatus> {
        status(paths)
    }

    fn enable(&self, paths: &Paths, platform: &Platform) -> Result<()> {
        enable(paths, platform, None)
    }

    fn disable(&self, paths: &Paths) -> Result<()> {
        disable(paths)
    }

    fn remove(&self, paths: &Paths) -> Result<()> {
        remove(paths)
    }

    fn recover(&self, paths: &Paths) -> Result<()> {
        recover(paths)
    }

    fn uninstall_plan(&self, paths: &Paths) -> Result<Option<AddonUninstallPlan>> {
        uninstall_plan(paths)
    }

    fn stop_for_uninstall(&self, paths: &Paths) -> Result<()> {
        stop_for_uninstall(paths)
    }

    fn enable_command(&self) -> clap::Command {
        clap::Command::new("mpris").arg(
            clap::Arg::new("data-dir")
                .long("data-dir")
                .value_parser(clap::value_parser!(PathBuf))
                .help("Soloist data directory, needed for non-default External installations."),
        )
    }

    fn enable_with(
        &self,
        paths: &Paths,
        platform: &Platform,
        matches: &clap::ArgMatches,
    ) -> Result<()> {
        let data_dir = matches.get_one::<PathBuf>("data-dir").cloned();
        enable(paths, platform, data_dir)
    }
}

#[cfg(test)]
pub(crate) fn create_installed_test_fixture(paths: &Paths) -> Result<()> {
    let data_dir = paths.soloist_data.clone();

    fs::create_dir_all(addon_dir(paths))?;
    fs::write(binary_path(paths), b"test MPRIS binary")?;

    let unit = unit_path(paths);
    fs::create_dir_all(unit.parent().context("missing unit directory")?)?;
    fs::write(&unit, render_unit(paths, &data_dir)?)?;

    save_state(
        paths,
        &MprisState {
            version: VERSION.into(),
            data_dir,
            phase: Phase::Installed,
        },
    )
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
        assert!(unit.contains(
            "ExecStart=\"/home/a b/.local/share/soloist-display/addons/mpris/soloist-mpris\""
        ));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!unit.contains("PartOf=soloist.service"));
        assert!(!unit.contains("Wants=soloist.service"));
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
            create_file(&binary_path(&paths), b"#!/bin/sh\nexec sleep 60\n", 0o755)?;
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

}
