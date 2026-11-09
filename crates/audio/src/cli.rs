//! Runtime diagnostics and audio output selection.
use crate::{cli_output, config_path, device, read_settings};
use futures_util::{SinkExt, StreamExt};
pub use soloist_display_cli::desktop::{completions, help, parse, quiet, verbose, Command};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
const DEFAULT_SOLOIST: &str = "ws://127.0.0.1:9090";

/// Device selection controls only the output loopback used for visualisation.
pub fn list_devices() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let outputs = device::outputs()?;
    let selected = device::selected_id()?;
    println!("Audio bridge output (used for the visualiser, not Spotify Connect):");
    if outputs.is_empty() {
        println!("No output devices detected.");
    }
    for (index, output) in outputs.into_iter().enumerate() {
        let mode = if selected.as_deref() == Some(output.id.as_str()) {
            " [selected]"
        } else if output.is_default {
            " [system default]"
        } else {
            ""
        };
        println!("{}. {}{}", index + 1, output.name, mode);
        println!("  ID: {}", output.id);
    }
    if selected.is_none() {
        println!("Selection: automatic (system default)");
    } else if let Some(id) = selected {
        println!("Selection: {id}");
    }
    Ok(())
}

pub fn set_device(id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let name = device::set(id)?;
    println!("Visualiser audio output: {name}");
    println!("Saved. A running audio bridge will reconnect to this output.");
    Ok(())
}

pub fn default_device() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    device::clear()?;
    println!("Visualiser audio output: system default (automatic)");
    println!("Saved. A running audio bridge will reconnect to the default output.");
    Ok(())
}

/// Match the Soloist authentication-state protocol, not merely an open TCP port.
/// Diagnostics must never connect to a remote host supplied in the settings.
async fn protocol_healthy(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    if url.scheme() != "ws"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
        || url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_none()
    {
        return false;
    }

    let Ok(Ok((mut ws, _))) =
        tokio::time::timeout(Duration::from_secs(2), connect_async(endpoint)).await
    else {
        return false;
    };
    if ws
        .send(Message::Text(
            r#"{"type":"command","command":"get_auth_state"}"#.into(),
        ))
        .await
        .is_err()
    {
        return false;
    }

    for _ in 0..4 {
        let Ok(Some(Ok(message))) = tokio::time::timeout(Duration::from_secs(2), ws.next()).await
        else {
            return false;
        };
        if let Message::Text(text) = message {
            if serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .is_some_and(|value| value["type"] == "auth_state")
            {
                return true;
            }
        }
    }
    false
}

/// Checks only: no service install, no process start, no updates.
pub async fn doctor() -> bool {
    let path = config_path();
    let settings = read_settings(&path);
    let endpoint = settings["soloistWebSocket"]
        .as_str()
        .unwrap_or(DEFAULT_SOLOIST);
    cli_output::heading("External-only desktop preview");
    cli_output::section("Configuration");
    cli_output::field("Config file", path.display());
    cli_output::field("Soloist WebSocket", endpoint);
    println!();
    cli_output::section("Diagnostics");
    let healthy = protocol_healthy(endpoint).await;
    cli_output::check(
        "External Soloist",
        healthy,
        "Start Soloist and verify the local WebSocket endpoint.",
    );
    let output = device::output();
    match &output {
        Ok(name) => {
            cli_output::check("Audio bridge", true, "");
            cli_output::field("Visualiser output", name);
        }
        Err(error) => {
            cli_output::check(
                "Audio bridge",
                false,
                "Select an available output with 'soloist-display device list/use'.",
            );
            cli_output::field("Audio error", error);
        }
    }
    cli_output::field("Managed Soloist", "Not supported (External-only)");
    healthy && output.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_external_preview_commands_are_accepted() {
        assert_eq!(parse(&[]).unwrap(), Command::Serve);
        assert_eq!(parse(&["doctor".into()]).unwrap(), Command::Doctor);
        assert_eq!(parse(&["--help".into()]).unwrap(), Command::Help);
        assert_eq!(parse(&["--version".into()]).unwrap(), Command::Version);
        assert_eq!(
            parse(&["device".into(), "list".into()]).unwrap(),
            Command::DeviceList
        );
        assert_eq!(
            parse(&["device".into(), "set".into(), "wasapi:abc".into()]).unwrap(),
            Command::DeviceSet("wasapi:abc".into())
        );
        assert_eq!(
            parse(&["device".into(), "use".into(), "2".into()]).unwrap(),
            Command::DeviceSet("2".into())
        );
        assert_eq!(
            parse(&["device".into(), "default".into()]).unwrap(),
            Command::DeviceDefault
        );
        assert!(parse(&["device".into(), "set".into()]).is_err());
        assert!(parse(&["device".into(), "use".into()]).is_err());
        assert!(parse(&["device".into(), "set".into(), "".into()]).is_err());
        assert_eq!(parse(&["launch".into()]).unwrap(), Command::Launch);
        assert_eq!(parse(&["settings".into()]).unwrap(), Command::Settings);
        assert!(matches!(
            parse(&["completions".into(), "powershell".into()]),
            Ok(Command::Completions(_))
        ));
        assert!(parse(&["--verbose".into(), "--quiet".into()]).is_err());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn desktop_lifecycle_commands_are_accepted() {
        assert_eq!(parse(&["install".into()]).unwrap(), Command::Install);
        assert_eq!(
            parse(&["uninstall".into()]).unwrap(),
            Command::Uninstall(false)
        );
        assert_eq!(
            parse(&["uninstall".into(), "--data".into(), "remove".into()]).unwrap(),
            Command::Uninstall(true)
        );
        assert!(matches!(
            parse(&["update".into()]),
            Ok(Command::Update {
                channel: None,
                version: None,
                allow_downgrade: false
            })
        ));
        assert_eq!(
            parse(&["check-updates".into()]).unwrap(),
            Command::CheckUpdates
        );
        assert_eq!(
            parse(&["notifications".into(), "off".into()]).unwrap(),
            Command::Notifications(false)
        );
        assert_eq!(parse(&["repair".into()]).unwrap(), Command::Repair);
        assert!(parse(&["uninstall".into(), "--data".into(), "invalid".into()]).is_err());
    }

    #[tokio::test]
    async fn doctor_checks_soloist_protocol_not_just_a_websocket_handshake() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request = ws.next().await.unwrap().unwrap();
            assert_eq!(
                request.into_text().unwrap().as_str(),
                r#"{"type":"command","command":"get_auth_state"}"#
            );
            ws.send(Message::Text(
                r#"{"type":"auth_state","ready":true}"#.into(),
            ))
            .await
            .unwrap();
        });
        assert!(protocol_healthy(&endpoint).await);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn diagnostic_never_probes_remote_endpoints() {
        assert!(!protocol_healthy("ws://example.com:9090").await);
        assert!(!protocol_healthy("wss://127.0.0.1:9090").await);
        assert!(!protocol_healthy("ws://user@localhost:9090").await);
        assert!(!protocol_healthy("ws://localhost:9090/#fragment").await);
    }
}
