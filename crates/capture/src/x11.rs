use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::damage::{ConnectionExt as DamageExt, ReportLevel};
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};
use x11rb::rust_connection::RustConnection;

use crate::bitmap::{Bitmap, Rect};
use crate::damage::{all_tiles, changed_tiles, coalesce, tile_count, tiles_touching};
use crate::error::CaptureError;
use crate::{Region, ScreenSource, Update};

const SIZE_CHECK_INTERVAL: Duration = Duration::from_millis(250);
const FULL_GRAB_PERCENT: usize = 60;

#[derive(Debug)]
pub struct X11Screen {
    connection: RustConnection,
    root: u32,
    width: u16,
    height: u16,
    damage: Option<u32>,
    previous: Option<Bitmap>,
    needs_full: bool,
    last_size_check: Instant,
}

impl X11Screen {
    pub fn connect(display: Option<&str>) -> Result<Self, CaptureError> {
        Self::open(display, true)
    }

    pub fn connect_polling(display: Option<&str>) -> Result<Self, CaptureError> {
        Self::open(display, false)
    }

    fn open(display: Option<&str>, use_damage: bool) -> Result<Self, CaptureError> {
        let (connection, number) =
            x11rb::connect(display).map_err(|e| CaptureError::Connect(e.to_string()))?;
        let setup = connection.setup();
        let screen = setup
            .roots
            .get(number)
            .ok_or_else(|| CaptureError::Connect(format!("no screen {number}")))?;
        ensure_bgra(setup, screen.root_depth)?;
        let (root, width, height) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
        let damage = use_damage
            .then(|| enable_damage(&connection, root))
            .flatten();
        Ok(Self {
            connection,
            root,
            width,
            height,
            damage,
            previous: None,
            needs_full: true,
            last_size_check: Instant::now(),
        })
    }

    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    #[must_use]
    pub fn reports_damage(&self) -> bool {
        self.damage.is_some()
    }

    pub fn grab(&mut self) -> Result<Bitmap, CaptureError> {
        self.grab_region(Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        })
    }

    fn grab_region(&self, rect: Rect) -> Result<Bitmap, CaptureError> {
        let reply = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.root,
                to_coordinate(rect.x)?,
                to_coordinate(rect.y)?,
                rect.width,
                rect.height,
                u32::MAX,
            )
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?;
        let mut data = reply.data;
        opaque(&mut data);
        Bitmap::new(rect.width, rect.height, data)
    }

    fn refresh_size(&mut self) -> Result<bool, CaptureError> {
        if self.last_size_check.elapsed() < SIZE_CHECK_INTERVAL {
            return Ok(false);
        }
        self.last_size_check = Instant::now();
        let geometry = self
            .connection
            .get_geometry(self.root)
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?;
        let changed = geometry.width != self.width || geometry.height != self.height;
        self.width = geometry.width;
        self.height = geometry.height;
        Ok(changed)
    }

    fn drain_damage(&mut self) -> Result<Vec<Rect>, CaptureError> {
        let Some(damage) = self.damage else {
            return Ok(Vec::new());
        };
        let mut rects = Vec::new();
        while let Some(event) = self.connection.poll_for_event().map_err(request_error)? {
            if let Event::DamageNotify(notify) = event {
                rects.push(Rect {
                    x: u16::try_from(notify.area.x.max(0)).unwrap_or(0),
                    y: u16::try_from(notify.area.y.max(0)).unwrap_or(0),
                    width: notify.area.width,
                    height: notify.area.height,
                });
            }
        }
        if !rects.is_empty() {
            self.connection
                .damage_subtract(damage, x11rb::NONE, x11rb::NONE)
                .map_err(request_error)?;
            self.connection.flush().map_err(request_error)?;
        }
        Ok(rects)
    }

    fn full_update(&mut self, resized: bool) -> Result<Update, CaptureError> {
        self.drain_damage()?;
        let bitmap = self.grab()?;
        let rect = Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        };
        if self.damage.is_none() {
            self.previous = Some(bitmap.clone());
        }
        self.needs_full = false;
        Ok(self.update(resized, vec![Region { rect, bitmap }]))
    }

    fn damage_update(&mut self) -> Result<Update, CaptureError> {
        let rects = self.drain_damage()?;
        if rects.is_empty() {
            return Ok(self.update(false, Vec::new()));
        }
        let tiles = tiles_touching(&rects, self.width, self.height);
        let total = tile_count(self.width) * tile_count(self.height);
        if tiles.len() * 100 > total * FULL_GRAB_PERCENT {
            return self.full_update(false);
        }
        let regions = self.grab_runs(&coalesce(&tiles))?;
        Ok(self.update(false, regions))
    }

    fn polling_update(&mut self) -> Result<Update, CaptureError> {
        let current = self.grab()?;
        let tiles = match &self.previous {
            Some(previous) => changed_tiles(previous, &current),
            None => all_tiles(&current),
        };
        let regions = coalesce(&tiles)
            .into_iter()
            .map(|rect| {
                Bitmap::new(rect.width, rect.height, current.crop(rect))
                    .map(|bitmap| Region { rect, bitmap })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.previous = Some(current);
        Ok(self.update(false, regions))
    }

    fn grab_runs(&self, runs: &[Rect]) -> Result<Vec<Region>, CaptureError> {
        runs.iter()
            .map(|&rect| self.grab_region(rect).map(|bitmap| Region { rect, bitmap }))
            .collect()
    }

    fn update(&self, resized: bool, regions: Vec<Region>) -> Update {
        Update {
            width: self.width,
            height: self.height,
            resized,
            regions,
        }
    }
}

impl ScreenSource for X11Screen {
    fn next_update(&mut self) -> Result<Update, CaptureError> {
        let resized = self.refresh_size()?;
        if resized || self.needs_full {
            return self.full_update(true);
        }
        if self.damage.is_some() {
            self.damage_update()
        } else {
            self.polling_update()
        }
    }
}

fn enable_damage(connection: &RustConnection, root: u32) -> Option<u32> {
    connection.damage_query_version(1, 1).ok()?.reply().ok()?;
    let id = connection.generate_id().ok()?;
    connection
        .damage_create(id, root, ReportLevel::DELTA_RECTANGLES)
        .ok()?
        .check()
        .ok()?;
    Some(id)
}

fn ensure_bgra(setup: &x11rb::protocol::xproto::Setup, depth: u8) -> Result<(), CaptureError> {
    let bits_per_pixel = setup
        .pixmap_formats
        .iter()
        .find(|format| format.depth == depth)
        .map_or(0, |format| format.bits_per_pixel);
    let little_endian = setup.image_byte_order == ImageOrder::LSB_FIRST;
    if bits_per_pixel == 32 && little_endian {
        Ok(())
    } else {
        Err(CaptureError::UnsupportedFormat {
            depth,
            bits_per_pixel,
        })
    }
}

fn opaque(data: &mut [u8]) {
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
}

fn to_coordinate(value: u16) -> Result<i16, CaptureError> {
    i16::try_from(value)
        .map_err(|_| CaptureError::Request(format!("coordinate {value} is too large")))
}

fn request_error(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forces_the_alpha_channel_to_opaque() {
        let mut data = vec![1, 2, 3, 0, 4, 5, 6, 7];
        opaque(&mut data);
        assert_eq!(data, [1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn rejects_coordinates_that_do_not_fit_the_protocol() {
        assert!(to_coordinate(32767).is_ok());
        assert!(to_coordinate(32768).is_err());
    }
}
