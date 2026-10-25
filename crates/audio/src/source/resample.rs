use crate::{CHANNELS, FRAME_VALUES, SAMPLE_RATE};

#[derive(Debug)]
pub struct Converter {
    step: f64,
    channels: usize,
    position: f64,
    previous: [f32; 2],
    pending: Vec<f32>,
}

impl Converter {
    #[must_use]
    pub fn new(rate: u32, channels: usize) -> Self {
        Self {
            step: f64::from(rate) / f64::from(SAMPLE_RATE),
            channels: channels.max(1),
            position: 0.0,
            previous: [0.0; 2],
            pending: Vec::with_capacity(FRAME_VALUES * 2),
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn push(&mut self, samples: &[f32], mut frame: impl FnMut(&[f32])) {
        for chunk in samples.chunks_exact(self.channels) {
            let left = chunk[0];
            let right = if self.channels > 1 { chunk[1] } else { left };
            while self.position < 1.0 {
                let t = self.position as f32;
                self.pending
                    .push(self.previous[0] + (left - self.previous[0]) * t);
                self.pending
                    .push(self.previous[1] + (right - self.previous[1]) * t);
                self.position += self.step;
            }
            self.position -= 1.0;
            self.previous = [left, right];
        }
        let whole = self.pending.len() / FRAME_VALUES * FRAME_VALUES;
        for block in self.pending[..whole].as_chunks::<FRAME_VALUES>().0 {
            frame(block);
        }
        self.pending.drain(..whole);
    }
}

const _: () = assert!(CHANNELS == 2);

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    fn collect(converter: &mut Converter, samples: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        converter.push(samples, |block| out.extend_from_slice(block));
        out
    }

    #[test]
    fn same_rate_stereo_passes_through_delayed_by_one_sample() {
        let mut converter = Converter::new(48_000, 2);
        let input: Vec<f32> = (0..FRAME_VALUES * 2 + 2)
            .map(|i| (i as f32) / 10_000.0)
            .collect();
        let out = collect(&mut converter, &input);
        assert_eq!(out.len(), FRAME_VALUES * 2);
        assert_eq!(&out[..2], &[0.0, 0.0]);
        assert!((out[2] - input[0]).abs() < 1e-6);
        assert!((out[3] - input[1]).abs() < 1e-6);
        assert!((out[1000] - input[998]).abs() < 1e-6);
    }

    #[test]
    fn mono_is_duplicated_to_both_channels() {
        let mut converter = Converter::new(48_000, 1);
        let input = vec![0.5f32; 960 * 3];
        let out = collect(&mut converter, &input);
        assert_eq!(out.len(), FRAME_VALUES * 3);
        assert!(
            out[10..]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| (pair[0] - pair[1]).abs() < 1e-6)
        );
    }

    #[test]
    fn surround_keeps_only_the_front_pair() {
        let mut converter = Converter::new(48_000, 6);
        let mut input = Vec::new();
        for _ in 0..960 * 2 {
            input.extend_from_slice(&[0.25, -0.25, 0.9, 0.9, 0.9, 0.9]);
        }
        let out = collect(&mut converter, &input);
        assert!(
            out[10..]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| (pair[0] - 0.25).abs() < 1e-6 && (pair[1] + 0.25).abs() < 1e-6)
        );
    }

    #[test]
    fn a_lower_rate_is_stretched_to_48_khz_keeping_the_pitch() {
        let mut converter = Converter::new(44_100, 2);
        let seconds = 1.0;
        let count = (44_100.0 * seconds) as usize;
        let input: Vec<f32> = (0..count)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44_100.0).sin() as f32;
                [v, v]
            })
            .collect();
        let out = collect(&mut converter, &input);
        let frames = out.len() / 2;
        assert!((frames as f64 - 48_000.0).abs() < 1_000.0, "{frames}");
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        assert!((crossings as f64 - 1000.0).abs() < 25.0, "{crossings}");
    }
}
