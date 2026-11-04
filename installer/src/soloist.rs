use crate::{
    engine_policy::{classify, EngineMode},
    filesystem,
    paths::Paths,
    platform::{OperatingSystem, Platform},
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
    path::{Component, Path},
    process::Command,
    time::Duration,
};
use tar::Archive;
use zip::ZipArchive;

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

pub fn discover(paths: &Paths, installed: Option<&SoloistState>) -> SoloistState {
    let websocket_url = Some(DEFAULT_WS.to_owned());
    let executable_name = if cfg!(windows) {
        "soloist.exe"
    } else {
        "soloist"
    };
    let candidates = [
        installed.and_then(|state| state.executable.clone()),
        paths
            .managed_soloist_current
            .join(executable_name)
            .is_file()
            .then(|| paths.managed_soloist_current.join(executable_name)),
        service::external_soloist_executable(),
        Some(paths.home.join(".local/bin").join(executable_name)),
        Some(paths.home.join("bin").join(executable_name)),
        process::find(executable_name),
    ];
    let executable = candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.is_file());
    let running = websocket_healthy(DEFAULT_WS);
    let independent_service_exists = service::independent_soloist_service_exists(paths)
        && !installed.is_some_and(|state| state.service_owned);
    let mode = classify(
        installed.is_some_and(|state| state.ownership == SoloistOwnership::Installer),
        executable.as_deref(),
        &paths.managed_soloist,
        running,
        independent_service_exists,
    );
    SoloistState {
        ownership: match mode {
            EngineMode::Missing => SoloistOwnership::None,
            EngineMode::External => SoloistOwnership::Independent,
            EngineMode::Managed => SoloistOwnership::Installer,
        },
        executable,
        service_owned: mode.may_remove_engine() && paths.soloist_service.exists(),
        data_owned: mode.may_remove_engine(),
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
    filesystem::set_private(&temporary)?;
    fs::rename(temporary, &paths.soloist_env)?;
    Ok(())
}

fn safe_extract_soloist(archive_path: &Path, destination: &Path, windows: bool) -> Result<()> {
    if windows {
        let file = fs::File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut entry = archive
            .by_name("soloist.exe")
            .context("Soloist executable is missing from its archive")?;
        if entry.is_dir() || entry.enclosed_name().as_deref() != Some(Path::new("soloist.exe")) {
            bail!("unsafe Soloist Windows archive layout");
        }
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)?;
        io::copy(&mut entry, &mut output)?;
        output.sync_all()?;
        return Ok(());
    }
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
            let mut options = fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o755);
            }
            let mut output = options.open(destination)?;
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

fn validate_soloist_binary(path: &Path, target: &str) -> Result<()> {
    let bytes = fs::read(path)?;
    let valid = match target {
        "linux-x86_64" => {
            bytes.len() >= 40
                && &bytes[..4] == b"\x7fELF"
                && bytes[4] == 2
                && u16::from_le_bytes([bytes[18], bytes[19]]) == 62
        }
        "linux-arm64" => {
            bytes.len() >= 40
                && &bytes[..4] == b"\x7fELF"
                && bytes[4] == 2
                && u16::from_le_bytes([bytes[18], bytes[19]]) == 183
        }
        "linux-arm32" if bytes.len() >= 40 && &bytes[..4] == b"\x7fELF" => {
            let flags = u32::from_le_bytes(bytes[36..40].try_into()?);
            bytes[4] == 1 && u16::from_le_bytes([bytes[18], bytes[19]]) == 40 && flags & 0x400 != 0
        }
        "windows-x86_64" if bytes.len() >= 0x40 && &bytes[..2] == b"MZ" => {
            let offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into()?) as usize;
            bytes.get(offset..offset + 6).is_some_and(|header| {
                &header[..4] == b"PE\0\0" && u16::from_le_bytes([header[4], header[5]]) == 0x8664
            })
        }
        "macos-arm64" if bytes.len() >= 8 => {
            u32::from_le_bytes(bytes[..4].try_into()?) == 0xfeedfacf
                && u32::from_le_bytes(bytes[4..8].try_into()?) == 0x0100000c
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
    platform: &Platform,
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
        .join(format!("soloist-{}.part", std::process::id()));
    if archive_path.exists() {
        fs::remove_file(&archive_path)?;
    }
    client.download(artifact, &archive_path)?;
    let executable_name = if matches!(platform.os, OperatingSystem::Windows) {
        "soloist.exe"
    } else {
        "soloist"
    };
    let executable = version_dir.join(executable_name);
    let staged_executable =
        version_dir.join(format!(".{executable_name}.{}.part", std::process::id()));
    if staged_executable.exists() {
        fs::remove_file(&staged_executable)?;
    }
    safe_extract_soloist(
        &archive_path,
        &staged_executable,
        matches!(platform.os, OperatingSystem::Windows),
    )?;
    validate_soloist_binary(&staged_executable, &artifact.target)?;
    filesystem::set_executable(&staged_executable)?;
    fs::rename(&staged_executable, &executable)?;
    fs::remove_file(&archive_path).ok();
    filesystem::atomic_activate(&version_dir, &paths.managed_soloist_current)?;
    service::install_soloist(paths, &paths.managed_soloist_current.join(executable_name))?;
    Ok(SoloistState {
        ownership: SoloistOwnership::Installer,
        executable: Some(executable),
        service_owned: true,
        data_owned: true,
        websocket_url: Some(DEFAULT_WS.into()),
        version: Some(version.into()),
    })
}

/// Ensure the Soloist CLI can discover the fixed loopback endpoint used by Display.
/// Never replace an existing endpoint: independent Soloist installations may
/// have their own configuration, which the installer must not take over.
pub fn ensure_cli_discovery(paths: &Paths, state: &SoloistState) -> Result<()> {
    if paths.is_test() || !healthy(state) || state.ownership != SoloistOwnership::Installer {
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
    if state.ownership == SoloistOwnership::Independent {
        service::start_external_soloist(paths);
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

fn unquote_env(value: &str) -> Result<String> {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    let mut output = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    if escaped {
        bail!("invalid trailing escape in Soloist configuration");
    }
    Ok(output)
}

pub fn run_managed(paths: &Paths, executable: &Path) -> Result<()> {
    if !filesystem::canonical_contained(executable, &paths.managed_soloist) {
        bail!("refusing to run Soloist outside the installer-owned directory");
    }
    let contents =
        fs::read_to_string(&paths.soloist_env).context("read managed Soloist credentials")?;
    let mut name = None;
    let mut key = None;
    for line in contents.lines() {
        if let Some((field, value)) = line.split_once('=') {
            match field {
                "SOLOIST_NAME" => name = Some(unquote_env(value)?),
                "SOLOIST_API_KEY" => key = Some(unquote_env(value)?),
                _ => {}
            }
        }
    }
    let name = name.context("managed Soloist device name is missing")?;
    let key = key.context("managed Soloist API key is missing")?;
    let status = Command::new(executable)
        .args([
            "--device-name",
            &name,
            "--api-key",
            &key,
            "--ws",
            "127.0.0.1:9090",
            "--data-dir",
        ])
        .arg(&paths.soloist_data)
        .arg("--cache-dir")
        .arg(&paths.soloist_cache)
        .status()
        .context("start managed Spotify Soloist")?;
    if !status.success() {
        bail!("managed Spotify Soloist exited with {status}");
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
        let executable_name = if cfg!(windows) { "soloist.exe" } else { "soloist" };
        let independent = paths.home.join(".local/bin").join(executable_name);
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
        let owned = paths.managed_soloist.join("1").join(executable_name);
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
