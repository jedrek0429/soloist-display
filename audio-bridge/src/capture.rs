use crate::{pcm::PcmInput, spectrum::Spectrum};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use std::{
    env,
    error::Error,
    sync::mpsc::{self, SyncSender},
    time::Duration,
};
use tokio::sync::broadcast;

type CaptureResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[cfg(target_os = "linux")]
fn output_device() -> CaptureResult<(cpal::Device, bool)> {
    let host = cpal::host_from_id(cpal::HostId::PulseAudio)?;
    let preferred = env::var("SOLOIST_DISPLAY_MONITOR")
        .ok()
        .filter(|name| !name.is_empty());
    let default_sink = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| format!("{}.monitor", id.id()));

    let mut fallback = None;
    for device in host.input_devices()? {
        let id = device.id()?;
        let source = id.id();
        if let Some(name) = &preferred {
            if source == name || device.to_string() == *name || id.to_string() == *name {
                return Ok((device, false));
            }
        } else if source.ends_with(".monitor") {
            if default_sink.as_deref() == Some(source) {
                return Ok((device, false));
            }
            if fallback.is_none() {
                fallback = Some(device);
            }
        }
    }
    if preferred.is_some() {
        return Err("requested PulseAudio monitor source does not exist".into());
    }
    fallback.map(|device| (device, false)).ok_or_else(|| {
        "no PulseAudio/PipeWire output monitor found (microphone capture is not a fallback)".into()
    })
}

#[cfg(any(windows, target_os = "macos"))]
fn output_device() -> CaptureResult<(cpal::Device, bool)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default system audio output device")?;
    Ok((device, true))
}

macro_rules! capture_stream {
    ($device:expr, $config:expr, $tx:expr, $errors:expr, $type:ty, $convert:expr) => {{
        let samples: SyncSender<Vec<f32>> = $tx.clone();
        let failures = $errors.clone();
        $device.build_input_stream::<$type, _, _>(
            $config,
            move |chunk: &[$type], _| {
                let converted = chunk.iter().map(|value| ($convert)(*value)).collect();
                let _ = samples.try_send(converted);
            },
            move |error| {
                let _ = failures.send(error.to_string());
            },
            None,
        )?
    }};
}

pub fn run(sender: broadcast::Sender<String>) -> CaptureResult<()> {
    let (device, loopback) = output_device()?;
    let format = if loopback {
        device.default_output_config()?
    } else {
        device.default_input_config()?
    };
    let rate = format.sample_rate();
    let channels = format.channels();
    let config = format.config();
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(8);
    let (error_tx, error_rx) = mpsc::channel::<String>();

    let stream = match format.sample_format() {
        SampleFormat::F32 => capture_stream!(device, config, tx, error_tx, f32, |value: f32| value),
        SampleFormat::F64 => {
            capture_stream!(device, config, tx, error_tx, f64, |value: f64| value as f32)
        }
        SampleFormat::I16 => capture_stream!(device, config, tx, error_tx, i16, |value: i16| value
            as f32
            / 32_768.),
        SampleFormat::I32 => capture_stream!(device, config, tx, error_tx, i32, |value: i32| value
            as f32
            / 2_147_483_648.),
        SampleFormat::U16 => {
            capture_stream!(device, config, tx, error_tx, u16, |value: u16| (value
                as f32
                - 32_768.)
                / 32_768.)
        }
        other => return Err(format!("unsupported system audio sample format: {other:?}").into()),
    };
    stream.play()?;
    eprintln!("Capturing system audio with CPAL: {device} ({rate} Hz, {channels} channels)");

    let mut pcm = PcmInput::new();
    let mut spectrum = Spectrum::new();
    loop {
        if let Ok(error) = error_rx.try_recv() {
            return Err(format!("CPAL stream stopped: {error}").into());
        }
        let samples = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(samples) => samples,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("CPAL audio callback disconnected".into());
            }
        };
        let mono = pcm.push(&samples, rate, channels)?;
        for bands in spectrum.push_mono(mono.as_ref()) {
            if sender.receiver_count() == 0 {
                continue;
            }
            let payload = format!(
                "{{\"bands\":[{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}]}}",
                bands[0], bands[1], bands[2], bands[3], bands[4], bands[5]
            );
            let _ = sender.send(payload);
        }
    }
}
