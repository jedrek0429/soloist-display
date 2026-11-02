use super::AudioChunk;
use anyhow::{anyhow, Context, Result};
use std::{collections::VecDeque, sync::mpsc::SyncSender};
use wasapi::{initialize_mta, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

pub fn capture(sender: SyncSender<AudioChunk>) -> Result<()> {
    initialize_mta().ok().context("initialize Windows audio")?;
    let device = DeviceEnumerator::new()?
        .get_default_device(&Direction::Render)
        .context("find the default Windows playback device")?;
    let mut client = device.get_iaudioclient()?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
    let (_, minimum_period) = client.get_device_period()?;
    client.initialize_client(
        &format,
        &Direction::Capture,
        &StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: minimum_period,
        },
    )?;
    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    let mut bytes = VecDeque::new();
    client.start_stream()?;
    eprintln!("Capturing default playback device with WASAPI loopback");
    loop {
        capture.read_from_device_to_deque(&mut bytes)?;
        let complete = bytes.len() / 8 * 8;
        if complete > 0 {
            let raw: Vec<u8> = bytes.drain(..complete).collect();
            let samples = raw
                .chunks_exact(4)
                .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
                .collect();
            sender
                .send(AudioChunk {
                    samples,
                    sample_rate: 48_000,
                    channels: 2,
                })
                .map_err(|_| anyhow!("audio processor stopped"))?;
        }
        event
            .wait_for_event(2_000)
            .context("WASAPI loopback device stopped responding")?;
    }
}
