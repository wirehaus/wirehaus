use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::damage::{ConnectionExt as DamageExt, ReportLevel};
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};
use x11rb::rust_connection::RustConnection;

use crate::bitmap::{Bitmap, Rect};
use crate::damage::{all_tiles, changed_tiles, coalesce, tile_count, tiles_touching};
use crate::error::CaptureError;
use crate::{Monitor, Monitors, Region, ScreenSource, Update};

const SIZE_CHECK_INTERVAL: Duration = Duration::from_millis(250);
const FULL_GRAB_PERCENT: usize = 60;

#[derive(Debug)]
pub struct X11Screen {
    connection: RustConnection,
    root: u32,
    root_width: u16,
    root_height: u16,
    width: u16,
    height: u16,
    view: Option<u8>,
    origin: (u16, u16),
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
        if let Some(name) = display {
            check_display_name(name)?;
        }
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
            root_width: width,
            root_height: height,
            width,
            height,
            view: None,
            origin: (0, 0),
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
                to_coordinate(rect.x.saturating_add(self.origin.0))?,
                to_coordinate(rect.y.saturating_add(self.origin.1))?,
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
        if geometry.width == self.root_width && geometry.height == self.root_height {
            return Ok(false);
        }
        self.root_width = geometry.width;
        self.root_height = geometry.height;
        let before = (self.width, self.height, self.origin);
        self.apply_view(self.view);
        Ok(before != (self.width, self.height, self.origin))
    }

    fn list(&self) -> Vec<Monitor> {
        let Ok(reply) = self
            .connection
            .randr_get_monitors(self.root, true)
            .map_err(request_error)
            .and_then(|cookie| cookie.reply().map_err(request_error))
        else {
            return Vec::new();
        };
        let mut items: Vec<Monitor> = reply
            .monitors
            .iter()
            .filter(|monitor| {
                monitor.width > 0 && monitor.height > 0 && monitor.x >= 0 && monitor.y >= 0
            })
            .map(|monitor| Monitor {
                x: i32::from(monitor.x),
                y: i32::from(monitor.y),
                width: monitor.width,
                height: monitor.height,
                primary: monitor.primary,
                all: false,
            })
            .collect();
        items.sort_by_key(|monitor| (!monitor.primary, monitor.x, monitor.y));
        if items.len() > 1 {
            items.retain(|monitor| {
                (monitor.x, monitor.y, monitor.width, monitor.height)
                    != (0, 0, self.root_width, self.root_height)
            });
        }
        items
    }

    fn apply_view(&mut self, view: Option<u8>) {
        let list = self.list();
        let picked = view.and_then(|index| list.get(usize::from(index)));
        if let Some(monitor) = picked {
            self.view = view;
            self.origin = (
                u16::try_from(monitor.x).unwrap_or(0),
                u16::try_from(monitor.y).unwrap_or(0),
            );
            self.width = monitor
                .width
                .min(self.root_width.saturating_sub(self.origin.0));
            self.height = monitor
                .height
                .min(self.root_height.saturating_sub(self.origin.1));
        } else {
            self.view = None;
            self.origin = (0, 0);
            self.width = self.root_width;
            self.height = self.root_height;
        }
        self.needs_full = true;
        self.previous = None;
    }

    fn drain_damage(&mut self) -> Result<Vec<Rect>, CaptureError> {
        let Some(damage) = self.damage else {
            return Ok(Vec::new());
        };
        let mut rects = Vec::new();
        while let Some(event) = self.connection.poll_for_event().map_err(request_error)? {
            if let Event::DamageNotify(notify) = event
                && let Some(rect) = self.within_view(notify.area)
            {
                rects.push(rect);
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

    fn within_view(&self, area: x11rb::protocol::xproto::Rectangle) -> Option<Rect> {
        let left = i32::from(area.x).max(i32::from(self.origin.0));
        let top = i32::from(area.y).max(i32::from(self.origin.1));
        let right = (i32::from(area.x) + i32::from(area.width))
            .min(i32::from(self.origin.0) + i32::from(self.width));
        let bottom = (i32::from(area.y) + i32::from(area.height))
            .min(i32::from(self.origin.1) + i32::from(self.height));
        if right <= left || bottom <= top {
            return None;
        }
        Some(Rect {
            x: u16::try_from(left - i32::from(self.origin.0)).ok()?,
            y: u16::try_from(top - i32::from(self.origin.1)).ok()?,
            width: u16::try_from(right - left).ok()?,
            height: u16::try_from(bottom - top).ok()?,
        })
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

    fn monitors(&mut self) -> Monitors {
        let mut items = self.list();
        if items.len() < 2 {
            return Monitors::default();
        }
        items.push(Monitor {
            x: 0,
            y: 0,
            width: self.root_width,
            height: self.root_height,
            primary: false,
            all: true,
        });
        let active = self
            .view
            .unwrap_or_else(|| u8::try_from(items.len() - 1).unwrap_or(u8::MAX));
        Monitors { items, active }
    }

    fn select_monitor(&mut self, index: u8) -> bool {
        let count = self.list().len();
        let wanted = if usize::from(index) < count {
            Some(index)
        } else {
            None
        };
        if usize::from(index) > count || wanted == self.view {
            return false;
        }
        self.apply_view(wanted);
        true
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

const MAX_DISPLAY_NUMBER: u32 = 65535 - 6000;

fn check_display_name(name: &str) -> Result<(), CaptureError> {
    let invalid = || CaptureError::Connect(format!("invalid display name: {name}"));
    let (_, rest) = name.rsplit_once(':').ok_or_else(invalid)?;
    let number = rest.split('.').next().unwrap_or_default();
    match number.parse::<u32>() {
        Ok(n) if n <= MAX_DISPLAY_NUMBER => Ok(()),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_display_numbers_that_overflow_the_tcp_port() {
        assert!(check_display_name(":59999").is_err());
        assert!(check_display_name("host:99999.0").is_err());
        assert!(check_display_name("nocolon").is_err());
        assert!(check_display_name(":x").is_err());
        assert!(check_display_name(":0").is_ok());
        assert!(check_display_name("host:10.1").is_ok());
        assert!(check_display_name(":59535").is_ok());
    }

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
