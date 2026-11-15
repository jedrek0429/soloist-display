//! User-authored runtime settings. The installer state remains private and transactional.
use crate::{
    manager,
    paths::Paths,
    service::{self, ManagedRuntime},
    soloist,
    state::{InstallState, SoloistOwnership},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::{IpAddr, SocketAddr},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::Command,
    thread,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    soloist: SoloistOptions,
    installation: InstallationOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SoloistOptions {
    device_name: Option<String>,
    ws: String,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pipewire_device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_volume: Option<u8>,
    verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallationOptions {
    mode: String,
    channel: soloist_display_core::release::ReleaseChannel,
}

const HEADER: &str = "# Soloist Display runtime configuration (Linux).\n# Edit this file, then run: soloist-display reload\n# Appearance settings remain in: soloist-display settings\n# WARNING: Setting ws to 0.0.0.0:9090 exposes the unauthenticated\n# Soloist WebSocket API to other devices. Use a trusted network or firewall.\n\n";

/// Map a listening socket to an address that local clients can actually connect to.
/// Never attempt WebSocket handshakes against 0.0.0.0 or ::.

fn format_config(config: &Config) -> String {
    fn quoted(value: &str) -> String {
        serde_json::to_string(value).expect("serialize string")
    }
    let o = &config.soloist;
    let mut out = format!(
        "{HEADER}[soloist]\ndevice_name = {}\nws = {}\ndata_dir = {}\ncache_dir = {}\n",
        quoted(o.device_name.as_deref().unwrap_or("Soloist Display")),
        quoted(&o.ws),
        quoted(&o.data_dir.to_string_lossy()),
        quoted(&o.cache_dir.to_string_lossy())
    );
    if let Some(size) = o.cache_size {
        out.push_str(&format!("cache_size = {size}\n"));
    }
    if let Some(dev) = &o.pipewire_device {
        out.push_str(&format!("pipewire_device = {}\n", quoted(dev)));
    }
    if let Some(volume) = o.initial_volume {
        out.push_str(&format!("initial_volume = {volume}\n"));
    }
    out.push_str(&format!(
        "verbose = {}\n\n[installation]\nmode = {}\nchannel = {}\n",
        o.verbose,
        quoted(&config.installation.mode),
        quoted(match config.installation.channel {
            soloist_display_core::release::ReleaseChannel::Stable => "stable",
            soloist_display_core::release::ReleaseChannel::Alpha => "alpha",
            soloist_display_core::release::ReleaseChannel::Beta => "beta",
        })
    ));
    out
}

fn parse_config(contents: &str) -> Result<Config> {
    let doc: toml_edit::DocumentMut = contents.parse().context("invalid TOML syntax")?;
    for (key, _) in doc.iter() {
        if key != "soloist" && key != "installation" {
            bail!("unknown section: {key}");
        }
    }
    let soloist = doc
        .get("soloist")
        .and_then(|item| item.as_table())
        .context("missing [soloist] table")?;
    let installation = doc
        .get("installation")
        .and_then(|item| item.as_table())
        .context("missing [installation] table")?;
    for (key, _) in soloist.iter() {
        if ![
            "device_name",
            "ws",
            "data_dir",
            "cache_dir",
            "cache_size",
            "pipewire_device",
            "initial_volume",
            "verbose",
        ]
        .contains(&key)
        {
            bail!("unknown soloist option: {key}");
        }
    }
    for (key, _) in installation.iter() {
        if !["mode", "channel"].contains(&key) {
            bail!("unknown installation option: {key}");
        }
    }
    fn opt_string(table: &toml_edit::Table, key: &str) -> Result<Option<String>> {
        table
            .get(key)
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("{key} must be a string"))
            })
            .transpose()
    }
    fn required_string(table: &toml_edit::Table, key: &str) -> Result<String> {
        opt_string(table, key)?.ok_or_else(|| anyhow!("missing {key}"))
    }
    fn opt_u64(table: &toml_edit::Table, key: &str) -> Result<Option<u64>> {
        table
            .get(key)
            .map(|item| {
                let number = item
                    .as_integer()
                    .ok_or_else(|| anyhow!("{key} must be a nonnegative integer"))?;
                u64::try_from(number).context(format!("{key} must be nonnegative"))
            })
            .transpose()
    }
    let channel = match required_string(installation, "channel")?.as_str() {
        "stable" => soloist_display_core::release::ReleaseChannel::Stable,
        "alpha" => soloist_display_core::release::ReleaseChannel::Alpha,
        "beta" => soloist_display_core::release::ReleaseChannel::Beta,
        _ => bail!("installation.channel must be stable, alpha, or beta"),
    };
    let verbose = soloist
        .get("verbose")
        .map(|item| {
            item.as_bool()
                .ok_or_else(|| anyhow!("verbose must be a boolean"))
        })
        .transpose()?
        .unwrap_or(false);
    Ok(Config {
        soloist: SoloistOptions {
            device_name: opt_string(soloist, "device_name")?,
            ws: required_string(soloist, "ws")?,
            data_dir: PathBuf::from(required_string(soloist, "data_dir")?),
            cache_dir: PathBuf::from(required_string(soloist, "cache_dir")?),
            cache_size: opt_u64(soloist, "cache_size")?,
            pipewire_device: opt_string(soloist, "pipewire_device")?,
            initial_volume: opt_u64(soloist, "initial_volume")?
                .map(|v| u8::try_from(v).context("initial_volume must fit in 0..100"))
                .transpose()?,
            verbose,
        },
        installation: InstallationOptions {
            mode: required_string(installation, "mode")?,
            channel,
        },
    })
}

pub fn client_for_bind(bind: &str) -> Result<String> {
    let socket: SocketAddr = bind
        .parse()
        .context("soloist.ws must be an IP:port socket address (IPv6: [::]:9090)")?;
    if socket.port() == 0 {
        bail!("soloist.ws must use a fixed nonzero port");
    }
    let client = match socket.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    if !matches!(
        client,
        IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) | IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
    ) {
        bail!("soloist.ws must bind to 127.0.0.1, [::1], 0.0.0.0, or [::]");
    }
    Ok(match client {
        IpAddr::V4(ip) => format!("ws://{ip}:{}/", socket.port()),
        IpAddr::V6(ip) => format!("ws://[{ip}]:{}/", socket.port()),
    })
}

fn bind_from_runtime(runtime: &ManagedRuntime) -> Result<String> {
    if let Some(bind) = &runtime.websocket_bind {
        return Ok(bind.clone());
    }
    let url = url::Url::parse(&runtime.websocket_url)?;
    let host = url.host_str().context("missing managed WebSocket host")?;
    let port = url.port().context("missing managed WebSocket port")?;
    Ok(if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    })
}

fn mode(state: &InstallState) -> &'static str {
    match state.soloist.ownership {
        SoloistOwnership::Installer => "managed",
        SoloistOwnership::Independent => "external",
        SoloistOwnership::None => "none",
    }
}

fn config_path(paths: &Paths) -> PathBuf {
    paths.config.join("config.toml")
}

pub fn ensure(paths: &Paths) -> Result<()> {
    let path = config_path(paths);
    if path.is_symlink() {
        bail!("refusing symlinked config.toml");
    }
    if path.exists() {
        return Ok(());
    }
    let state = InstallState::load(paths)?.context("Soloist Display is not installed")?;
    let runtime = ManagedRuntime::load(paths)?;
    let (name, _) = soloist::configured_credentials(paths)?;
    let config = Config {
        soloist: SoloistOptions {
            device_name: name,
            ws: bind_from_runtime(&runtime)?,
            data_dir: runtime.data_dir,
            cache_dir: runtime.cache_dir,
            cache_size: runtime.cache_size_mb,
            pipewire_device: runtime.pipewire_device,
            initial_volume: runtime.initial_volume,
            verbose: runtime.verbose,
        },
        installation: InstallationOptions {
            mode: mode(&state).to_string(),
            channel: state.effective_channel(),
        },
    };
    fs::create_dir_all(&paths.config)?;
    let value = format_config(&config);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(value.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn read(paths: &Paths) -> Result<Config> {
    let path = config_path(paths);
    if path.is_symlink() {
        bail!("refusing symlinked config.toml");
    }
    let config = parse_config(
        &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .context("invalid config.toml")?;
    let expected = InstallState::load(paths)?.context("Soloist Display is not installed")?;
    if config.installation.mode != mode(&expected) {
        bail!("changing installation.mode requires reinstalling in the selected mode");
    }
    if let Some(name) = &config.soloist.device_name {
        if name.trim().is_empty() || name.contains(['\n', '\r', '\0']) {
            bail!("invalid soloist.device_name");
        }
    }
    if !config.soloist.data_dir.is_absolute() || !config.soloist.cache_dir.is_absolute() {
        bail!("Soloist directories must be absolute paths");
    }
    if config
        .soloist
        .cache_size
        .is_some_and(|size| size != 0 && size < 100)
    {
        bail!("soloist.cache_size must be 0 or at least 100 MB");
    }
    if config
        .soloist
        .initial_volume
        .is_some_and(|value| value > 100)
    {
        bail!("soloist.initial_volume must be between 0 and 100");
    }
    client_for_bind(&config.soloist.ws)?;
    Ok(config)
}

pub fn sync_channel(paths: &Paths) -> Result<()> {
    ensure(paths)?;
    let path = config_path(paths);
    if path.is_symlink() {
        bail!("refusing symlinked config.toml");
    }
    let original = fs::read_to_string(&path)?;
    let doc: toml_edit::DocumentMut = original.parse().context("invalid config.toml")?;
    let state = InstallState::load(paths)?.context("Soloist Display is not installed")?;
    let channel = match state.effective_channel() {
        soloist_display_core::release::ReleaseChannel::Stable => "stable",
        soloist_display_core::release::ReleaseChannel::Alpha => "alpha",
        soloist_display_core::release::ReleaseChannel::Beta => "beta",
    };
    if doc["installation"]["channel"].as_str() == Some(channel) {
        return Ok(());
    }
    let mut in_installation = false;
    let mut replaced = false;
    let mut output = String::new();
    for line in original.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_installation = trimmed == "[installation]";
        }
        if in_installation
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "channel")
        {
            output.push_str(&format!("channel = \"{channel}\"\n"));
            replaced = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !replaced {
        bail!("missing installation.channel");
    }
    let temp = path.with_extension(format!("toml.tmp.{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(output.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temp, &path)?;
    Ok(())
}

pub fn edit(paths: &Paths) -> Result<()> {
    ensure(paths)?;
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into());
    let status = Command::new(editor)
        .arg(config_path(paths))
        .status()
        .context("launch editor (set $EDITOR)")?;
    if !status.success() {
        bail!("editor exited unsuccessfully");
    }
    Ok(())
}

pub fn reload(paths: &Paths, dry_run: bool) -> Result<()> {
    ensure(paths)?;
    let config = read(paths)?;
    let mut state = InstallState::load(paths)?.context("Soloist Display is not installed")?;
    let old_runtime = ManagedRuntime::load(paths)?;
    let old_name = soloist::configured_credentials(paths)?.0;

    if state.soloist.ownership == SoloistOwnership::Independent {
        // External Soloist is not owned by this application; do not alter its service.
        if config.installation.channel != state.effective_channel() {
            if dry_run {
                println!("Would change update channel.");
            } else {
                state.channel = Some(config.installation.channel);
                state.save(paths)?;
            }
        } else {
            println!("No installation changes.");
        }
        return Ok(());
    }
    if state.soloist.ownership != SoloistOwnership::Installer {
        bail!("Soloist is not managed");
    }

    let mut desired = old_runtime.clone();
    desired.data_dir = config.soloist.data_dir;
    desired.cache_dir = config.soloist.cache_dir;
    desired.cache_size_mb = config.soloist.cache_size;
    desired.pipewire_device = config.soloist.pipewire_device;
    desired.initial_volume = config.soloist.initial_volume;
    desired.verbose = config.soloist.verbose;
    desired.websocket_bind = if config.soloist.ws == bind_from_runtime(&old_runtime)? {
        old_runtime.websocket_bind.clone()
    } else {
        Some(config.soloist.ws.clone())
    };
    desired.websocket_url = client_for_bind(&config.soloist.ws)?;
    // Validate before writing anything.
    if !desired.data_dir.is_absolute() || !desired.cache_dir.is_absolute() {
        bail!("Soloist directories must be absolute");
    }
    if desired
        .pipewire_device
        .as_ref()
        .is_some_and(|d| d.is_empty() || d.contains(['\n', '\r', '\0']))
    {
        bail!("invalid soloist.pipewire_device");
    }
    let name_changed =
        config.soloist.device_name != old_name && config.soloist.device_name.is_some();
    let runtime_changed = serde_json::to_value(&desired)? != serde_json::to_value(&old_runtime)?;
    let channel_changed = config.installation.channel != state.effective_channel();
    if !name_changed && !runtime_changed && !channel_changed {
        println!("Configuration unchanged.");
        return Ok(());
    }
    if dry_run {
        println!(
            "Would update: {}{}{}",
            if runtime_changed {
                "Soloist runtime "
            } else {
                ""
            },
            if name_changed { "device name " } else { "" },
            if channel_changed {
                "release channel"
            } else {
                ""
            }
        );
        return Ok(());
    }
    if !runtime_changed && !name_changed {
        state.channel = Some(config.installation.channel);
        state.save(paths)?;
        println!("Configuration reloaded.");
        return Ok(());
    }

    // Keep the applied runtime metadata and service intact until validation succeeds.
    let unit_backup = fs::read(&paths.soloist_service).ok();
    let env_backup = fs::read(&paths.soloist_env).ok();
    let previous_state = state.clone();
    let result = (|| -> Result<()> {
        desired.save(paths)?;
        if name_changed {
            soloist::configure(paths, config.soloist.device_name.clone(), None)?;
        }
        service::install_soloist(paths, &paths.managed_soloist_current.join("soloist"))?;
        state.soloist.websocket_url = Some(desired.websocket_url.clone());
        let managed_healthy = || {
            service::managed_process_healthy(
                paths,
                &paths.managed_soloist_current.join("soloist"),
                &desired.websocket_url,
                || soloist::healthy(&state.soloist),
            )
        };
        for _ in 0..20 {
            if managed_healthy() {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        if !managed_healthy() {
            bail!("managed Soloist did not become healthy");
        }
        state.channel = Some(config.installation.channel);
        state.save(paths)?;
        manager::save_soloist_websocket(paths, &desired.websocket_url)?;
        Ok(())
    })();
    if let Err(error) = result {
        old_runtime
            .save(paths)
            .context("restore previous Soloist runtime")?;
        if let Some(bytes) = unit_backup {
            fs::write(&paths.soloist_service, bytes)?;
        }
        if let Some(bytes) = env_backup {
            fs::write(&paths.soloist_env, bytes)?;
        }
        previous_state.save(paths)?;
        service::daemon_reload(paths)?;
        service::restart_soloist(paths)?;
        return Err(error.context("reload failed; previous runtime restored"));
    }
    println!("Configuration reloaded.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_bind_connects_on_loopback() {
        assert_eq!(
            client_for_bind("0.0.0.0:9090").unwrap(),
            "ws://127.0.0.1:9090/"
        );
        assert_eq!(client_for_bind("[::]:9090").unwrap(), "ws://[::1]:9090/");
        assert!(client_for_bind("0.0.0.0:0").is_err());
        assert!(client_for_bind("192.168.1.2:9090").is_err());
        assert!(client_for_bind("127.0.0.2:9090").is_err());
        let original = Config {
            soloist: SoloistOptions {
                device_name: Some("Living Room".into()),
                ws: "0.0.0.0:9090".into(),
                data_dir: "/tmp/data".into(),
                cache_dir: "/tmp/cache".into(),
                cache_size: Some(500),
                pipewire_device: None,
                initial_volume: Some(50),
                verbose: false,
            },
            installation: InstallationOptions {
                mode: "managed".into(),
                channel: soloist_display_core::release::ReleaseChannel::Alpha,
            },
        };
        let parsed = parse_config(&format_config(&original)).unwrap();
        assert_eq!(parsed.soloist.ws, original.soloist.ws);
        assert_eq!(parsed.soloist.initial_volume, Some(50));
    }
}
