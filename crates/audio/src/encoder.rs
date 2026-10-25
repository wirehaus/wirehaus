use opus_pure::{Application, MAX_PACKET_BYTES, OpusEncoder};

use crate::error::AudioError;
use crate::{CHANNELS, FRAME_SAMPLES, FRAME_VALUES, SAMPLE_RATE};

const BITRATE: i32 = 64_000;

pub struct Encoder {
    inner: OpusEncoder,
    buffer: Vec<u8>,
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Encoder").finish_non_exhaustive()
    }
}

impl Encoder {
    pub fn new() -> Result<Self, AudioError> {
        let rate = i32::try_from(SAMPLE_RATE).map_err(|e| AudioError::Encode(e.to_string()))?;
        let mut inner = OpusEncoder::new(rate, CHANNELS, Application::Audio)
            .map_err(|e| AudioError::Encode(e.to_string()))?;
        inner.bitrate_bps = BITRATE;
        Ok(Self {
            inner,
            buffer: vec![0; MAX_PACKET_BYTES],
        })
    }

    pub fn encode(&mut self, frame: &[f32]) -> Result<Vec<u8>, AudioError> {
        if frame.len() != FRAME_VALUES {
            return Err(AudioError::Encode(format!(
                "a frame has {} values, expected {FRAME_VALUES}",
                frame.len()
            )));
        }
        let size = self
            .inner
            .encode(frame, FRAME_SAMPLES, &mut self.buffer)
            .map_err(|e| AudioError::Encode(e.to_string()))?;
        Ok(self.buffer[..size].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_possible_truncation)]
    fn tone(frames: usize, hertz: f32) -> Vec<f32> {
        (0..frames * FRAME_SAMPLES)
            .flat_map(|i| {
                let t = f64::from(u32::try_from(i).unwrap()) / f64::from(SAMPLE_RATE);
                let v = (0.4 * (2.0 * std::f64::consts::PI * f64::from(hertz) * t).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_tone_encodes_to_small_packets_and_decodes_with_the_same_pitch() {
        let mut encoder = Encoder::new().unwrap();
        let pcm = tone(50, 440.0);
        let mut decoder = opus_pure::OpusDecoder::new(48_000, 2).unwrap();
        let mut output = Vec::new();
        let mut frame_out = vec![0f32; FRAME_VALUES];
        for frame in pcm.as_chunks::<FRAME_VALUES>().0 {
            let packet = encoder.encode(frame).unwrap();
            assert!(!packet.is_empty() && packet.len() < 400, "{}", packet.len());
            decoder
                .decode(&packet, FRAME_SAMPLES, &mut frame_out)
                .unwrap();
            output.extend_from_slice(&frame_out);
        }
        let left: Vec<f32> = output.iter().step_by(2).copied().collect();
        let tail = &left[left.len() - 9600..];
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let measured = f64::from(u32::try_from(crossings).unwrap()) / 0.2;
        assert!((measured - 440.0).abs() < 15.0, "{measured}");
    }

    #[test]
    fn a_frame_of_the_wrong_size_is_rejected() {
        let mut encoder = Encoder::new().unwrap();
        assert!(encoder.encode(&[0.0; 10]).is_err());
    }

    #[test]
    fn silence_costs_almost_nothing() {
        let mut encoder = Encoder::new().unwrap();
        let silence = vec![0f32; FRAME_VALUES];
        let mut last = 0;
        for _ in 0..20 {
            last = encoder.encode(&silence).unwrap().len();
        }
        assert!(last < 20, "{last}");
    }
}
