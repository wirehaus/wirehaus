use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, FrameType, Profile, RateControlMode,
    UsageType, VuiConfig,
};
use openh264::formats::YUVSource;
use wirehaus_capture::Bitmap;

use crate::error::EncodeError;

pub const MIN_DIMENSION: u16 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedVideo {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub width: u16,
    pub height: u16,
}

struct I420 {
    width: usize,
    height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl I420 {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            y: vec![0; width * height],
            u: vec![0; width * height / 4],
            v: vec![0; width * height / 4],
        }
    }

    fn fill(&mut self, bitmap: &Bitmap) {
        let source_stride = usize::from(bitmap.width()) * 4;
        let pixels = bitmap.bgra();
        for row in 0..self.height {
            for column in 0..self.width {
                let at = row * source_stride + column * 4;
                let (b, g, r) = (
                    i32::from(pixels[at]),
                    i32::from(pixels[at + 1]),
                    i32::from(pixels[at + 2]),
                );
                self.y[row * self.width + column] = luma(r, g, b);
            }
        }
        let chroma_width = self.width / 2;
        for row in 0..self.height / 2 {
            for column in 0..chroma_width {
                let (mut r, mut g, mut b) = (0, 0, 0);
                for dy in 0..2 {
                    for dx in 0..2 {
                        let at = (row * 2 + dy) * source_stride + (column * 2 + dx) * 4;
                        b += i32::from(pixels[at]);
                        g += i32::from(pixels[at + 1]);
                        r += i32::from(pixels[at + 2]);
                    }
                }
                let (r, g, b) = ((r + 2) / 4, (g + 2) / 4, (b + 2) / 4);
                self.u[row * chroma_width + column] =
                    clamp((-26 * r - 86 * g + 112 * b + 128) >> 8, 128);
                self.v[row * chroma_width + column] =
                    clamp((112 * r - 102 * g - 10 * b + 128) >> 8, 128);
            }
        }
    }
}

fn luma(r: i32, g: i32, b: i32) -> u8 {
    clamp((47 * r + 157 * g + 16 * b + 128) >> 8, 16)
}

fn clamp(value: i32, offset: i32) -> u8 {
    u8::try_from((value + offset).clamp(0, 255)).unwrap_or(u8::MAX)
}

impl YUVSource for I420 {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}

pub struct VideoEncoder {
    encoder: Encoder,
    frame: I420,
    width: u16,
    height: u16,
    bitrate_kbps: u32,
}

impl std::fmt::Debug for VideoEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoEncoder")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bitrate_kbps", &self.bitrate_kbps)
            .finish_non_exhaustive()
    }
}

impl VideoEncoder {
    pub fn new(
        width: u16,
        height: u16,
        bitrate_kbps: u32,
        frame_rate: f32,
    ) -> Result<Self, EncodeError> {
        let (width, height) = (width & !1, height & !1);
        if width < MIN_DIMENSION || height < MIN_DIMENSION {
            return Err(EncodeError::Video(format!(
                "a {width}x{height} picture is too small to encode"
            )));
        }
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_kbps.saturating_mul(1000)))
            .max_frame_rate(FrameRate::from_hz(frame_rate))
            .skip_frames(false)
            .profile(Profile::Baseline)
            .complexity(Complexity::Low)
            .vui(VuiConfig::bt709());
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| EncodeError::Video(e.to_string()))?;
        Ok(Self {
            encoder,
            frame: I420::new(usize::from(width), usize::from(height)),
            width,
            height,
            bitrate_kbps,
        })
    }

    #[must_use]
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    #[must_use]
    pub fn bitrate_kbps(&self) -> u32 {
        self.bitrate_kbps
    }

    pub fn encode(
        &mut self,
        bitmap: &Bitmap,
        force_keyframe: bool,
    ) -> Result<Option<EncodedVideo>, EncodeError> {
        if bitmap.width() & !1 != self.width || bitmap.height() & !1 != self.height {
            return Err(EncodeError::Video(format!(
                "the picture is {}x{} but the encoder was created for {}x{}",
                bitmap.width(),
                bitmap.height(),
                self.width,
                self.height
            )));
        }
        self.frame.fill(bitmap);
        if force_keyframe {
            self.encoder.force_intra_frame();
        }
        let stream = self
            .encoder
            .encode(&self.frame)
            .map_err(|e| EncodeError::Video(e.to_string()))?;
        let keyframe = match stream.frame_type() {
            FrameType::IDR | FrameType::I => true,
            FrameType::P | FrameType::IPMixed => false,
            FrameType::Skip | FrameType::Invalid => return Ok(None),
        };
        let data = stream.to_vec();
        if data.is_empty() {
            return Ok(None);
        }
        Ok(Some(EncodedVideo {
            data,
            keyframe,
            width: self.width,
            height: self.height,
        }))
    }
}

#[cfg(test)]
mod tests {
    use openh264::decoder::Decoder;

    use super::*;

    fn solid(width: u16, height: u16, bgra: [u8; 4]) -> Bitmap {
        let data = bgra
            .iter()
            .copied()
            .cycle()
            .take(usize::from(width) * usize::from(height) * 4)
            .collect();
        Bitmap::new(width, height, data).unwrap()
    }

    fn gradient(width: u16, height: u16, shift: u16) -> Bitmap {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let value = u8::try_from((x + y + shift) % 256).unwrap();
                data.extend_from_slice(&[value, value.wrapping_mul(2), 255 - value, 255]);
            }
        }
        Bitmap::new(width, height, data).unwrap()
    }

    fn nal_types(data: &[u8]) -> Vec<u8> {
        let mut types = Vec::new();
        let mut at = 0;
        while at + 3 < data.len() {
            if data[at..at + 3] == [0, 0, 1] {
                types.push(data[at + 3] & 0x1F);
                at += 3;
            } else {
                at += 1;
            }
        }
        types
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn decode_center(data: &[u8]) -> ((usize, usize), [u8; 3]) {
        let mut decoder = Decoder::new().unwrap();
        let picture = decoder.decode(data).unwrap().expect("a decoded picture");
        let (width, height) = picture.dimensions();
        let (luma_stride, chroma_stride, _) = picture.strides();
        let (row, column) = (height / 2, width / 2);
        let luma = (f64::from(picture.y()[row * luma_stride + column]) - 16.0) / 219.0;
        let blue_difference =
            (f64::from(picture.u()[(row / 2) * chroma_stride + column / 2]) - 128.0) / 224.0;
        let red_difference =
            (f64::from(picture.v()[(row / 2) * chroma_stride + column / 2]) - 128.0) / 224.0;
        let red = luma + 1.5748 * red_difference;
        let green = luma - 0.1873 * blue_difference - 0.4681 * red_difference;
        let blue = luma + 1.8556 * blue_difference;
        let to_byte = |value: f64| (value * 255.0).round().clamp(0.0, 255.0) as u8;
        (
            (width, height),
            [to_byte(red), to_byte(green), to_byte(blue)],
        )
    }

    fn close(actual: [u8; 3], expected: [u8; 3]) -> bool {
        actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 8)
    }

    #[test]
    fn converts_known_colours_to_bt709_limited_range() {
        let mut frame = I420::new(2, 2);
        frame.fill(&solid(2, 2, [255, 255, 255, 255]));
        assert_eq!((frame.y[0], frame.u[0], frame.v[0]), (235, 128, 128));
        frame.fill(&solid(2, 2, [0, 0, 0, 255]));
        assert_eq!((frame.y[0], frame.u[0], frame.v[0]), (16, 128, 128));
        frame.fill(&solid(2, 2, [0, 0, 255, 255]));
        assert!(frame.y[0].abs_diff(63) <= 1, "{}", frame.y[0]);
        assert!(frame.v[0] > 230, "{}", frame.v[0]);
    }

    #[test]
    fn the_first_picture_is_a_self_contained_keyframe_in_annex_b() {
        let mut encoder = VideoEncoder::new(128, 64, 2000, 30.0).unwrap();
        let first = encoder
            .encode(&solid(128, 64, [200, 40, 20, 255]), false)
            .unwrap()
            .unwrap();
        assert!(first.keyframe);
        assert!(first.data.starts_with(&[0, 0, 0, 1]) || first.data.starts_with(&[0, 0, 1]));
        let types = nal_types(&first.data);
        assert!(types.contains(&7), "no SPS in {types:?}");
        assert!(types.contains(&8), "no PPS in {types:?}");
        assert!(types.contains(&5), "no IDR slice in {types:?}");
        assert_eq!((first.width, first.height), (128, 64));
    }

    #[test]
    fn what_is_encoded_decodes_to_the_same_colour() {
        for bgra in [
            [200, 40, 20, 255],
            [30, 160, 220, 255],
            [250, 250, 250, 255],
            [10, 10, 10, 255],
        ] {
            let mut encoder = VideoEncoder::new(128, 64, 2000, 30.0).unwrap();
            let first = encoder
                .encode(&solid(128, 64, bgra), false)
                .unwrap()
                .unwrap();
            let (dimensions, rgb) = decode_center(&first.data);
            assert_eq!(dimensions, (128, 64));
            assert!(
                close(rgb, [bgra[2], bgra[1], bgra[0]]),
                "{rgb:?} vs {bgra:?}"
            );
        }
    }

    #[test]
    fn an_unchanged_picture_costs_almost_nothing() {
        let mut encoder = VideoEncoder::new(256, 128, 4000, 30.0).unwrap();
        let picture = gradient(256, 128, 0);
        let first = encoder.encode(&picture, false).unwrap().unwrap();
        let second = encoder.encode(&picture, false).unwrap();
        let second = second.map_or(0, |video| video.data.len());
        assert!(
            second * 20 < first.data.len(),
            "{second} vs {}",
            first.data.len()
        );
    }

    #[test]
    fn later_pictures_are_not_keyframes_until_one_is_forced() {
        let mut encoder = VideoEncoder::new(128, 64, 2000, 30.0).unwrap();
        encoder
            .encode(&gradient(128, 64, 0), false)
            .unwrap()
            .unwrap();
        let delta = encoder
            .encode(&gradient(128, 64, 7), false)
            .unwrap()
            .unwrap();
        assert!(!delta.keyframe);
        let forced = encoder
            .encode(&gradient(128, 64, 9), true)
            .unwrap()
            .unwrap();
        assert!(forced.keyframe);
        assert!(nal_types(&forced.data).contains(&7));
    }

    #[test]
    fn a_stream_of_pictures_decodes_picture_by_picture() {
        let mut source = VideoEncoder::new(128, 64, 3000, 30.0).unwrap();
        let mut sink = Decoder::new().unwrap();
        let mut shown = 0;
        for shift in 0..12 {
            let output = source.encode(&gradient(128, 64, shift * 5), false).unwrap();
            let picture = output.and_then(|video| sink.decode(&video.data).unwrap().map(|_| ()));
            shown += usize::from(picture.is_some());
        }
        assert!(shown >= 10, "{shown}");
    }

    #[test]
    fn odd_dimensions_are_rounded_down_to_even() {
        let mut encoder = VideoEncoder::new(129, 65, 2000, 30.0).unwrap();
        assert_eq!(encoder.dimensions(), (128, 64));
        let video = encoder
            .encode(&solid(129, 65, [1, 2, 3, 255]), false)
            .unwrap()
            .unwrap();
        assert_eq!((video.width, video.height), (128, 64));
    }

    #[test]
    fn tiny_pictures_are_refused() {
        assert!(matches!(
            VideoEncoder::new(8, 8, 1000, 30.0),
            Err(EncodeError::Video(_))
        ));
        assert!(VideoEncoder::new(MIN_DIMENSION, MIN_DIMENSION, 1000, 30.0).is_ok());
    }

    #[test]
    fn a_picture_of_another_size_is_refused() {
        let mut encoder = VideoEncoder::new(128, 64, 2000, 30.0).unwrap();
        assert!(encoder.encode(&solid(64, 64, [0; 4]), false).is_err());
    }

    #[test]
    fn a_lower_bitrate_produces_smaller_streams() {
        let size = |kbps: u32| {
            let mut encoder = VideoEncoder::new(320, 180, kbps, 30.0).unwrap();
            let mut total = 0;
            for shift in 0..30 {
                total += encoder
                    .encode(&gradient(320, 180, shift * 13), false)
                    .unwrap()
                    .map_or(0, |video| video.data.len());
            }
            total
        };
        assert!(size(300) < size(6000));
    }
}
