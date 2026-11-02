use super::AudioChunk;
use anyhow::{anyhow, Context, Result};
use screencapturekit::prelude::*;
use std::{sync::mpsc::SyncSender, time::Duration};

struct Handler {
    sender: SyncSender<AudioChunk>,
}

impl SCStreamOutputTrait for Handler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, output_type: SCStreamOutputType) {
        if output_type != SCStreamOutputType::Audio {
            return;
        }
        let rate = sample
            .format_description()
            .and_then(|format| format.audio_sample_rate())
            .unwrap_or(48_000.) as u32;
        let Ok(buffers) = sample.audio_buffer_list() else {
            return;
        };
        for buffer in &buffers {
            let samples: Vec<f32> = buffer
                .data()
                .chunks_exact(4)
                .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
            if !samples.is_empty() {
                // ScreenCaptureKit may deliver either one interleaved buffer or
                // one buffer per channel. Treat planar buffers as mono chunks;
                // the spectrum is intentionally based on a mono downmix.
                let channels = sample
                    .format_description()
                    .and_then(|format| format.audio_channel_count())
                    .filter(|_| buffers.len() == 1)
                    .unwrap_or(1) as u16;
                let _ = self.sender.try_send(AudioChunk {
                    samples,
                    sample_rate: rate,
                    channels,
                });
            }
        }
    }
}

pub fn capture(sender: SyncSender<AudioChunk>) -> Result<()> {
    let content = SCShareableContent::get()
        .context("request macOS Screen Recording access for system-output audio capture")?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("macOS reported no display for system-audio capture"))?;
    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build()?;
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_minimum_frame_interval(&CMTime::new(1, 1))
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(48_000)
        .with_channel_count(2);
    let mut stream = SCStream::new(&filter, &config)?;
    stream.add_output_handler(Handler { sender }, SCStreamOutputType::Audio)?;
    stream
        .start_capture()
        .context("start ScreenCaptureKit system-audio capture")?;
    eprintln!("Capturing system output with ScreenCaptureKit");
    loop {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}
