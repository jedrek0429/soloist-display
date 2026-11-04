use super::spectrum::RATE;
use std::{borrow::Cow, error::Error, fmt};

#[derive(Debug, PartialEq, Eq)]
pub enum PcmError {
    InvalidFormat,
    IncompleteFrame,
    NonFiniteSample,
}

impl fmt::Display for PcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "audio stream has zero sample rate or channels"),
            Self::IncompleteFrame => write!(f, "audio chunk ends with an incomplete frame"),
            Self::NonFiniteSample => write!(f, "audio stream contains a non-finite sample"),
        }
    }
}

impl Error for PcmError {}

#[derive(Default)]
pub struct PcmInput {
    format: Option<(u32, u16)>,
    previous: Option<f32>,
    source_index: u64,
    output_index: u64,
}

impl PcmInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<'a>(
        &mut self,
        samples: &'a [f32],
        sample_rate: u32,
        channels: u16,
    ) -> Result<Cow<'a, [f32]>, PcmError> {
        if sample_rate == 0 || channels == 0 {
            return Err(PcmError::InvalidFormat);
        }
        let channels = usize::from(channels);
        if !samples.len().is_multiple_of(channels) {
            return Err(PcmError::IncompleteFrame);
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(PcmError::NonFiniteSample);
        }

        let format = (sample_rate, channels as u16);
        if self.format != Some(format) {
            self.format = Some(format);
            self.previous = None;
            self.source_index = 0;
            self.output_index = 0;
        }

        if sample_rate == RATE as u32 && channels == 1 {
            return Ok(Cow::Borrowed(samples));
        }

        let mut output = Vec::new();
        for frame in samples.chunks_exact(channels) {
            let mono = (frame.iter().copied().sum::<f32>() / channels as f32).clamp(-1., 1.);
            if sample_rate == RATE as u32 {
                output.push(mono);
                continue;
            }

            let Some(previous) = self.previous else {
                output.push(mono);
                self.output_index = 1;
                self.previous = Some(mono);
                continue;
            };
            self.source_index += 1;
            let right = self.source_index as u128 * RATE as u128;
            let left = (self.source_index - 1) as u128 * RATE as u128;
            while self.output_index as u128 * sample_rate as u128 <= right {
                let position = self.output_index as u128 * sample_rate as u128;
                let fraction = (position - left) as f32 / RATE as f32;
                output.push((previous + (mono - previous) * fraction).clamp(-1., 1.));
                self.output_index += 1;
            }
            self.previous = Some(mono);
        }
        Ok(Cow::Owned(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, frequency: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|frame| {
                (2. * std::f32::consts::PI * frequency * frame as f32 / rate as f32).sin() * 0.6
            })
            .collect()
    }

    #[test]
    fn target_rate_mono_is_borrowed_without_modification() {
        let mut input = PcmInput::new();
        let samples = [0., -0.25, 0.5, 1.];
        let converted = input.push(&samples, RATE as u32, 1).unwrap();
        assert!(matches!(converted, Cow::Borrowed(_)));
        assert_eq!(converted.as_ref(), &samples);
    }

    #[test]
    fn downmixes_stereo_at_target_rate() {
        let mut input = PcmInput::new();
        let samples = [1., -1., 0.5, 0.5, -0.25, -0.75];
        assert_eq!(
            input.push(&samples, RATE as u32, 2).unwrap().as_ref(),
            &[0., 0.5, -0.5]
        );
    }

    #[test]
    fn conversion_preserves_time_and_tone_at_48khz() {
        let source = tone(48_000, 440., 48_000);
        let mut input = PcmInput::new();
        let converted = input.push(&source, 48_000, 1).unwrap();
        assert_eq!(converted.len(), RATE);
        for (i, &sample) in converted.iter().enumerate() {
            let expected =
                (2. * std::f32::consts::PI * 440. * i as f32 / RATE as f32).sin() * 0.6;
            assert!((sample - expected).abs() < 0.003, "sample {i} drifted");
        }
    }

    #[test]
    fn arbitrary_frame_boundaries_produce_identical_output() {
        let mono = tone(44_100, 600., 4410);
        let stereo: Vec<f32> = mono.iter().flat_map(|&sample| [sample, sample]).collect();
        let mut whole_input = PcmInput::new();
        let whole = whole_input.push(&stereo, 44_100, 2).unwrap().into_owned();
        let mut chunked_input = PcmInput::new();
        let mut chunked = Vec::new();
        for chunk in stereo.chunks(14) {
            chunked.extend_from_slice(chunked_input.push(chunk, 44_100, 2).unwrap().as_ref());
        }
        assert_eq!(whole, chunked);
        assert_eq!(whole.len(), RATE / 10);
    }

    #[test]
    fn single_frame_chunks_and_upsampling_do_not_stall() {
        let mut input = PcmInput::new();
        let mut output = Vec::new();
        for frame in 0..11025 {
            output.extend(input.push(&[(frame % 2) as f32], 11_025, 1).unwrap().into_owned());
        }
        assert_eq!(output.len(), 22_049);
        assert!(output.iter().all(|sample| (0. ..=1.).contains(sample)));
    }

    #[test]
    fn format_changes_reset_resampling_phase() {
        let mut input = PcmInput::new();
        input.push(&[0.; 480], 48_000, 1).unwrap();
        assert_eq!(input.push(&[0.75], 44_100, 1).unwrap().as_ref(), &[0.75]);
        assert_eq!(input.push(&[0.25], 48_000, 1).unwrap().as_ref(), &[0.25]);
    }

    #[test]
    fn rejects_invalid_chunks_without_changing_the_stream() {
        let mut input = PcmInput::new();
        assert_eq!(input.push(&[0.], 0, 1).unwrap_err(), PcmError::InvalidFormat);
        assert_eq!(input.push(&[0.], 48_000, 0).unwrap_err(), PcmError::InvalidFormat);
        assert_eq!(
            input.push(&[0.], 48_000, 2).unwrap_err(),
            PcmError::IncompleteFrame
        );
        assert_eq!(
            input.push(&[f32::NAN], 48_000, 1).unwrap_err(),
            PcmError::NonFiniteSample
        );
        assert_eq!(input.push(&[0.5], 48_000, 1).unwrap().as_ref(), &[0.5]);
    }
}
