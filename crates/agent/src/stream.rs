use wirehaus_capture::{FrameBuffer, Rect, Region, Update, all_tiles, coalesce};
use wirehaus_encode::{JpegEncoder, TileEncoder, VideoEncoder};
use wirehaus_protocol::{Channel, DesktopMessage, Frame, ProtocolError};

use crate::error::ServerError;
use crate::link::FrameSink;
use crate::mode::{Mode, ModePolicy, Transition};

const VIDEO_FRAME_RATE: f32 = 30.0;
const KBPS_PER_QUALITY_POINT: u32 = 100;

#[derive(Debug)]
enum VideoOutcome {
    Sent(usize),
    Nothing,
    Failed,
}

#[derive(Debug)]
pub struct Stream {
    jpeg: JpegEncoder,
    video: Option<VideoEncoder>,
    frames: FrameBuffer,
    policy: ModePolicy,
    viewer_decodes_video: bool,
    video_disabled: bool,
}

impl Stream {
    #[must_use]
    pub fn new(jpeg_quality: u8) -> Self {
        Self {
            jpeg: JpegEncoder::new(jpeg_quality),
            video: None,
            frames: FrameBuffer::new(),
            policy: ModePolicy::new(),
            viewer_decodes_video: false,
            video_disabled: false,
        }
    }

    pub fn set_viewer_decodes_video(&mut self, decodes: bool) {
        self.viewer_decodes_video = decodes;
    }

    #[cfg(test)]
    fn mode(&self) -> Mode {
        self.policy.mode()
    }

    pub fn send(
        &mut self,
        sink: &mut impl FrameSink,
        update: &Update,
        quality: u8,
    ) -> Result<Option<usize>, ServerError> {
        self.frames.apply(update)?;
        self.jpeg.set_quality(quality);
        let mut bytes = 0;
        let mut sent = false;
        if update.resized {
            bytes += send_message(
                sink,
                &DesktopMessage::Resize {
                    width: update.width,
                    height: update.height,
                },
            )?;
            sent = true;
            self.video = None;
        }
        let transition = self.next_transition(update);
        let mut refresh = transition == Transition::ToTiles;
        if self.policy.mode() == Mode::Video {
            let wanted = update.resized || transition == Transition::ToVideo;
            if wanted || !update.regions.is_empty() {
                match self.send_video(sink, quality, wanted)? {
                    VideoOutcome::Sent(count) => {
                        bytes += count;
                        sent = true;
                    }
                    VideoOutcome::Nothing => {}
                    VideoOutcome::Failed => {
                        self.disable_video();
                        refresh = true;
                    }
                }
            }
        } else if !refresh {
            for region in &update.regions {
                bytes += self.send_region(sink, region)?;
                sent = true;
            }
        }
        if refresh {
            bytes += self.send_whole_picture(sink)?;
            sent = true;
        }
        Ok(sent.then_some(bytes))
    }

    fn next_transition(&mut self, update: &Update) -> Transition {
        if !self.viewer_decodes_video || self.video_disabled {
            if self.policy.mode() == Mode::Video {
                self.policy.reset();
                return Transition::ToTiles;
            }
            return Transition::None;
        }
        if update.resized {
            self.policy.reset();
            return Transition::None;
        }
        self.policy.observe(changed_fraction(update))
    }

    fn disable_video(&mut self) {
        self.video_disabled = true;
        self.video = None;
        self.policy.reset();
    }

    fn send_video(
        &mut self,
        sink: &mut impl FrameSink,
        quality: u8,
        mut keyframe: bool,
    ) -> Result<VideoOutcome, ServerError> {
        let Some(picture) = self.frames.frame() else {
            return Ok(VideoOutcome::Nothing);
        };
        let (width, height) = (picture.width(), picture.height());
        let bitrate = u32::from(quality) * KBPS_PER_QUALITY_POINT;
        let stale = self.video.as_ref().is_none_or(|encoder| {
            encoder.dimensions() != (width & !1, height & !1)
                || encoder.bitrate_kbps().abs_diff(bitrate) * 2 > encoder.bitrate_kbps()
        });
        if stale {
            match VideoEncoder::new(width, height, bitrate, VIDEO_FRAME_RATE) {
                Ok(encoder) => self.video = Some(encoder),
                Err(_) => return Ok(VideoOutcome::Failed),
            }
            keyframe = true;
        }
        let Some(codec) = self.video.as_mut() else {
            return Ok(VideoOutcome::Failed);
        };
        let output = match codec.encode(picture, keyframe) {
            Ok(Some(output)) => output,
            Ok(None) => return Ok(VideoOutcome::Nothing),
            Err(_) => return Ok(VideoOutcome::Failed),
        };
        let message = DesktopMessage::Video {
            width: output.width,
            height: output.height,
            keyframe: output.keyframe,
            data: output.data,
        };
        match Frame::new(Channel::Desktop, 0, message.encode()) {
            Ok(frame) => Ok(VideoOutcome::Sent(sink.send_frame(&frame)?)),
            Err(ProtocolError::PayloadTooLarge { .. }) => Ok(VideoOutcome::Failed),
            Err(error) => Err(error.into()),
        }
    }

    fn send_whole_picture(&mut self, sink: &mut impl FrameSink) -> Result<usize, ServerError> {
        let Some(picture) = self.frames.frame() else {
            return Ok(0);
        };
        let region = Region {
            rect: Rect {
                x: 0,
                y: 0,
                width: picture.width(),
                height: picture.height(),
            },
            bitmap: picture.clone(),
        };
        self.send_region(sink, &region)
    }

    fn send_region(
        &mut self,
        sink: &mut impl FrameSink,
        region: &Region,
    ) -> Result<usize, ServerError> {
        coalesce(&all_tiles(&region.bitmap))
            .into_iter()
            .try_fold(0, |total, local| {
                Ok(total + self.send_rect(sink, region, local)?)
            })
    }

    fn send_rect(
        &mut self,
        sink: &mut impl FrameSink,
        region: &Region,
        local: Rect,
    ) -> Result<usize, ServerError> {
        let message = DesktopMessage::Tile {
            x: region.rect.x.saturating_add(local.x),
            y: region.rect.y.saturating_add(local.y),
            width: local.width,
            height: local.height,
            codec: self.jpeg.codec(),
            data: self.jpeg.encode(&region.bitmap, local)?,
        };
        match Frame::new(Channel::Desktop, 0, message.encode()) {
            Ok(frame) => sink.send_frame(&frame),
            Err(ProtocolError::PayloadTooLarge { .. }) => match split(local) {
                Some(halves) => halves.into_iter().try_fold(0, |total, half| {
                    Ok(total + self.send_rect(sink, region, half)?)
                }),
                None => Err(ProtocolError::PayloadTooLarge {
                    len: usize::MAX,
                    max: wirehaus_protocol::MAX_PAYLOAD_LEN,
                }
                .into()),
            },
            Err(error) => Err(error.into()),
        }
    }
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn changed_fraction(update: &Update) -> f32 {
    let screen = u64::from(update.width) * u64::from(update.height);
    if screen == 0 {
        return 0.0;
    }
    let changed: u64 = update
        .regions
        .iter()
        .map(|region| u64::from(region.rect.width) * u64::from(region.rect.height))
        .sum();
    (changed as f64 / screen as f64).min(1.0) as f32
}

pub fn send_message(
    sink: &mut impl FrameSink,
    message: &DesktopMessage,
) -> Result<usize, ServerError> {
    sink.send_frame(&Frame::new(Channel::Desktop, 0, message.encode())?)
}

fn split(rect: Rect) -> Option<[Rect; 2]> {
    if rect.width > 1 {
        let left = rect.width / 2;
        Some([
            Rect {
                width: left,
                ..rect
            },
            Rect {
                x: rect.x + left,
                width: rect.width - left,
                ..rect
            },
        ])
    } else if rect.height > 1 {
        let top = rect.height / 2;
        Some([
            Rect {
                height: top,
                ..rect
            },
            Rect {
                y: rect.y + top,
                height: rect.height - top,
                ..rect
            },
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use openh264::decoder::Decoder;
    use wirehaus_capture::Bitmap;

    use super::*;
    use crate::mode::{ENTER_HITS, LEAVE_TICKS};

    #[derive(Default)]
    struct Collector {
        frames: Vec<DesktopMessage>,
    }

    impl FrameSink for Collector {
        fn send_frame(&mut self, frame: &Frame) -> Result<usize, ServerError> {
            self.frames
                .push(DesktopMessage::decode(&frame.payload).unwrap());
            Ok(frame.payload.len())
        }
    }

    fn picture(width: u16, height: u16, seed: u8) -> Bitmap {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let value = u8::try_from((x + y + u16::from(seed) * 7) % 256).unwrap();
                data.extend_from_slice(&[value, value / 2, 255 - value, 255]);
            }
        }
        Bitmap::new(width, height, data).unwrap()
    }

    fn whole(width: u16, height: u16, seed: u8) -> Update {
        Update {
            width,
            height,
            resized: true,
            regions: vec![Region {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                bitmap: picture(width, height, seed),
            }],
        }
    }

    fn busy(width: u16, height: u16, seed: u8) -> Update {
        let rows = height * 3 / 4;
        Update {
            width,
            height,
            resized: false,
            regions: vec![Region {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width,
                    height: rows,
                },
                bitmap: Bitmap::new(
                    width,
                    rows,
                    picture(width, height, seed).crop(Rect {
                        x: 0,
                        y: 0,
                        width,
                        height: rows,
                    }),
                )
                .unwrap(),
            }],
        }
    }

    fn quiet(width: u16, height: u16) -> Update {
        Update {
            width,
            height,
            resized: false,
            regions: vec![],
        }
    }

    fn videos(sink: &Collector) -> Vec<(u16, u16, bool, usize)> {
        sink.frames
            .iter()
            .filter_map(|message| match message {
                DesktopMessage::Video {
                    width,
                    height,
                    keyframe,
                    data,
                } => Some((*width, *height, *keyframe, data.len())),
                _ => None,
            })
            .collect()
    }

    fn tile_area(frames: &[DesktopMessage]) -> u64 {
        frames
            .iter()
            .map(|message| match message {
                DesktopMessage::Tile { width, height, .. } => {
                    u64::from(*width) * u64::from(*height)
                }
                _ => 0,
            })
            .sum()
    }

    fn capable() -> Stream {
        let mut stream = Stream::new(70);
        stream.set_viewer_decodes_video(true);
        stream
    }

    fn drive_into_video(stream: &mut Stream, sink: &mut Collector, width: u16, height: u16) {
        stream.send(sink, &whole(width, height, 0), 70).unwrap();
        for seed in 1..=u8::try_from(ENTER_HITS + 2).unwrap() {
            stream.send(sink, &busy(width, height, seed), 70).unwrap();
        }
    }

    #[test]
    fn the_first_picture_goes_out_as_a_resize_and_tiles() {
        let mut stream = capable();
        let mut sink = Collector::default();
        let bytes = stream.send(&mut sink, &whole(128, 64, 0), 70).unwrap();
        assert!(bytes.is_some_and(|count| count > 0));
        assert!(matches!(
            sink.frames[0],
            DesktopMessage::Resize {
                width: 128,
                height: 64
            }
        ));
        assert_eq!(tile_area(&sink.frames), 128 * 64);
        assert_eq!(videos(&sink), []);
        assert_eq!(stream.mode(), Mode::Tiles);
    }

    #[test]
    fn an_update_without_changes_sends_nothing() {
        let mut stream = capable();
        let mut sink = Collector::default();
        stream.send(&mut sink, &whole(128, 64, 0), 70).unwrap();
        let before = sink.frames.len();
        assert_eq!(stream.send(&mut sink, &quiet(128, 64), 70).unwrap(), None);
        assert_eq!(sink.frames.len(), before);
    }

    #[test]
    fn small_changes_stay_as_tiles() {
        let mut stream = capable();
        let mut sink = Collector::default();
        stream.send(&mut sink, &whole(128, 64, 0), 70).unwrap();
        let small = Update {
            width: 128,
            height: 64,
            resized: false,
            regions: vec![Region {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 16,
                    height: 16,
                },
                bitmap: picture(16, 16, 3),
            }],
        };
        for _ in 0..50 {
            stream.send(&mut sink, &small, 70).unwrap();
        }
        assert_eq!(videos(&sink), []);
        assert_eq!(stream.mode(), Mode::Tiles);
    }

    #[test]
    fn sustained_activity_switches_a_capable_viewer_to_video_starting_with_a_keyframe() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        assert_eq!(stream.mode(), Mode::Video);
        let sent = videos(&sink);
        assert!(sent.len() >= 2, "{sent:?}");
        assert!(sent[0].2, "the first video picture must be a keyframe");
        assert!(sent[1..].iter().all(|video| !video.2));
        assert!(sent.iter().all(|video| (video.0, video.1) == (128, 64)));
    }

    #[test]
    fn the_video_stream_is_decodable_by_a_standard_decoder() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        let mut viewer = Decoder::new().unwrap();
        let mut pictures = 0;
        for message in &sink.frames {
            if let DesktopMessage::Video { data, .. } = message
                && let Some(shown) = viewer.decode(data).unwrap()
            {
                use openh264::formats::YUVSource;
                assert_eq!(shown.dimensions(), (128, 64));
                pictures += 1;
            }
        }
        assert!(pictures >= 2, "{pictures}");
    }

    #[test]
    fn a_viewer_without_video_support_never_gets_video() {
        let mut stream = Stream::new(70);
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        for seed in 20..60 {
            stream.send(&mut sink, &busy(128, 64, seed), 70).unwrap();
        }
        assert_eq!(videos(&sink), []);
        assert_eq!(stream.mode(), Mode::Tiles);
    }

    #[test]
    fn a_long_calm_returns_to_tiles_with_one_full_refresh() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        let before = sink.frames.len();
        for _ in 0..LEAVE_TICKS {
            stream.send(&mut sink, &quiet(128, 64), 70).unwrap();
        }
        assert_eq!(stream.mode(), Mode::Tiles);
        let after = &sink.frames[before..];
        assert_eq!(tile_area(after), 128 * 64);
    }

    #[test]
    fn entering_video_again_after_tiles_always_starts_with_a_keyframe() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        for _ in 0..LEAVE_TICKS {
            stream.send(&mut sink, &quiet(128, 64), 70).unwrap();
        }
        assert_eq!(stream.mode(), Mode::Tiles);
        sink.frames.clear();
        for seed in 50..=u8::try_from(50 + ENTER_HITS + 2).unwrap() {
            stream.send(&mut sink, &busy(128, 64, seed), 70).unwrap();
        }
        let sent = videos(&sink);
        assert_ne!(sent, []);
        assert!(
            sent[0].2,
            "the second video session did not start with a keyframe"
        );
    }

    #[test]
    fn video_is_not_sent_while_the_screen_is_still() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        let count = videos(&sink).len();
        for _ in 0..(LEAVE_TICKS - 1) {
            assert_eq!(stream.send(&mut sink, &quiet(128, 64), 70).unwrap(), None);
        }
        assert_eq!(videos(&sink).len(), count);
    }

    #[test]
    fn a_resize_restarts_the_stream_with_a_new_keyframe() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        sink.frames.clear();
        stream.send(&mut sink, &whole(160, 96, 0), 70).unwrap();
        assert!(matches!(
            sink.frames[0],
            DesktopMessage::Resize {
                width: 160,
                height: 96
            }
        ));
        for seed in 1..=u8::try_from(ENTER_HITS + 2).unwrap() {
            stream.send(&mut sink, &busy(160, 96, seed), 70).unwrap();
        }
        let sent = videos(&sink);
        assert_ne!(sent, []);
        assert!(sent[0].2);
        assert!(sent.iter().all(|video| (video.0, video.1) == (160, 96)));
    }

    #[test]
    fn a_picture_too_small_for_the_encoder_falls_back_to_tiles_for_good() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 8, 8);
        for seed in 20..40 {
            stream.send(&mut sink, &busy(8, 8, seed), 70).unwrap();
        }
        assert_eq!(videos(&sink), []);
        assert_eq!(stream.mode(), Mode::Tiles);
    }

    #[test]
    fn a_lower_quality_rebuilds_the_encoder_and_starts_with_a_keyframe() {
        let mut stream = capable();
        let mut sink = Collector::default();
        drive_into_video(&mut stream, &mut sink, 128, 64);
        sink.frames.clear();
        stream.send(&mut sink, &busy(128, 64, 40), 30).unwrap();
        let sent = videos(&sink);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].2);
    }

    #[test]
    fn the_byte_count_matches_what_was_sent() {
        let mut stream = capable();
        let mut sink = Collector::default();
        let count = stream
            .send(&mut sink, &whole(128, 64, 0), 70)
            .unwrap()
            .unwrap();
        let total: usize = sink
            .frames
            .iter()
            .map(|message| message.encode().len())
            .sum();
        assert_eq!(count, total);
    }

    #[test]
    fn the_changed_fraction_is_the_share_of_the_screen() {
        assert!((changed_fraction(&busy(100, 100, 1)) - 0.75).abs() < 0.01);
        assert!(changed_fraction(&quiet(100, 100)) < f32::EPSILON);
        assert!((changed_fraction(&whole(10, 10, 1)) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn wide_regions_split_before_tall_ones() {
        let rect = Rect {
            x: 10,
            y: 20,
            width: 5,
            height: 8,
        };
        let [left, right] = split(rect).unwrap();
        assert_eq!((left.x, left.width), (10, 2));
        assert_eq!((right.x, right.width), (12, 3));
        let [top, bottom] = split(Rect { width: 1, ..rect }).unwrap();
        assert_eq!((top.y, top.height), (20, 4));
        assert_eq!((bottom.y, bottom.height), (24, 4));
        assert!(
            split(Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1
            })
            .is_none()
        );
    }
}
