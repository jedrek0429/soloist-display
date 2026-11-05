//! Read-only diagnostics for the standalone External Soloist preview.

use crate::{config_path, read_settings};
use cpal::traits::HostTrait;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_SOLOIST: &str = "ws://127.0.0.1:9090";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    Doctor,
    Help,
    Version,
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Serve),
        [arg] if arg == "doctor" => Ok(Command::Doctor),
        [arg] if arg == "--help" || arg == "-h" => Ok(Command::Help),
        [arg] if arg == "--version" || arg == "-V" => Ok(Command::Version),
        _ => Err("unknown command; use --help to list available commands".into()),
    }
}

pub fn help() {
    println!("Soloist Display (External-only desktop preview)");
    println!("Usage: soloist-display [doctor|--help|--version]");
    println!("Run without arguments to serve http://127.0.0.1:8080");
    println!("Managed Soloist is not available in this build.");
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
    println!("Soloist Display (External-only desktop preview)");
    println!("Configuration: {}", path.display());
    println!("Soloist WebSocket: {endpoint}");
    let healthy = protocol_healthy(endpoint).await;
    println!(
        "External Soloist: {}",
        if healthy { "healthy" } else { "unavailable" }
    );
    let output = cpal::default_host().default_output_device();
    match &output {
        Some(device) => println!("Default audio output: {device}"),
        None => println!("Default audio output: unavailable"),
    }
    println!("Managed Soloist: unsupported in this preview");
    healthy && output.is_some()
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
        assert!(parse(&["install".into()]).is_err());
        assert!(parse(&["uninstall".into()]).is_err());
        assert!(parse(&["update".into()]).is_err());
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
