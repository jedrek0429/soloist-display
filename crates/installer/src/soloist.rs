use crate::{
    paths::Paths,
    platform::Platform,
    process, renewal, service,
    state::{SoloistOwnership, SoloistState},
};
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use soloist_display_core::{classify, EngineMode};
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
    let Ok(stream) = std::net::TcpStream::connect_timeout(&address, Duration::from_millis(600))
    else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(900)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(900)));
    tungstenite::client(url.as_str(), stream).is_ok()
}

fn protocol_healthy(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    let Some(port) = url.port() else {
        return false;
    };
    let addr = if matches!(url.host_str(), Some("::1" | "[::1]")) {
        std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))
    } else {
        std::net::SocketAddr::from(([127, 0, 0, 1], port))
    };
    let Ok(stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(700)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let Ok((mut ws, _)) = tungstenite::client(url.as_str(), stream) else {
        return false;
    };
    if ws
        .send(tungstenite::Message::Text(
            r#"{"type":"command","command":"get_auth_state"}"#.into(),
        ))
        .is_err()
    {
        return false;
    }
    for _ in 0..4 {
        let Ok(message) = ws.read() else {
            break;
        };
        if let tungstenite::Message::Text(text) = message {
            if serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .is_some_and(|value| {
                    value.as_object().is_some_and(|obj| {
                        obj.get("type").and_then(serde_json::Value::as_str) == Some("auth_state")
                    })
                })
            {
                return true;
            }
        }
    }
    false
}

pub fn external_healthy(state: &SoloistState) -> bool {
    state.websocket_url.as_deref().is_some_and(protocol_healthy)
}

pub fn verify_external(
    paths: &Paths,
    discovered: &SoloistState,
    endpoint: Option<&str>,
    allow_start: bool,
) -> Result<SoloistState> {
    let url = validate_websocket_url(endpoint.unwrap_or(DEFAULT_WS))?;
    let mut state = discovered.clone();
    state.ownership = SoloistOwnership::Independent;
    state.service_owned = false;
    state.data_owned = false;
    state.websocket_url = Some(url.clone());

    let unit_present = service::soloist_unit_present(paths);
    let was_active = unit_present && service::active(paths, "soloist.service");
    let mut started = false;
    if !protocol_healthy(&url) && unit_present && !was_active {
        let consent = if allow_start {
            true
        } else if io::stdin().is_terminal() {
            eprint!("External Soloist is stopped. Start its existing service? [y/N]: ");
            io::stderr().flush()?;
            let mut reply = String::new();
            io::stdin().read_line(&mut reply)?;
            matches!(reply.trim().to_ascii_lowercase().as_str(), "y" | "yes")
        } else {
            false
        };
        if consent {
            started = true;
            if let Err(error) = service::start_soloist_existing(paths) {
                service::stop_soloist_existing(paths).ok();
                return Err(error.context("start external Soloist"));
            }
            for _ in 0..20 {
                if protocol_healthy(&url) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    if !protocol_healthy(&url) {
        if started {
            service::stop_soloist_existing(paths).ok();
        }
        bail!(
            "external Soloist at {url} did not respond to get_state; check its protocol and service, or use managed Soloist"
        );
    }
    if state.executable.is_none() && unit_present {
        state.executable = executable_from_service();
    }
    if state.executable.is_none() {
        eprintln!(
            "  External Soloist executable could not be located; WebSocket handshake succeeded."
        );
    }
    if !unit_present {
        eprintln!("  External Soloist: no systemd unit detected; service startup and binary renewal remain your responsibility.");
    } else {
        eprintln!(
            "  External Soloist: systemd unit detected; automatic binary renewal is disabled."
        );
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
    let websocket_url = installed
        .and_then(|state| state.websocket_url.clone())
        .or_else(|| Some(DEFAULT_WS.to_owned()));
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
    let running = websocket_url.as_deref().is_some_and(websocket_healthy);
    let independent_service_exists =
        paths.soloist_service.exists() && !installed.is_some_and(|state| state.service_owned);
    let mode = classify(
        installed.is_some_and(|state| state.ownership == SoloistOwnership::Installer),
        executable.as_deref(),
        &paths.managed_soloist,
        running,
        independent_service_exists,
    );
    let owned = mode == EngineMode::Managed;
    SoloistState {
        ownership: match mode {
            EngineMode::Missing => SoloistOwnership::None,
            EngineMode::External => SoloistOwnership::Independent,
            EngineMode::Managed => SoloistOwnership::Installer,
        },
        executable,
        service_owned: owned && paths.soloist_service.exists(),
        data_owned: owned && installed.is_some_and(|state| state.data_owned),
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
        eprintln!("  Get your key: https://developer.spotify.com/dashboard/soloist");
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

fn read_credentials(path: &Path) -> Result<(Option<String>, Option<String>)> {
    if !path.exists() {
        return Ok((None, None));
    }
    let mut name = None;
    let mut key = None;
    for line in fs::read_to_string(path)?.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (variable, raw) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid existing Soloist environment file"))?;
        if variable != "SOLOIST_NAME" && variable != "SOLOIST_API_KEY" {
            continue;
        }
        let raw = raw.trim();
        let value = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw)
                .context("invalid quoted Soloist environment value")?
        } else {
            raw.to_owned()
        };
        if variable == "SOLOIST_NAME" {
            name = Some(value);
        } else {
            key = Some(value);
        }
    }
    Ok((name, key))
}

pub fn configure(paths: &Paths, name: Option<String>, api_key: Option<String>) -> Result<()> {
    let (previous_name, previous_key) = read_credentials(&paths.soloist_env)?;
    if name.is_none()
        && api_key.is_none()
        && previous_name
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        && previous_key
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    let name = [name, previous_name, std::env::var("SOLOIST_NAME").ok()]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            if io::stdin().is_terminal() {
                prompt("Spotify Connect device name", false)
                    .unwrap_or_else(|_| "Soloist Display".into())
            } else {
                "Soloist Display".into()
            }
        });
    let key = [api_key, previous_key, std::env::var("SOLOIST_API_KEY").ok()]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
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

pub fn managed_installation_complete(paths: &Paths) -> Result<bool> {
    let (name, key) = read_credentials(&paths.soloist_env)?;
    Ok(paths.soloist_service.is_file()
        && name
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && key.as_deref().is_some_and(|value| !value.trim().is_empty())
        && service::active(paths, "soloist.service"))
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

const MAX_SESSION_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SESSION_ENTRIES: usize = 10000;

fn copy_session_tree(
    source: &Path,
    destination: &Path,
    depth: usize,
    count: &mut usize,
    total: &mut u64,
) -> Result<()> {
    if depth > 32 {
        bail!("existing Soloist data directory is too deeply nested");
    }
    fs::create_dir(destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    for item in fs::read_dir(source)? {
        let entry = item?;
        *count += 1;
        if *count > MAX_SESSION_ENTRIES {
            bail!("existing Soloist session has too many files");
        }
        let name = entry.file_name();
        if depth == 0 && matches!(name.to_str(), Some("soloist.pid" | "ws.addr" | "ws.port")) {
            continue;
        }
        let from = entry.path();
        let to = destination.join(name);
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("existing Soloist data contains a symlink; cannot copy it safely");
        }
        if kind.is_dir() {
            copy_session_tree(&from, &to, depth + 1, count, total)?;
        } else if kind.is_file() {
            let size = entry.metadata()?.len();
            *total = total
                .checked_add(size)
                .ok_or_else(|| anyhow!("Soloist session exceeds size limit"))?;
            if *total > MAX_SESSION_BYTES {
                bail!("existing Soloist session exceeds 512 MiB; use external mode");
            }
            fs::copy(&from, &to)?;
            fs::set_permissions(&to, fs::Permissions::from_mode(0o600))?;
            fs::File::open(&to)?.sync_all()?;
        } else {
            bail!("existing Soloist data contains an unsupported file type");
        }
    }
    fs::File::open(destination)?.sync_all()?;
    Ok(())
}

pub fn copy_existing_session(paths: &Paths, source: &Path) -> Result<PathBuf> {
    let destination = paths.managed_soloist.join("session");
    if source.starts_with(&paths.managed_soloist) || destination.exists() {
        bail!("managed Soloist session directory is not available for migration");
    }
    let metadata = fs::symlink_metadata(source).with_context(|| {
        format!(
            "read existing Soloist session directory {}",
            source.display()
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("existing Soloist session path is not a regular directory");
    }
    fs::create_dir_all(&paths.managed_soloist)?;
    let (mut count, mut total) = (0, 0);
    if let Err(error) = copy_session_tree(source, &destination, 0, &mut count, &mut total) {
        fs::remove_dir_all(&destination).ok();
        return Err(error.context("copy existing Soloist session"));
    }
    fs::File::open(&paths.managed_soloist)?.sync_all()?;
    Ok(destination)
}

pub fn install(
    paths: &Paths,
    platform: &Platform,
    version: &str,
    name: Option<String>,
    api_key: Option<String>,
) -> Result<SoloistState> {
    configure(paths, name, api_key)?;
    let runtime = service::ManagedRuntime::load(paths)?;
    if !paths.state.join("soloist-runtime.json").exists() {
        runtime.save(paths)?;
    }
    let data_owned =
        runtime.data_dir.starts_with(&paths.managed_soloist) || !runtime.data_dir.exists();
    let version_dir = paths.managed_soloist.join(version);
    fs::create_dir_all(&version_dir)?;
    fs::create_dir_all(&paths.cache)?;
    let available = fs2::available_space(&paths.cache)?;
    if available < 2 * 160 * 1024 * 1024 + 16 * 1024 * 1024 {
        bail!("insufficient disk space to stage and verify Spotify Soloist");
    }
    let archive_path = paths
        .cache
        .join(format!("soloist-{}.tar.gz.part", std::process::id()));
    if archive_path.exists() {
        fs::remove_file(&archive_path)?;
    }
    renewal::download_current_archive(paths, platform, &archive_path)?;
    let executable = version_dir.join("soloist");
    let staged_executable = version_dir.join(format!(".soloist.{}.part", std::process::id()));
    if staged_executable.exists() {
        fs::remove_file(&staged_executable)?;
    }
    safe_extract_soloist(&archive_path, &staged_executable)?;
    validate_soloist_elf(&staged_executable, platform.soloist_archive())?;
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
        websocket_url: Some(runtime.websocket_url),
        version: Some(version.into()),
    })
}

pub fn ensure_cli_discovery(paths: &Paths, state: &SoloistState) -> Result<()> {
    if paths.is_test() || state.ownership != SoloistOwnership::Installer || !healthy(state) {
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
    let runtime = service::ManagedRuntime::load(paths)?;
    let data = &runtime.data_dir;
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
    if state.ownership == SoloistOwnership::Installer {
        if managed_installation_complete(paths)? && healthy(state) {
            return Ok(());
        }
        service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
        for _ in 0..20 {
            if healthy(state) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        bail!("managed Soloist did not start or expose its WebSocket");
    }
    if healthy(state) {
        return Ok(());
    }
    if state.ownership == SoloistOwnership::Independent {
        if !paths.is_test() {
            service::start_soloist_existing(paths)?;
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
    fn managed_session_copy_cannot_modify_previous_session() {
        let (_directory, paths) = paths();
        let original = paths.home.join("existing-data");
        fs::create_dir_all(&original).unwrap();
        fs::write(original.join("credentials"), b"existing login").unwrap();
        fs::write(original.join("soloist.pid"), b"old pid").unwrap();
        let managed = copy_existing_session(&paths, &original).unwrap();
        assert_eq!(
            fs::read(managed.join("credentials")).unwrap(),
            b"existing login"
        );
        assert!(!managed.join("soloist.pid").exists());
        fs::write(managed.join("credentials"), b"new login").unwrap();
        assert_eq!(
            fs::read(original.join("credentials")).unwrap(),
            b"existing login"
        );
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn session_copy_rejects_symlinks_without_touching_source() {
        let (_directory, paths) = paths();
        let original = paths.home.join("existing-data");
        fs::create_dir_all(&original).unwrap();
        symlink("/etc/passwd", original.join("external")).unwrap();
        assert!(copy_existing_session(&paths, &original).is_err());
        assert!(original.join("external").is_symlink());
        assert!(!paths.managed_soloist.join("session").exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
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
                let mut socket = tungstenite::accept(stream).unwrap();
                let _ = socket.read().unwrap();
                socket
                    .send(tungstenite::Message::Text(
                        r#"{"type":"auth_state","logged_in":false}"#.into(),
                    ))
                    .unwrap();
            }
        });
        let state = verify_external(&paths, &SoloistState::default(), Some(&url), false).unwrap();
        assert_eq!(state.ownership, SoloistOwnership::Independent);
        assert_eq!(
            state.websocket_url.as_deref(),
            Some(format!("{url}/").as_str())
        );
        assert!(!state.service_owned);
        assert!(!state.data_owned);
        assert!(!paths.soloist_service.exists());
        worker.join().unwrap();
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn rejects_websocket_servers_without_soloist_protocol() {
        let (_directory, paths) = paths();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                socket.read().unwrap();
                socket
                    .send(tungstenite::Message::Text(
                        r#"{"message":"unrelated websocket"}"#.into(),
                    ))
                    .unwrap();
            }
        });
        assert!(verify_external(&paths, &SoloistState::default(), Some(&url), false).is_err());
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
    #[serial]
    fn rediscovery_preserves_external_endpoint_and_does_not_claim_existing_data() {
        let (_directory, paths) = paths();
        let independent = paths.home.join(".local/bin/soloist");
        fs::create_dir_all(independent.parent().unwrap()).unwrap();
        fs::write(&independent, b"independent").unwrap();
        let previous = SoloistState {
            ownership: SoloistOwnership::Independent,
            executable: Some(independent),
            service_owned: false,
            data_owned: false,
            websocket_url: Some("ws://127.0.0.1:9190/".into()),
            version: None,
        };
        let state = discover(&paths, Some(&previous));
        assert_eq!(state.ownership, SoloistOwnership::Independent);
        assert_eq!(state.websocket_url, previous.websocket_url);
        assert!(!state.data_owned);
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn missing_managed_credentials_require_recovery_even_with_a_service() {
        let (_directory, paths) = paths();
        fs::create_dir_all(paths.soloist_service.parent().unwrap()).unwrap();
        fs::write(&paths.soloist_service, "[Service]\n").unwrap();
        assert!(!managed_installation_complete(&paths).unwrap());

        fs::create_dir_all(paths.soloist_env.parent().unwrap()).unwrap();
        fs::write(
            &paths.soloist_env,
            "SOLOIST_NAME=\"Speaker\"\nSOLOIST_API_KEY=\"token\"\n",
        )
        .unwrap();
        assert!(managed_installation_complete(&paths).unwrap());

        fs::write(
            &paths.soloist_env,
            "SOLOIST_NAME=\"Speaker\"\nSOLOIST_API_KEY=\"\"\n",
        )
        .unwrap();
        assert!(!managed_installation_complete(&paths).unwrap());

        fs::remove_file(&paths.soloist_service).unwrap();
        assert!(!managed_installation_complete(&paths).unwrap());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn empty_legacy_credentials_do_not_mask_environment_key() {
        let (_directory, paths) = paths();
        std::env::set_var("SOLOIST_API_KEY", "environment-key");
        let result = configure(&paths, Some(String::new()), Some(String::new()));
        std::env::remove_var("SOLOIST_API_KEY");
        result.unwrap();
        let (_, key) = read_credentials(&paths.soloist_env).unwrap();
        assert_eq!(key.as_deref(), Some("environment-key"));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
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
