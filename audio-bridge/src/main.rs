use std::{env, process::Stdio};
use futures_util::SinkExt;
use tokio::{io::AsyncReadExt, net::TcpListener, process::Command, sync::broadcast, time::Duration};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const RATE: usize = 22050;
const SAMPLES: usize = 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("SOLOIST_DISPLAY_AUDIO_PORT").unwrap_or_else(|_| "9091".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let (sender, _) = broadcast::channel::<Vec<u8>>(32);
    let capture_tx = sender.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = capture(capture_tx.clone()).await {
                eprintln!("Audio capture stopped: {error}; retrying");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    eprintln!("Raw PCM WebSocket listening on ws://127.0.0.1:{port}");
    loop {
        let (socket, _) = listener.accept().await?;
        let mut receiver = sender.subscribe();
        tokio::spawn(async move {
            if let Ok(mut ws) = accept_async(socket).await {
                loop {
                    let bytes = match receiver.recv().await {
                        Ok(bytes) => bytes,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    };
                    if ws.send(Message::Binary(bytes)).await.is_err() { break; }
                }
            }
        });
    }
}

async fn capture(sender: broadcast::Sender<Vec<u8>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("parec");
    cmd.args(["--raw", "--format=s16le", "--rate", "22050", "--channels=1"]);
    let source = env::var("SOLOIST_DISPLAY_MONITOR").ok().filter(|s| !s.is_empty()).or_else(|| {
        let output = std::process::Command::new("pactl").arg("get-default-sink").output().ok()?;
        if !output.status.success() { return None; }
        let sink = String::from_utf8(output.stdout).ok()?;
        Some(format!("{}.monitor", sink.trim()))
    });
    if let Some(source) = &source {
        cmd.arg("--device").arg(source);
        eprintln!("Capturing monitor source: {source}");
    } else {
        eprintln!("Warning: no output monitor found; set SOLOIST_DISPLAY_MONITOR explicitly");
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().ok_or("No audio stream")?;
    let mut buffer = vec![0u8; SAMPLES * 2];
    let mut packets = 0u64;
    let mut peak = 0f32;
    let mut report = tokio::time::Instant::now();
    loop {
        stdout.read_exact(&mut buffer).await?;
        for pair in buffer.chunks_exact(2) {
            peak = peak.max((i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0).abs());
        }
        if sender.receiver_count() > 0 { let _ = sender.send(buffer.clone()); }
        packets += 1;
        if report.elapsed() >= Duration::from_secs(5) {
            eprintln!("PCM capture: {:.1} packets/s, peak {:.3}, listeners {}", packets as f32 / report.elapsed().as_secs_f32(), peak, sender.receiver_count());
            packets = 0;
            peak = 0.0;
            report = tokio::time::Instant::now();
        }
    }
}
