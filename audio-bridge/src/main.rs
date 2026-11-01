use std::{env, f32::consts::PI, process::Stdio, sync::Arc};
use futures_util::SinkExt;
use rustfft::{FftPlanner, num_complex::Complex32};
use tokio::{io::AsyncReadExt, net::TcpListener, process::Command, sync::broadcast, time::{sleep, Duration}};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const RATE: usize = 22050;
const FFT_SIZE: usize = 2048;
const HOP: usize = 735; // ~30 updates per second
const BANDS: usize = 48;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("SOLOIST_DISPLAY_AUDIO_PORT").unwrap_or_else(|_| "9091".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let (sender, _) = broadcast::channel::<String>(16);
    let capture_tx = sender.clone();
    tokio::spawn(async move {
        if let Err(error) = capture(capture_tx).await { eprintln!("Capture stopped: {error}"); }
    });
    eprintln!("Spectrum WebSocket listening on ws://127.0.0.1:{port}");
    loop {
        let (socket, _) = listener.accept().await?;
        let mut receiver = sender.subscribe();
        tokio::spawn(async move {
            if let Ok(mut ws) = accept_async(socket).await {
                while let Ok(data) = receiver.recv().await {
                    if ws.send(Message::Text(data)).await.is_err() { break; }
                }
            }
        });
    }
}

async fn capture(sender: broadcast::Sender<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("parec");
    cmd.args(["--raw", "--format=s16le", "--rate", "22050", "--channels=1"]);
    if let Ok(source) = env::var("SOLOIST_DISPLAY_MONITOR") {
        if !source.is_empty() { cmd.args(["--device", &source]); }
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().ok_or("No audio stream")?;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut ring = vec![0f32; FFT_SIZE];
    let mut cursor = 0usize;
    let mut buffer = vec![0u8; HOP * 2];
    let mut spectrum = vec![Complex32::default(); FFT_SIZE];
    let window: Vec<f32> = (0..FFT_SIZE).map(|i| 0.5-0.5*(2.0*PI*i as f32/(FFT_SIZE-1) as f32).cos()).collect();
    loop {
        if stdout.read_exact(&mut buffer).await.is_err() { break; }
        for pair in buffer.chunks_exact(2) {
            ring[cursor] = i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0;
            cursor = (cursor + 1) % FFT_SIZE;
        }
        for i in 0..FFT_SIZE {
            spectrum[i] = Complex32::new(ring[(cursor+i)%FFT_SIZE]*window[i], 0.0);
        }
        fft.process(&mut spectrum);
        let mut bars = Vec::with_capacity(BANDS);
        for band in 0..BANDS {
            let low = 40.0_f32 * (9000.0_f32/40.0).powf(band as f32/BANDS as f32);
            let high = 40.0_f32 * (9000.0_f32/40.0).powf((band+1) as f32/BANDS as f32);
            let start = ((low*FFT_SIZE as f32/RATE as f32) as usize).min(FFT_SIZE/2);
            let end = (((high*FFT_SIZE as f32/RATE as f32).ceil() as usize).max(start+1)).min(FFT_SIZE/2);
            let magnitude = spectrum[start..end].iter().map(|v|v.norm()).fold(0.0f32,f32::max);
            let normalized = ((1.0+magnitude).ln()/4.2).clamp(0.0,1.0);
            bars.push((normalized*1000.0).round()/1000.0);
        }
        if sender.receiver_count()>0 {
            let payload=serde_json::json!({"type":"spectrum","bars":bars}).to_string();
            let _=sender.send(payload);
        }
        // Input capture itself clocks frames; no artificial timer or polling delay.
    }
    sleep(Duration::from_millis(50)).await;
    Err("parec exited; verify the configured monitor source".into())
}
