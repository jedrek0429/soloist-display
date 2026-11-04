use super::AudioChunk;
use anyhow::{bail, Result};
use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;

const RATE: u32 = 22_050;
const WINDOW: usize = 2048;
const HOP: usize = 512;
const BANDS: [f32; 7] = [55., 150., 350., 800., 1800., 4000., 9000.];
const REFERENCE_DB: [f32; 6] = [-19., -23., -29., -34., -39., -44.];

pub struct SpectrumProcessor {
    source_rate: u32,
    resample_buffer: Vec<f32>,
    resample_position: f64,
    ring: [f32; WINDOW],
    cursor: usize,
    filled: usize,
    since_fft: usize,
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    frame: Vec<Complex32>,
    smoothed: [f32; 6],
}

impl SpectrumProcessor {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        Self {
            source_rate: 0,
            resample_buffer: Vec::new(),
            resample_position: 0.,
            ring: [0.; WINDOW],
            cursor: 0,
            filled: 0,
            since_fft: 0,
            fft: planner.plan_fft_forward(WINDOW),
            hann: (0..WINDOW)
                .map(|i| {
                    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos()
                })
                .collect(),
            frame: vec![Complex32::new(0., 0.); WINDOW],
            smoothed: [0.; 6],
        }
    }

    pub fn push(&mut self, chunk: AudioChunk) -> Result<Vec<[f32; 6]>> {
        if chunk.sample_rate == 0 || chunk.channels == 0 {
            bail!("audio backend returned an invalid stream format");
        }
        let channels = usize::from(chunk.channels);
        if !chunk.samples.len().is_multiple_of(channels) {
            bail!("audio backend returned an incomplete interleaved frame");
        }
        if self.source_rate != chunk.sample_rate {
            self.source_rate = chunk.sample_rate;
            self.resample_buffer.clear();
            self.resample_position = 0.;
        }
        self.resample_buffer.extend(
            chunk
                .samples
                .chunks_exact(channels)
                .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32),
        );

        let step = self.source_rate as f64 / RATE as f64;
        let mut output = Vec::new();
        while self.resample_position + 1. < self.resample_buffer.len() as f64 {
            let left = self.resample_position.floor() as usize;
            let fraction = (self.resample_position - left as f64) as f32;
            let sample = self.resample_buffer[left]
                + (self.resample_buffer[left + 1] - self.resample_buffer[left]) * fraction;
            if let Some(bands) = self.push_sample(sample.clamp(-1., 1.)) {
                output.push(bands);
            }
            self.resample_position += step;
        }
        let consumed = (self.resample_position.floor() as usize).min(self.resample_buffer.len());
        if consumed > 0 {
            self.resample_buffer.drain(..consumed);
            self.resample_position -= consumed as f64;
        }
        Ok(output)
    }

    fn push_sample(&mut self, sample: f32) -> Option<[f32; 6]> {
        self.ring[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % WINDOW;
        self.filled = (self.filled + 1).min(WINDOW);
        self.since_fft += 1;
        if self.filled < WINDOW || self.since_fft < HOP {
            return None;
        }
        self.since_fft = 0;
        for i in 0..WINDOW {
            self.frame[i] =
                Complex32::new(self.ring[(self.cursor + i) % WINDOW] * self.hann[i], 0.);
        }
        self.fft.process(&mut self.frame);
        let mut bands = [0.; 6];
        for band in 0..6 {
            let lo = ((BANDS[band] * WINDOW as f32 / RATE as f32).floor() as usize).max(1);
            let hi =
                ((BANDS[band + 1] * WINDOW as f32 / RATE as f32).ceil() as usize).min(WINDOW / 2);
            let count = hi.saturating_sub(lo);
            let power = self.frame[lo..hi]
                .iter()
                .map(Complex32::norm_sqr)
                .sum::<f32>();
            let rms = if count == 0 {
                0.
            } else {
                (power / count as f32).sqrt() / (WINDOW as f32 * 0.25)
            };
            let db = 20. * rms.max(1e-8).log10();
            let level = ((db - REFERENCE_DB[band] + 49.) / 49.)
                .clamp(0., 1.)
                .powf(1.25);
            let coefficient = if level > self.smoothed[band] {
                0.85
            } else {
                0.12
            };
            self.smoothed[band] += (level - self.smoothed[band]) * coefficient;
        }
        bands.copy_from_slice(&self.smoothed);
        Some(bands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, channels: u16, frequency: f32, seconds: f32) -> AudioChunk {
        let frames = (rate as f32 * seconds) as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for frame in 0..frames {
            let sample =
                (2. * std::f32::consts::PI * frequency * frame as f32 / rate as f32).sin() * 0.6;
            samples.extend(std::iter::repeat_n(sample, usize::from(channels)));
        }
        AudioChunk {
            samples,
            sample_rate: rate,
            channels,
        }
    }

    #[test]
    fn resamples_stereo_and_emits_six_finite_bands() {
        let mut processor = SpectrumProcessor::new();
        let frames = processor.push(sine(48_000, 2, 440., 0.5)).unwrap();
        assert!(!frames.is_empty());
        assert!(frames.iter().flatten().all(|value| value.is_finite()));
        assert!(frames
            .iter()
            .flatten()
            .all(|value| (0. ..=1.).contains(value)));
    }

    #[test]
    fn dominant_frequency_lands_in_expected_band() {
        let mut processor = SpectrumProcessor::new();
        let frames = processor.push(sine(44_100, 1, 440., 1.)).unwrap();
        let bands = frames.last().unwrap();
        assert_eq!(
            bands
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0,
            2
        );
    }

    #[test]
    fn resamples_short_48khz_chunks_without_overrunning_the_buffer() {
        let mut processor = SpectrumProcessor::new();
        let mut frames = 0;
        for _ in 0..300 {
            let output = processor
                .push(AudioChunk {
                    samples: vec![0.25; 480],
                    sample_rate: 48_000,
                    channels: 1,
                })
                .unwrap();
            frames += output.len();
            assert!(output
                .iter()
                .flatten()
                .all(|level| level.is_finite() && (0. ..=1.).contains(level)));
        }
        assert!(frames > 0);
    }

    #[test]
    fn rejects_incomplete_frames() {
        let mut processor = SpectrumProcessor::new();
        assert!(processor
            .push(AudioChunk {
                samples: vec![0.],
                sample_rate: 48_000,
                channels: 2,
            })
            .is_err());
    }
}
