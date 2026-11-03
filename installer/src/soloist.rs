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

pub fn validate_websocket_url(raw: &str) -> Result<String> {
    let url = url::Url::parse(raw).context("invalid Soloist WebSocket URL")?;
    if url.scheme() != "ws"
        || !matches!(
            url.host_str(),
            Some("127.0.0.1" | "localhost" | "::1" | "[::1]")
        )
        || url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("external Soloist WebSocket must use ws:// on localhost");
    }
    if url.port_or_known_default().is_none() {
        bail!("external Soloist WebSocket requires a port");
    }
    Ok(url.to_string())
}

fn websocket_healthy(endpoint: &str) -> bool {
    if validate_websocket_url(endpoint).is_err() {
        return false;
    }
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    let address = if matches!(url.host_str(), Some("::1" | "[::1]")) {
        std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))
    } else {
        std::net::SocketAddr::from(([127, 0, 0, 1], port))
    };
    let Ok(stream) = std::net::TcpStream::connect_timeout(
        &address,
        Duration::from_millis(600),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(900)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(900)));
    tungstenite::client(url.as_str(), stream).is_ok()
}

pub fn verify_external(
    paths: &Paths,
    discovered: &SoloistState,
    endpoint: Option<&str>,
) -> Result<SoloistState> {
    let url = validate_websocket_url(endpoint.unwrap_or(DEFAULT_WS))?;
    let mut state = discovered.clone();
    state.ownership = SoloistOwnership::Independent;
    state.service_owned = false;
    state.data_owned = false;
    state.websocket_url = Some(url.clone());

    let unit_present = service::soloist_unit_present(paths);
    if !healthy(&state) && unit_present {
        service::start_soloist_existing(paths)?;
        for _ in 0..20 {
            if healthy(&state) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    if !healthy(&state) {
        bail!(
            "external Soloist WebSocket at {url} is unavailable; fix the existing instance or rerun installation without --external-soloist to use managed Soloist"
        );
    }
    if state.executable.is_none() && unit_present {
        state.executable = executable_from_service();
    }
    if state.executable.is_none() {
        eprintln!("  External Soloist executable could not be located; WebSocket handshake succeeded.");
    }
    if !unit_present {
        eprintln!("  External Soloist: no systemd unit detected; service startup and binary renewal remain your responsibility.");
    } else {
        eprintln!("  External Soloist: systemd unit detected; automatic binary renewal is disabled.");
    }
    Ok(state)
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
    let data_owned = !paths.soloist_data.exists();
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
        data_owned,
        websocket_url: Some(DEFAULT_WS.into()),
        version: Some(version.into()),
    })
}

/// Ensure the Soloist CLI can discover the fixed loopback endpoint used by Display.
/// Never replace an existing endpoint: independent Soloist installations may
/// have their own configuration, which the installer must not take over.
pub fn ensure_cli_discovery(paths: &Paths, state: &SoloistState) -> Result<()> {
    if paths.is_test()
        || state.ownership != SoloistOwnership::Installer
        || !healthy(state)
    {
        return Ok(());
    }
    let Some(url) = state.websocket_url.as_deref() else {
        return Ok(());
    };
    let endpoint = url::Url::parse(url)?;
    let Some(host) = endpoint.host_str() else {
        return Ok(());
    };
    let Some(port) = endpoint.port_or_known_default() else {
        return Ok(());
    };
    // The managed service explicitly uses this directory. For independent
    // services, only use the standard CLI data directory if no custom -D
    // directory is configured; do not guess or overwrite custom paths.
    let data = &paths.soloist_data;
    fs::create_dir_all(data)?;
    for (filename, value) in [("ws.addr", host.to_owned()), ("ws.port", port.to_string())] {
        let path = data.join(filename);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(error) = writeln!(file, "{value}") {
                    let _ = fs::remove_file(&path);
                    return Err(error.into());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn start_existing(paths: &Paths, state: &SoloistState) -> Result<()> {
    if healthy(state) {
        return Ok(());
    }
    if state.ownership == SoloistOwnership::Installer {
        service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
        service::restart_soloist(paths)?;
        for _ in 0..20 {
            if healthy(state) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        bail!("managed Soloist did not start or expose its WebSocket");
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
    #[serial]
    fn verifies_external_websocket_without_requiring_systemd() {
        let (_directory, paths) = paths();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let _socket = tungstenite::accept(stream).unwrap();
            }
        });
        let state = verify_external(&paths, &SoloistState::default(), Some(&url)).unwrap();
        assert_eq!(state.ownership, SoloistOwnership::Independent);
        assert_eq!(state.websocket_url.as_deref(), Some(url.as_str()));
        assert!(!state.service_owned);
        assert!(!state.data_owned);
        assert!(!paths.soloist_service.exists());
        worker.join().unwrap();
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    fn validates_custom_external_websocket_endpoints() {
        assert!(validate_websocket_url("ws://127.0.0.1:9190").is_ok());
        assert!(validate_websocket_url("ws://localhost:9090").is_ok());
        assert!(validate_websocket_url("ws://[::1]:9090").is_ok());
        assert!(validate_websocket_url("ws://192.168.1.1:9090").is_err());
        assert!(validate_websocket_url("wss://example.com").is_err());
        assert!(validate_websocket_url("ws://alice:password@localhost:9090").is_err());
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
