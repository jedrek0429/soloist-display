use std::{env, process::Stdio, time::Instant};
use futures_util::SinkExt;
use rustfft::{num_complex::Complex32, FftPlanner};
use tokio::{io::AsyncReadExt, net::TcpListener, process::Command, sync::broadcast, time::Duration};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const RATE: usize = 22050;
const WINDOW: usize = 2048;
const HOP: usize = 512;
const BANDS: [f32; 7] = [55., 150., 350., 800., 1800., 4000., 9000.];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("SOLOIST_DISPLAY_AUDIO_PORT").unwrap_or_else(|_| "9091".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let (sender, _) = broadcast::channel::<String>(64);
    let capture_tx = sender.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = capture(capture_tx.clone()).await {
                eprintln!("Audio capture stopped: {error}; retrying");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    eprintln!("Spectrum WebSocket listening on ws://127.0.0.1:{port} (6 bands, {:.1} Hz)", RATE as f32 / HOP as f32);
    loop {
        let (socket, _) = listener.accept().await?;
        let mut receiver = sender.subscribe();
        tokio::spawn(async move {
            if let Ok(mut ws) = accept_async(socket).await {
                loop {
                    let frame = match receiver.recv().await {
                        Ok(frame) => frame,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    };
                    if ws.send(Message::Text(frame.into())).await.is_err() { break; }
                }
            }
        });
    }
}

async fn capture(sender: broadcast::Sender<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("parec");
    cmd.args(["--raw", "--format=s16le", "--rate", "22050", "--channels=1"]);
    let source = env::var("SOLOIST_DISPLAY_MONITOR").ok().filter(|s| !s.is_empty()).or_else(|| {
        let output = std::process::Command::new("pactl").arg("get-default-sink").output().ok()?;
        if !output.status.success() { return None; }
        Some(format!("{}.monitor", String::from_utf8(output.stdout).ok()?.trim()))
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
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let hann: Vec<f32> = (0..WINDOW).map(|i| {
        0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos()
    }).collect();
    let mut ring = [0f32; WINDOW];
    let mut cursor = 0usize;
    let mut filled = 0usize;
    let mut frame = vec![Complex32::new(0., 0.); WINDOW];
    let mut raw = [0u8; HOP * 2];
    let mut smoothed = [0f32; 6];
    let mut peak = 0f32;
    let mut frames = 0u64;
    let mut report = Instant::now();
    loop {
        stdout.read_exact(&mut raw).await?;
        for bytes in raw.chunks_exact(2) {
            let sample = i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.;
            peak = peak.max(sample.abs());
            ring[cursor] = sample;
            cursor = (cursor + 1) % WINDOW;
            filled = (filled + 1).min(WINDOW);
        }
        if filled < WINDOW { continue; }
        for i in 0..WINDOW {
            frame[i] = Complex32::new(ring[(cursor + i) % WINDOW] * hann[i], 0.);
        }
        fft.process(&mut frame);
        let mut bands = [0f32; 6];
        for band in 0..6 {
            let lo = ((BANDS[band] * WINDOW as f32 / RATE as f32).floor() as usize).max(1);
            let hi = ((BANDS[band + 1] * WINDOW as f32 / RATE as f32).ceil() as usize).min(WINDOW / 2);
            let mut power = 0f32;
            let mut count = 0usize;
            for bin in lo..hi {
                power += frame[bin].norm_sqr();
                count += 1;
            }
            let rms = if count > 0 { (power / count as f32).sqrt() / (WINDOW as f32 * 0.25) } else { 0. };
            // Convert frequency-domain amplitude to a usable visual range, with
            // a modest noise gate. Keep each band's response independent.
            let db = 20. * rms.max(1e-8).log10();
            let level = ((db + 68.) / 49.).clamp(0., 1.).powf(1.25);
            // ~12 ms attack / ~180 ms release at 43 Hz.
            let coefficient = if level > smoothed[band] { 0.85 } else { 0.12 };
            smoothed[band] += (level - smoothed[band]) * coefficient;
            bands[band] = smoothed[band];
        }
        frames += 1;
        if sender.receiver_count() > 0 {
            let payload = format!(
                "{{\"bands\":[{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}]}}",
                bands[0], bands[1], bands[2], bands[3], bands[4], bands[5]
            );
            let _ = sender.send(payload);
        }
        if report.elapsed() >= Duration::from_secs(5) {
            eprintln!(
                "Audio: {:.1} frames/s | peak: {:.3} | bands: {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} | clients: {}",
                frames as f32 / report.elapsed().as_secs_f32(), peak,
                bands[0], bands[1], bands[2], bands[3], bands[4], bands[5],
                sender.receiver_count()
            );
            report = Instant::now();
            frames = 0;
            peak = 0.;
        }
    }
}
