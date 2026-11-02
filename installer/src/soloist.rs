use crate::{
    paths::Paths,
    process,
    release::{Artifact, ReleaseClient},
    service,
    state::{SoloistOwnership, SoloistState},
};
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use std::{
    fs,
    io::{self, IsTerminal, Write},
    os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    process::Command,
    time::Duration,
};
use tar::Archive;

const DEFAULT_WS: &str = "ws://127.0.0.1:9090";

fn websocket_healthy(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1")) {
        return false;
    }
    let Ok(stream) = std::net::TcpStream::connect_timeout(
        &"127.0.0.1:9090".parse().unwrap(),
        Duration::from_millis(600),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    tungstenite::client(url.as_str(), stream).is_ok()
}

fn executable_from_service() -> Option<PathBuf> {
    let output = Command::new("systemctl")
        .args([
            "--user",
            "show",
            "soloist.service",
            "--property=ExecStart",
            "--value",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let token = text
        .split(|character: char| character.is_whitespace() || character == '{' || character == ';')
        .find(|part| part.ends_with("/soloist"))?;
    let path = PathBuf::from(token);
    path.is_file().then_some(path)
}

pub fn discover(paths: &Paths, installed: Option<&SoloistState>) -> SoloistState {
    let websocket_url = Some(DEFAULT_WS.to_owned());
    let candidates = [
        installed.and_then(|state| state.executable.clone()),
        paths
            .managed_soloist_current
            .join("soloist")
            .is_file()
            .then(|| paths.managed_soloist_current.join("soloist")),
        executable_from_service(),
        Some(paths.home.join(".local/bin/soloist")),
        process::exists("soloist").then(|| PathBuf::from("soloist")),
    ];
    let executable = candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate == Path::new("soloist") || candidate.is_file());
    let owned = installed.is_some_and(|state| state.ownership == SoloistOwnership::Installer)
        && executable
            .as_ref()
            .is_some_and(|path| path.starts_with(&paths.managed_soloist));
    let running = websocket_healthy(DEFAULT_WS);
    let independent_service_exists =
        paths.soloist_service.exists() && !installed.is_some_and(|state| state.service_owned);
    SoloistState {
        ownership: if executable.is_none() && !running && !independent_service_exists {
            SoloistOwnership::None
        } else if owned {
            SoloistOwnership::Installer
        } else {
            SoloistOwnership::Independent
        },
        executable,
        service_owned: owned && paths.soloist_service.exists(),
        data_owned: owned,
        websocket_url,
        version: installed.and_then(|state| state.version.clone()),
    }
}

pub fn healthy(state: &SoloistState) -> bool {
    state
        .websocket_url
        .as_deref()
        .is_some_and(websocket_healthy)
}

fn prompt(label: &str, secret: bool) -> Result<String> {
    if !io::stdin().is_terminal() {
        bail!("{label} is required in non-interactive mode");
    }
    if secret {
        return rpassword::prompt_password(format!("{label}: ")).context("read API key");
    }
    eprint!("{label}: ");
    io::stderr().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_owned())
}

fn env_quote(value: &str) -> Result<String> {
    if value.contains(['\n', '\r', '\0']) {
        bail!("configuration values cannot contain line breaks or NUL bytes");
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub fn configure(paths: &Paths, name: Option<String>, api_key: Option<String>) -> Result<()> {
    if paths.soloist_env.exists() {
        return Ok(());
    }
    let name = name
        .or_else(|| std::env::var("SOLOIST_NAME").ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if io::stdin().is_terminal() {
                prompt("Spotify Connect device name", false)
                    .unwrap_or_else(|_| "Soloist Display".into())
            } else {
                "Soloist Display".into()
            }
        });
    let key = api_key
        .or_else(|| std::env::var("SOLOIST_API_KEY").ok())
        .filter(|s| !s.trim().is_empty())
        .map(Ok)
        .unwrap_or_else(|| prompt("Spotify Soloist API key", true))?;
    if key.trim().is_empty() {
        bail!("Spotify Soloist API key cannot be empty");
    }
    fs::create_dir_all(paths.soloist_env.parent().unwrap())?;
    let temporary = paths
        .soloist_env
        .with_extension(format!("tmp.{}", std::process::id()));
    fs::write(
        &temporary,
        format!(
            "SOLOIST_NAME={}\nSOLOIST_API_KEY={}\n",
            env_quote(name.trim())?,
            env_quote(key.trim())?
        ),
    )?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(temporary, &paths.soloist_env)?;
    Ok(())
}

fn safe_extract_soloist(archive_path: &Path, destination: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let mut found = false;
    for item in archive.entries()? {
        let mut entry = item?;
        let path = entry.path()?.into_owned();
        if path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!("unsafe path in Soloist archive");
        }
        if path == Path::new("soloist") {
            if !entry.header().entry_type().is_file() {
                bail!("Soloist archive executable is not a regular file");
            }
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o755)
                .open(destination)?;
            io::copy(&mut entry, &mut output)?;
            output.sync_all()?;
            found = true;
        }
    }
    if !found {
        bail!("Soloist executable is missing from its archive");
    }
    Ok(())
}

fn validate_soloist_elf(path: &Path, target: &str) -> Result<()> {
    let bytes = fs::read(path)?;
    if bytes.len() < 40 || &bytes[..4] != b"\x7fELF" || bytes[5] != 1 {
        bail!("Spotify Soloist artifact is not a supported little-endian ELF executable");
    }
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    let valid = match target {
        "x86_64" => bytes[4] == 2 && machine == 62,
        "arm64" => bytes[4] == 2 && machine == 183,
        "arm32" => {
            let flags = u32::from_le_bytes(bytes[36..40].try_into()?);
            bytes[4] == 1 && machine == 40 && flags & 0x400 != 0
        }
        _ => false,
    };
    if !valid {
        bail!("Spotify Soloist artifact does not match the detected {target} userland");
    }
    Ok(())
}

pub fn install(
    paths: &Paths,
    client: &ReleaseClient,
    artifact: &Artifact,
    version: &str,
    name: Option<String>,
    api_key: Option<String>,
) -> Result<SoloistState> {
    configure(paths, name, api_key)?;
    let version_dir = paths.managed_soloist.join(version);
    fs::create_dir_all(&version_dir)?;
    fs::create_dir_all(&paths.cache)?;
    let available = fs2::available_space(&paths.cache)?;
    if available
        < artifact
            .size
            .saturating_mul(2)
            .saturating_add(16 * 1024 * 1024)
    {
        bail!("insufficient disk space to stage and verify Spotify Soloist");
    }
    let archive_path = paths
        .cache
        .join(format!("soloist-{}.tar.gz.part", std::process::id()));
    if archive_path.exists() {
        fs::remove_file(&archive_path)?;
    }
    client.download(artifact, &archive_path)?;
    let executable = version_dir.join("soloist");
    let staged_executable = version_dir.join(format!(".soloist.{}.part", std::process::id()));
    if staged_executable.exists() {
        fs::remove_file(&staged_executable)?;
    }
    safe_extract_soloist(&archive_path, &staged_executable)?;
    validate_soloist_elf(&staged_executable, &artifact.target)?;
    fs::rename(&staged_executable, &executable)?;
    fs::remove_file(&archive_path).ok();
    let temporary = paths
        .managed_soloist
        .join(format!(".current.{}", std::process::id()));
    let relative = PathBuf::from(version);
    symlink(relative, &temporary)?;
    fs::rename(&temporary, &paths.managed_soloist_current)?;
    fs::File::open(&paths.managed_soloist)?.sync_all()?;
    service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
    Ok(SoloistState {
        ownership: SoloistOwnership::Installer,
        executable: Some(executable),
        service_owned: true,
        data_owned: true,
        websocket_url: Some(DEFAULT_WS.into()),
        version: Some(version.into()),
    })
}

pub fn start_existing(paths: &Paths, state: &SoloistState) -> Result<()> {
    if healthy(state) {
        return Ok(());
    }
    if state.ownership == SoloistOwnership::Independent {
        if !paths.is_test() {
            let _ = Command::new("systemctl")
                .args(["--user", "start", "soloist.service"])
                .status();
        }
        for _ in 0..20 {
            if healthy(state) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if !healthy(state) {
            return Err(anyhow!("Soloist is installed but its WebSocket is unavailable at 127.0.0.1:9090; start the existing installation, then run `soloist-display repair`"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn paths() -> (tempfile::TempDir, Paths) {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", directory.path().join("root"));
        let paths = Paths::discover().unwrap();
        paths.ensure_management_dirs().unwrap();
        (directory, paths)
    }
    #[test]
    fn rejects_environment_line_injection() {
        assert!(env_quote("ok\nBAD=x").is_err());
    }

    #[test]
    #[serial]
    fn distinguishes_independent_broken_and_installer_owned_soloist() {
        let (_directory, paths) = paths();
        let independent = paths.home.join(".local/bin/soloist");
        fs::create_dir_all(independent.parent().unwrap()).unwrap();
        fs::write(&independent, b"independent").unwrap();
        assert_eq!(
            discover(&paths, None).ownership,
            SoloistOwnership::Independent
        );
        fs::remove_file(independent).unwrap();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        fs::write(&paths.soloist_service, b"broken independent service").unwrap();
        assert_eq!(
            discover(&paths, None).ownership,
            SoloistOwnership::Independent
        );
        fs::remove_file(&paths.soloist_service).unwrap();
        let owned = paths.managed_soloist.join("1/soloist");
        fs::create_dir_all(owned.parent().unwrap()).unwrap();
        fs::write(&owned, b"owned").unwrap();
        let previous = SoloistState {
            ownership: SoloistOwnership::Installer,
            executable: Some(owned),
            service_owned: true,
            data_owned: true,
            websocket_url: Some(DEFAULT_WS.into()),
            version: Some("1".into()),
        };
        assert_eq!(
            discover(&paths, Some(&previous)).ownership,
            SoloistOwnership::Installer
        );
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
