#![cfg(target_os = "linux")]

use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use wirehaus_capture::{Rect, ScreenSource, Update, X11Screen};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt, CreateGCAux, Rectangle};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start(number: u32) -> Self {
        let display = format!(":{number}");
        let child = Command::new("Xvfb")
            .args([&display, "-screen", "0", "320x200x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb must be installed to run this test");
        let socket = format!("/tmp/.X11-unix/X{number}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(&socket).is_err() {
            assert!(Instant::now() < deadline, "Xvfb did not start in time");
            sleep(Duration::from_millis(50));
        }
        Self { child, display }
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Painter {
    connection: RustConnection,
    root: u32,
}

impl Painter {
    fn connect(display: &str) -> Self {
        let (connection, number) = x11rb::connect(Some(display)).unwrap();
        let root = connection.setup().roots[number].root;
        Self { connection, root }
    }

    fn fill_root(&self, color: u32) {
        self.connection
            .change_window_attributes(
                self.root,
                &ChangeWindowAttributesAux::new().background_pixel(color),
            )
            .unwrap();
        self.connection
            .clear_area(false, self.root, 0, 0, 0, 0)
            .unwrap();
        self.connection.sync().unwrap();
    }

    fn fill_rectangle(&self, rect: Rectangle, color: u32) {
        let gc = self.connection.generate_id().unwrap();
        self.connection
            .create_gc(gc, self.root, &CreateGCAux::new().foreground(color))
            .unwrap();
        self.connection
            .poly_fill_rectangle(self.root, gc, &[rect])
            .unwrap();
        self.connection.free_gc(gc).unwrap();
        self.connection.sync().unwrap();
    }
}

fn next_changes(screen: &mut X11Screen) -> Update {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let update = screen.next_update().unwrap();
        if !update.regions.is_empty() {
            return update;
        }
        assert!(Instant::now() < deadline, "no change was reported");
        sleep(Duration::from_millis(20));
    }
}

fn pixel_at(update: &Update, x: u16, y: u16) -> [u8; 4] {
    let region = update
        .regions
        .iter()
        .find(|r| {
            x >= r.rect.x
                && x < r.rect.x + r.rect.width
                && y >= r.rect.y
                && y < r.rect.y + r.rect.height
        })
        .expect("no region covers that pixel");
    let row = region.bitmap.row(y - region.rect.y);
    let at = usize::from(x - region.rect.x) * 4;
    row[at..at + 4].try_into().unwrap()
}

fn assert_first_update_is_the_whole_screen(screen: &mut X11Screen) {
    let first = screen.next_update().unwrap();
    assert!(first.resized);
    assert_eq!((first.width, first.height), (320, 200));
    assert_eq!(first.regions.len(), 1);
    assert_eq!(
        first.regions[0].rect,
        Rect {
            x: 0,
            y: 0,
            width: 320,
            height: 200
        }
    );
    assert_eq!(pixel_at(&first, 0, 0), [0, 255, 0, 255]);
}

fn assert_a_small_change_reports_only_its_tile(screen: &mut X11Screen, painter: &Painter) {
    let quiet = screen.next_update().unwrap();
    assert!(!quiet.resized);
    assert_eq!(quiet.regions.len(), 0);

    painter.fill_rectangle(
        Rectangle {
            x: 70,
            y: 5,
            width: 10,
            height: 10,
        },
        0x00ff_0000,
    );
    let changed = next_changes(screen);
    assert!(!changed.resized);
    assert_eq!(pixel_at(&changed, 72, 7), [0, 0, 255, 255]);
    let covered: u32 = changed
        .regions
        .iter()
        .map(|r| u32::from(r.rect.width) * u32::from(r.rect.height))
        .sum();
    assert!(covered <= 64 * 64 * 2, "covered {covered} pixels");
}

#[test]
fn reports_the_whole_screen_first_then_only_damaged_tiles() {
    let server = Xvfb::start(90 + std::process::id() % 9);
    let painter = Painter::connect(&server.display);
    painter.fill_root(0x0000_ff00);
    let mut screen = X11Screen::connect(Some(&server.display)).unwrap();
    assert!(screen.reports_damage());
    assert_eq!(screen.size(), (320, 200));
    assert_first_update_is_the_whole_screen(&mut screen);
    assert_a_small_change_reports_only_its_tile(&mut screen, &painter);
}

#[test]
fn the_polling_fallback_reports_the_same_changes() {
    let server = Xvfb::start(80 + std::process::id() % 9);
    let painter = Painter::connect(&server.display);
    painter.fill_root(0x0000_ff00);
    let mut screen = X11Screen::connect_polling(Some(&server.display)).unwrap();
    assert!(!screen.reports_damage());
    assert_first_update_is_the_whole_screen(&mut screen);
    assert_a_small_change_reports_only_its_tile(&mut screen, &painter);
}

#[test]
fn a_repaint_of_the_whole_screen_is_reported_in_full() {
    let server = Xvfb::start(70 + std::process::id() % 9);
    let painter = Painter::connect(&server.display);
    let mut screen = X11Screen::connect(Some(&server.display)).unwrap();
    screen.next_update().unwrap();
    painter.fill_root(0x0000_00ff);
    let update = next_changes(&mut screen);
    assert_eq!(pixel_at(&update, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&update, 319, 199), [255, 0, 0, 255]);
}

#[test]
fn grabs_the_whole_screen_on_demand() {
    let server = Xvfb::start(60 + std::process::id() % 9);
    let painter = Painter::connect(&server.display);
    painter.fill_root(0x00ff_0000);
    let mut screen = X11Screen::connect(Some(&server.display)).unwrap();
    let bitmap = screen.grab().unwrap();
    assert_eq!((bitmap.width(), bitmap.height()), (320, 200));
    assert_eq!(&bitmap.bgra()[..4], [0, 0, 255, 255]);
}

#[test]
fn changed_pixels_are_not_reported_twice() {
    let server = Xvfb::start(50 + std::process::id() % 9);
    let painter = Painter::connect(&server.display);
    let mut screen = X11Screen::connect(Some(&server.display)).unwrap();
    screen.next_update().unwrap();
    painter.fill_rectangle(
        Rectangle {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        0x00ff_ffff,
    );
    next_changes(&mut screen);
    sleep(Duration::from_millis(100));
    assert_eq!(screen.next_update().unwrap().regions.len(), 0);
}
