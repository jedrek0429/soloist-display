use super::AudioChunk;
use anyhow::{anyhow, Context, Result};
use std::{
    env,
    io::Read,
    process::{Command, Stdio},
    sync::mpsc::SyncSender,
};

pub fn capture(sender: SyncSender<AudioChunk>) -> Result<()> {
    let source = env::var("SOLOIST_DISPLAY_MONITOR")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(default_monitor);
    let source = source.ok_or_else(|| {
        anyhow!("no PulseAudio/PipeWire output monitor found; set SOLOIST_DISPLAY_MONITOR to a monitor source")
    })?;
    eprintln!("Capturing monitor source: {source}");
    let mut child = Command::new("parec")
        .args([
            "--raw",
            "--format=s16le",
            "--rate=22050",
            "--channels=1",
            "--latency-msec=25",
            "--process-time-msec=10",
            "--device",
            &source,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("start parec monitor capture")?;
    let mut stdout = child.stdout.take().context("open parec audio stream")?;
    let mut raw = [0u8; 1024];
    loop {
        stdout
            .read_exact(&mut raw)
            .context("read PulseAudio/PipeWire monitor")?;
        let samples = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| i16::from_le_bytes(*bytes) as f32 / 32768.)
            .collect();
        sender
            .send(AudioChunk {
                samples,
                sample_rate: 22_050,
                channels: 1,
            })
            .map_err(|_| anyhow!("audio processor stopped"))?;
    }
}

fn default_monitor() -> Option<String> {
    let output = Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| format!("{}.monitor", String::from_utf8_lossy(&output.stdout).trim()))
}
