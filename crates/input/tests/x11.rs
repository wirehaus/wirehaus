#![cfg(target_os = "linux")]

use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use wirehaus_input::{InputError, InputSink, X11Input};
use wirehaus_protocol::{Button, InputEvent};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::randr::{ConnectionExt as _, Rotation};
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, EventMask, InputFocus, WindowClass};
use x11rb::rust_connection::RustConnection;

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start(number: u32) -> Self {
        Self::start_sized(number, "320x200x24")
    }

    fn start_sized(number: u32, geometry: &str) -> Self {
        let display = format!(":{number}");
        let child = Command::new("Xvfb")
            .args([
                &display,
                "-screen",
                "0",
                geometry,
                "-nolisten",
                "tcp",
                "-noreset",
            ])
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

fn pointer(observer: &RustConnection, root: u32) -> (i16, i16, u16) {
    let reply = observer.query_pointer(root).unwrap().reply().unwrap();
    (reply.root_x, reply.root_y, u16::from(reply.mask))
}

fn key_is_down(observer: &RustConnection, keycode: u8) -> bool {
    let keys = observer.query_keymap().unwrap().reply().unwrap().keys;
    keys[usize::from(keycode / 8)] & (1 << (keycode % 8)) != 0
}

fn eventually(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn injects_pointer_buttons_and_keys() {
    let server = Xvfb::start(80 + std::process::id() % 9);
    let mut input = X11Input::connect(Some(&server.display)).unwrap();
    let (observer, number) = x11rb::connect(Some(&server.display)).unwrap();
    let root = observer.setup().roots[number].root;

    input
        .send(InputEvent::PointerMove { x: 120, y: 80 })
        .unwrap();
    assert!(eventually(|| {
        let (x, y, _) = pointer(&observer, root);
        (x, y) == (120, 80)
    }));

    input
        .send(InputEvent::Button {
            button: Button::Left,
            pressed: true,
        })
        .unwrap();
    assert!(eventually(|| pointer(&observer, root).2 & 0x100 != 0));
    input
        .send(InputEvent::Button {
            button: Button::Left,
            pressed: false,
        })
        .unwrap();
    assert!(eventually(|| pointer(&observer, root).2 & 0x100 == 0));

    input
        .send(InputEvent::Key {
            code: 30,
            pressed: true,
        })
        .unwrap();
    assert!(eventually(|| key_is_down(&observer, 38)));
    input
        .send(InputEvent::Key {
            code: 30,
            pressed: false,
        })
        .unwrap();
    assert!(eventually(|| !key_is_down(&observer, 38)));

    input
        .send(InputEvent::Wheel {
            horizontal: 0,
            vertical: -2,
        })
        .unwrap();
}

#[test]
fn refuses_positions_outside_the_screen_and_unknown_keys() {
    let server = Xvfb::start(70 + std::process::id() % 9);
    let mut input = X11Input::connect(Some(&server.display)).unwrap();
    assert!(matches!(
        input.send(InputEvent::PointerMove { x: 320, y: 0 }),
        Err(InputError::PointerOutOfRange { .. })
    ));
    assert!(matches!(
        input.send(InputEvent::Key {
            code: 400,
            pressed: true
        }),
        Err(InputError::KeyOutOfRange(400))
    ));
}

#[test]
fn follows_a_screen_that_changes_size_while_connected() {
    let server = Xvfb::start_sized(60 + std::process::id() % 9, "800x600x24");
    let (observer, number) = x11rb::connect(Some(&server.display)).unwrap();
    let root = observer.setup().roots[number].root;
    let resources = observer
        .randr_get_screen_resources_current(root)
        .unwrap()
        .reply()
        .unwrap();
    observer
        .randr_set_crtc_config(
            resources.crtcs[0],
            0,
            resources.config_timestamp,
            0,
            0,
            0,
            Rotation::ROTATE0,
            &[],
        )
        .unwrap()
        .reply()
        .unwrap();
    observer
        .randr_set_screen_size(root, 320, 200, 85, 53)
        .unwrap()
        .check()
        .unwrap();
    let mut input = X11Input::connect(Some(&server.display)).unwrap();
    assert!(matches!(
        input.send(InputEvent::PointerMove { x: 600, y: 300 }),
        Err(InputError::PointerOutOfRange { .. })
    ));
    sleep(Duration::from_millis(300));
    observer
        .randr_set_screen_size(root, 640, 400, 169, 106)
        .unwrap()
        .check()
        .unwrap();
    input
        .send(InputEvent::PointerMove { x: 600, y: 300 })
        .unwrap();
    assert!(eventually(|| {
        let (x, y, _) = pointer(&observer, root);
        (x, y) == (600, 300)
    }));
}

fn set_layout(display: &str, layout: &str) {
    let status = Command::new("setxkbmap")
        .args(["-display", display, layout])
        .status()
        .expect("setxkbmap must be installed to run this test");
    assert!(status.success(), "setxkbmap rejected the layout {layout}");
}

fn focused_window(observer: &RustConnection, root: u32) -> u32 {
    let window = observer.generate_id().unwrap();
    observer
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            100,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS),
        )
        .unwrap();
    observer.map_window(window).unwrap();
    observer
        .set_input_focus(InputFocus::POINTER_ROOT, window, 0u32)
        .unwrap();
    observer.get_input_focus().unwrap().reply().unwrap();
    window
}

fn keysym_produced_by(input: &mut X11Input, observer: &RustConnection, code: u16) -> Option<u32> {
    input
        .send(InputEvent::Key {
            code,
            pressed: true,
        })
        .unwrap();
    input
        .send(InputEvent::Key {
            code,
            pressed: false,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(Event::KeyPress(event)) = observer.poll_for_event().unwrap() {
            let mapping = observer
                .get_keyboard_mapping(event.detail, 1)
                .unwrap()
                .reply()
                .unwrap();
            return mapping.keysyms.first().copied();
        }
        sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn keys_follow_the_layout_of_the_target_machine() {
    let server = Xvfb::start(50 + std::process::id() % 9);
    let (observer, number) = x11rb::connect(Some(&server.display)).unwrap();
    let root = observer.setup().roots[number].root;
    focused_window(&observer, root);
    let mut input = X11Input::connect(Some(&server.display)).unwrap();
    let cases: [(&str, [(u16, u32); 4]); 4] = [
        ("us", [(21, 0x79), (44, 0x7a), (16, 0x71), (39, 0x3b)]),
        ("es", [(21, 0x79), (44, 0x7a), (16, 0x71), (39, 0xf1)]),
        ("de", [(21, 0x7a), (44, 0x79), (16, 0x71), (39, 0xf6)]),
        ("fr", [(21, 0x79), (44, 0x77), (16, 0x61), (39, 0x6d)]),
    ];
    for (layout, keys) in cases {
        set_layout(&server.display, layout);
        sleep(Duration::from_millis(200));
        for (code, expected) in keys {
            assert_eq!(
                keysym_produced_by(&mut input, &observer, code),
                Some(expected),
                "layout {layout}, evdev code {code}"
            );
        }
    }
}
