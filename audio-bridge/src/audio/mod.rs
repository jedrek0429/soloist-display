mod processor;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use anyhow::{anyhow, Result};
use processor::SpectrumProcessor;
use std::sync::mpsc;
use tokio::sync::broadcast;

#[derive(Debug)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn capture_backend() -> &'static str {
    #[cfg(target_os = "linux")]
    return "PulseAudio/PipeWire monitor";
    #[cfg(windows)]
    return "WASAPI loopback";
    #[cfg(target_os = "macos")]
    return "ScreenCaptureKit system audio";
    #[allow(unreachable_code)]
    "unsupported"
}

/// Capture system playback and deliver the existing six-band JSON protocol.
/// Acquisition runs separately from resampling/FFT so a device restart cannot
/// corrupt processor state or the WebSocket server.
pub fn run(sender: broadcast::Sender<String>) -> Result<()> {
    let (chunk_tx, chunk_rx) = mpsc::sync_channel(8);
    let capture = std::thread::Builder::new()
        .name("soloist-audio-capture".into())
        .spawn(move || backend_capture(chunk_tx))?;
    let mut processor = SpectrumProcessor::new();
    while let Ok(chunk) = chunk_rx.recv() {
        for bands in processor.push(chunk)? {
            if sender.receiver_count() > 0 {
                let payload = format!(
                    "{{\"bands\":[{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}]}}",
                    bands[0], bands[1], bands[2], bands[3], bands[4], bands[5]
                );
                let _ = sender.send(payload);
            }
        }
    }
    match capture.join() {
        Ok(result) => result,
        Err(_) => Err(anyhow!("audio capture backend panicked")),
    }
}

fn backend_capture(sender: mpsc::SyncSender<AudioChunk>) -> Result<()> {
    #[cfg(target_os = "linux")]
    return linux::capture(sender);
    #[cfg(windows)]
    return windows::capture(sender);
    #[cfg(target_os = "macos")]
    return macos::capture(sender);
    #[allow(unreachable_code)]
    Err(anyhow!(
        "system-output capture is unavailable on this platform"
    ))
}
