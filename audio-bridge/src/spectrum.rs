use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;

pub const RATE: usize = 22_050;
pub const HOP: usize = 512;
const WINDOW: usize = 2048;
const BANDS: [f32; 7] = [55., 150., 350., 800., 1800., 4000., 9000.];
const REFERENCE_DB: [f32; 6] = [-19., -23., -29., -34., -39., -44.];

pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    ring: [f32; WINDOW],
    cursor: usize,
    filled: usize,
    since_fft: usize,
    frame: Vec<Complex32>,
    smoothed: [f32; 6],
}

impl Spectrum {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        Self {
            fft: planner.plan_fft_forward(WINDOW),
            hann: (0..WINDOW)
                .map(|i| {
                    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos()
                })
                .collect(),
            ring: [0.; WINDOW],
            cursor: 0,
            filled: 0,
            since_fft: 0,
            frame: vec![Complex32::new(0., 0.); WINDOW],
            smoothed: [0.; 6],
        }
    }

    pub fn push_mono(&mut self, samples: &[f32]) -> Vec<[f32; 6]> {
        let mut output = Vec::new();
        for &sample in samples {
            self.ring[self.cursor] = sample;
            self.cursor = (self.cursor + 1) % WINDOW;
            self.filled = (self.filled + 1).min(WINDOW);
            self.since_fft += 1;
            if self.filled == WINDOW && self.since_fft >= HOP {
                self.since_fft = 0;
                output.push(self.compute_bands());
            }
        }
        output
    }

    fn compute_bands(&mut self) -> [f32; 6] {
        for i in 0..WINDOW {
            self.frame[i] =
                Complex32::new(self.ring[(self.cursor + i) % WINDOW] * self.hann[i], 0.);
        }
        self.fft.process(&mut self.frame);

        let mut bands = [0.; 6];
        for (band, level) in bands.iter_mut().enumerate() {
            let lo = ((BANDS[band] * WINDOW as f32 / RATE as f32).floor() as usize).max(1);
            let hi =
                ((BANDS[band + 1] * WINDOW as f32 / RATE as f32).ceil() as usize).min(WINDOW / 2);
            let mut power = 0.;
            let mut count = 0;
            for value in self.frame.iter().take(hi).skip(lo) {
                power += value.norm_sqr();
                count += 1;
            }
            let rms = if count > 0 {
                (power / count as f32).sqrt() / (WINDOW as f32 * 0.25)
            } else {
                0.
            };
            let db = 20. * rms.max(1e-8).log10();
            let target = ((db - REFERENCE_DB[band] + 49.) / 49.)
                .clamp(0., 1.)
                .powf(1.25);
            let coefficient = if target > self.smoothed[band] {
                0.85
            } else {
                0.12
            };
            self.smoothed[band] += (target - self.smoothed[band]) * coefficient;
            *level = self.smoothed[band];
        }
        bands
    }
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frequency: f32, count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| (2. * std::f32::consts::PI * frequency * i as f32 / RATE as f32).sin() * 0.6)
            .collect()
    }

    #[test]
    fn silence_stays_silent_and_emits_at_the_original_cadence() {
        let mut spectrum = Spectrum::new();
        let frames = spectrum.push_mono(&[0.; WINDOW + 4 * HOP]);
        assert_eq!(frames.len(), 5);
        assert!(frames.iter().all(|bands| bands.iter().all(|level| *level == 0.)));
    }

    #[test]
    fn chunk_boundaries_do_not_change_the_spectrum() {
        let samples = sine(440., WINDOW + 9 * HOP);
        let whole = Spectrum::new().push_mono(&samples);
        let mut chunked = Spectrum::new();
        let parts: Vec<_> = samples
            .chunks(137)
            .flat_map(|chunk| chunked.push_mono(chunk))
            .collect();
        assert_eq!(whole, parts);
        assert_eq!(whole.len(), 10);
        let dominant = whole.last().unwrap();
        assert_eq!(
            dominant
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0,
            2
        );
    }

    #[test]
    fn small_chunks_wait_for_full_window() {
        let mut spectrum = Spectrum::new();
        for _ in 0..(WINDOW - 1) {
            assert!(spectrum.push_mono(&[0.5]).is_empty());
        }
        assert_eq!(spectrum.push_mono(&[0.5]).len(), 1);
        assert!(spectrum.push_mono(&[0.5; HOP - 1]).is_empty());
        assert_eq!(spectrum.push_mono(&[0.5]).len(), 1);
    }
}
