use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};
use x11rb::rust_connection::RustConnection;

use crate::ScreenSource;
use crate::bitmap::Bitmap;
use crate::error::CaptureError;

#[derive(Debug)]
pub struct X11Screen {
    connection: RustConnection,
    root: u32,
    width: u16,
    height: u16,
    depth: u8,
}

impl X11Screen {
    pub fn connect(display: Option<&str>) -> Result<Self, CaptureError> {
        let (connection, number) =
            x11rb::connect(display).map_err(|e| CaptureError::Connect(e.to_string()))?;
        let setup = connection.setup();
        let screen = setup
            .roots
            .get(number)
            .ok_or_else(|| CaptureError::Connect(format!("no screen {number}")))?;
        let depth = screen.root_depth;
        ensure_bgra(setup, depth)?;
        Ok(Self {
            root: screen.root,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
            depth,
            connection,
        })
    }
}

impl ScreenSource for X11Screen {
    fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn grab(&mut self) -> Result<Bitmap, CaptureError> {
        let reply = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.root,
                0,
                0,
                self.width,
                self.height,
                u32::MAX,
            )
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?;
        debug_assert_eq!(reply.depth, self.depth);
        let mut data = reply.data;
        opaque(&mut data);
        Bitmap::new(self.width, self.height, data)
    }
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
}
