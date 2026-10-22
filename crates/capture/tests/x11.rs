#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use wirehaus_capture::{ScreenSource, X11Screen, changed_tiles};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt};
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
        while !Path::new(&socket).exists() {
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

fn paint_root(display: &str, color: u32) {
    let (connection, number) = x11rb::connect(Some(display)).unwrap();
    let root = connection.setup().roots[number].root;
    connection
        .change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().background_pixel(color),
        )
        .unwrap();
    connection.clear_area(false, root, 0, 0, 0, 0).unwrap();
    connection.sync().unwrap();
}

#[test]
fn grabs_the_screen_and_sees_it_change() {
    let server = Xvfb::start(90 + std::process::id() % 9);
    let mut screen = X11Screen::connect(Some(&server.display)).unwrap();
    assert_eq!(screen.size(), (320, 200));

    paint_root(&server.display, 0x00ff_0000);
    let first = screen.grab().unwrap();
    assert_eq!((first.width(), first.height()), (320, 200));
    assert_eq!(&first.bgra()[..4], [0, 0, 255, 255]);

    let unchanged = screen.grab().unwrap();
    assert_eq!(changed_tiles(&first, &unchanged).len(), 0);

    paint_root(&server.display, 0x0000_00ff);
    let second = screen.grab().unwrap();
    assert_eq!(&second.bgra()[..4], [255, 0, 0, 255]);
    assert_eq!(changed_tiles(&first, &second).len(), 5 * 4);
}
