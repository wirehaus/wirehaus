#![cfg(target_os = "linux")]

use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use wirehaus_clipboard::{Clipboard, ClipboardError, MAX_TEXT_BYTES, X11Clipboard};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Self {
        let offset = u32::try_from(COUNTER.fetch_add(1, Ordering::SeqCst)).unwrap();
        let number = 5000 + (std::process::id() % 400) * 10 + offset;
        let display = format!(":{number}");
        let child = Command::new("Xvfb")
            .args([
                &display,
                "-screen",
                "0",
                "320x200x24",
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

    fn clipboard(&self) -> X11Clipboard {
        X11Clipboard::connect(Some(&self.display)).unwrap()
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let number = self.display.trim_start_matches(':');
        let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
        let _ = std::fs::remove_file(format!("/tmp/.X11-unix/X{number}"));
    }
}

fn atom(connection: &RustConnection, name: &str) -> u32 {
    connection
        .intern_atom(false, name.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom
}

fn foreign_fetch(display: &str, target: &str) -> Option<Vec<u8>> {
    let (connection, number) = x11rb::connect(Some(display)).unwrap();
    let root = connection.setup().roots[number].root;
    let window = connection.generate_id().unwrap();
    connection
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap();
    let (clipboard, target, property) = (
        atom(&connection, "CLIPBOARD"),
        atom(&connection, target),
        atom(&connection, "FOREIGN_TRANSFER"),
    );
    connection
        .convert_selection(window, clipboard, target, property, CURRENT_TIME)
        .unwrap();
    connection.flush().unwrap();
    loop {
        if let Event::SelectionNotify(notify) = connection.wait_for_event().unwrap() {
            if notify.property == NONE {
                return None;
            }
            let reply = connection
                .get_property(true, window, property, AtomEnum::ANY, 0, 100_000)
                .unwrap()
                .reply()
                .unwrap();
            return Some(reply.value);
        }
    }
}

struct ForeignOwner {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ForeignOwner {
    fn start(display: &str, payload: Vec<u8>) -> Self {
        let (connection, number) = x11rb::connect(Some(display)).unwrap();
        let root = connection.setup().roots[number].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap();
        let clipboard = atom(&connection, "CLIPBOARD");
        let utf8 = atom(&connection, "UTF8_STRING");
        connection
            .set_selection_owner(window, clipboard, CURRENT_TIME)
            .unwrap();
        connection.flush().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                let Ok(Some(event)) = connection.poll_for_event() else {
                    sleep(Duration::from_millis(5));
                    continue;
                };
                if let Event::SelectionRequest(request) = event {
                    connection
                        .change_property8(
                            PropMode::REPLACE,
                            request.requestor,
                            request.property,
                            utf8,
                            &payload,
                        )
                        .unwrap();
                    let notify = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: request.time,
                        requestor: request.requestor,
                        selection: request.selection,
                        target: request.target,
                        property: request.property,
                    };
                    connection
                        .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
                        .unwrap();
                    connection.flush().unwrap();
                }
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for ForeignOwner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn eventually<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "the condition never held");
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_empty_clipboard_reads_as_nothing() {
    let xvfb = Xvfb::start();
    assert_eq!(xvfb.clipboard().read().unwrap(), None);
}

#[test]
fn text_written_by_one_client_is_read_by_another() {
    let xvfb = Xvfb::start();
    let mut writer = xvfb.clipboard();
    let mut reader = xvfb.clipboard();
    writer
        .write("hola desde el visor, ñandú\nsegunda línea")
        .unwrap();
    assert_eq!(
        reader.read().unwrap().as_deref(),
        Some("hola desde el visor, ñandú\nsegunda línea")
    );
    assert_eq!(
        writer.read().unwrap().as_deref(),
        Some("hola desde el visor, ñandú\nsegunda línea")
    );
}

#[test]
fn a_plain_x11_client_can_ask_for_the_formats_it_knows() {
    let xvfb = Xvfb::start();
    let mut writer = xvfb.clipboard();
    writer.write("ascii text").unwrap();
    assert_eq!(
        foreign_fetch(&xvfb.display, "UTF8_STRING").unwrap(),
        b"ascii text"
    );
    assert_eq!(
        foreign_fetch(&xvfb.display, "STRING").unwrap(),
        b"ascii text"
    );
    assert_eq!(
        foreign_fetch(&xvfb.display, "text/plain;charset=utf-8").unwrap(),
        b"ascii text"
    );
    let targets = foreign_fetch(&xvfb.display, "TARGETS").unwrap();
    assert_eq!(targets.len(), 16);
    assert_eq!(foreign_fetch(&xvfb.display, "image/png"), None);
}

#[test]
fn text_with_accents_is_not_offered_as_latin_one() {
    let xvfb = Xvfb::start();
    let mut writer = xvfb.clipboard();
    writer.write("ñandú").unwrap();
    assert_eq!(foreign_fetch(&xvfb.display, "STRING"), None);
    assert_eq!(
        foreign_fetch(&xvfb.display, "UTF8_STRING").unwrap(),
        "ñandú".as_bytes()
    );
}

#[test]
fn a_new_owner_replaces_the_previous_one() {
    let xvfb = Xvfb::start();
    let mut first = xvfb.clipboard();
    let mut second = xvfb.clipboard();
    first.write("primero").unwrap();
    second.write("segundo").unwrap();
    assert_eq!(first.read().unwrap().as_deref(), Some("segundo"));
    eventually(|| (foreign_fetch(&xvfb.display, "UTF8_STRING")? == b"segundo").then_some(()));
}

#[test]
fn the_text_disappears_when_its_owner_goes_away() {
    let xvfb = Xvfb::start();
    let mut reader = xvfb.clipboard();
    {
        let mut writer = xvfb.clipboard();
        writer.write("efímero").unwrap();
        assert_eq!(reader.read().unwrap().as_deref(), Some("efímero"));
    }
    assert!(eventually(|| reader
        .read()
        .unwrap()
        .is_none()
        .then_some(true)));
}

#[test]
fn text_over_the_limit_is_refused_on_write_and_on_read() {
    let xvfb = Xvfb::start();
    let mut clipboard = xvfb.clipboard();
    let too_big = "a".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        clipboard.write(&too_big),
        Err(ClipboardError::TooLarge(MAX_TEXT_BYTES))
    );
    assert!(clipboard.write(&"a".repeat(MAX_TEXT_BYTES)).is_ok());
    drop(clipboard);

    let _owner = ForeignOwner::start(&xvfb.display, vec![b'b'; MAX_TEXT_BYTES + 1]);
    let mut reader = xvfb.clipboard();
    assert_eq!(reader.read(), Err(ClipboardError::TooLarge(MAX_TEXT_BYTES)));
}

#[test]
fn text_from_a_foreign_owner_at_the_limit_is_accepted() {
    let xvfb = Xvfb::start();
    let _owner = ForeignOwner::start(&xvfb.display, vec![b'c'; MAX_TEXT_BYTES]);
    let mut reader = xvfb.clipboard();
    assert_eq!(reader.read().unwrap().unwrap().len(), MAX_TEXT_BYTES);
}

#[test]
fn invalid_utf8_from_a_foreign_owner_reads_as_nothing() {
    let xvfb = Xvfb::start();
    let _owner = ForeignOwner::start(&xvfb.display, vec![0xFF, 0xFE, 0x41]);
    let mut reader = xvfb.clipboard();
    assert_eq!(reader.read().unwrap(), None);
}

#[test]
fn an_owner_that_never_answers_times_out() {
    let xvfb = Xvfb::start();
    let (connection, number) = x11rb::connect(Some(&xvfb.display)).unwrap();
    let root = connection.setup().roots[number].root;
    let window = connection.generate_id().unwrap();
    connection
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap();
    let clipboard = atom(&connection, "CLIPBOARD");
    connection
        .set_selection_owner(window, clipboard, CURRENT_TIME)
        .unwrap();
    connection.flush().unwrap();
    let mut reader = xvfb.clipboard();
    assert_eq!(reader.read(), Err(ClipboardError::Timeout));
}
