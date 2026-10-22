#![cfg(target_os = "linux")]

use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};
use wirehaus_agent::{Server, ServerConfig, ServerError};
use wirehaus_protocol::{Channel, Codec, DesktopMessage, Frame, InputEvent};
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

struct Running {
    port: u16,
    stop: &'static AtomicBool,
    thread: Option<thread::JoinHandle<()>>,
    dir: PathBuf,
}

impl Running {
    fn start(display: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("wirehaus-serve-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.html"), b"<title>viewer</title>").unwrap();
        let server = Server::bind(ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            display: Some(display.to_owned()),
            viewer_dir: dir.clone(),
            jpeg_quality: 70,
        })
        .unwrap();
        let port = server.local_addr().unwrap().port();
        let stop: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let thread = thread::spawn(move || {
            server.run(stop).unwrap();
        });
        Self {
            port,
            stop,
            thread: Some(thread),
            dir,
        }
    }

    fn connect(&self, origin: Option<&str>) -> Result<Socket, tungstenite::Error> {
        let mut request = format!("ws://127.0.0.1:{}/ws", self.port)
            .into_client_request()
            .unwrap();
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", origin.parse().unwrap());
        }
        let stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        tungstenite::client(request, MaybeTlsStream::Plain(stream))
            .map(|(socket, _)| socket)
            .map_err(|e| match e {
                tungstenite::HandshakeError::Failure(error) => error,
                tungstenite::HandshakeError::Interrupted(_) => unreachable!(),
            })
    }

    fn http(&self, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        let _ = stream.shutdown(Shutdown::Both);
        out
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn next_desktop(socket: &mut Socket) -> DesktopMessage {
    loop {
        match socket.read().unwrap() {
            Message::Binary(bytes) => {
                let frame = Frame::decode(&bytes).unwrap();
                assert_eq!(frame.channel, Channel::Desktop);
                return DesktopMessage::decode(&frame.payload).unwrap();
            }
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("unexpected message {other:?}"),
        }
    }
}

fn send_input(socket: &mut Socket, event: InputEvent) {
    let frame = Frame::new(Channel::Desktop, 0, event.encode()).unwrap();
    socket
        .send(Message::Binary(frame.encode().unwrap().into()))
        .unwrap();
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

fn pointer_at(display: &str) -> (i16, i16) {
    let (connection, number) = x11rb::connect(Some(display)).unwrap();
    let root = connection.setup().roots[number].root;
    let reply = connection.query_pointer(root).unwrap().reply().unwrap();
    (reply.root_x, reply.root_y)
}

#[test]
fn streams_the_screen_and_applies_input() {
    let xvfb = Xvfb::start(60 + std::process::id() % 9);
    paint_root(&xvfb.display, 0x0000_ff00);
    let server = Running::start(&xvfb.display);
    let mut socket = server.connect(Some(&server.origin())).unwrap();

    assert_eq!(
        next_desktop(&mut socket),
        DesktopMessage::Resize {
            width: 320,
            height: 200
        }
    );
    let mut covered = 0_u32;
    while covered < 320 * 200 {
        match next_desktop(&mut socket) {
            DesktopMessage::Tile {
                width,
                height,
                codec,
                data,
                ..
            } => {
                assert_eq!(codec, Codec::Jpeg);
                assert_eq!(&data[..2], [0xFF, 0xD8]);
                covered += u32::from(width) * u32::from(height);
            }
            resize @ DesktopMessage::Resize { .. } => panic!("expected a tile, got {resize:?}"),
        }
    }
    assert_eq!(covered, 320 * 200);

    send_input(&mut socket, InputEvent::PointerMove { x: 100, y: 50 });
    let deadline = Instant::now() + Duration::from_secs(3);
    while pointer_at(&xvfb.display) != (100, 50) {
        assert!(Instant::now() < deadline, "the pointer never moved");
        sleep(Duration::from_millis(20));
    }

    paint_root(&xvfb.display, 0x0000_00ff);
    let mut updated = 0_u32;
    let deadline = Instant::now() + Duration::from_secs(5);
    while updated < 320 * 200 {
        assert!(Instant::now() < deadline, "the repaint never arrived");
        if let DesktopMessage::Tile { width, height, .. } = next_desktop(&mut socket) {
            updated += u32::from(width) * u32::from(height);
        }
    }
}

#[test]
fn rejects_foreign_origins_and_hosts() {
    let xvfb = Xvfb::start(50 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);

    let error = server.connect(Some("http://evil.example")).unwrap_err();
    assert!(error.to_string().contains("403"), "{error}");
    let error = server.connect(Some("null")).unwrap_err();
    assert!(error.to_string().contains("403"), "{error}");

    let reply = server.http("GET / HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    let reply = server.http(&format!(
        "GET /ws HTTP/1.1\r\nHost: evil.example:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
        server.port
    ));
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
}

#[test]
fn serves_only_the_viewer_files() {
    let xvfb = Xvfb::start(40 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);

    let reply = server.http(&format!("GET / HTTP/1.1\r\n{host}\r\n"));
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("Content-Security-Policy: default-src 'none'"));
    assert!(reply.ends_with("<title>viewer</title>"));

    for path in [
        "/../Cargo.toml",
        "/%2e%2e/x.js",
        "/missing.js",
        "/index.txt",
    ] {
        let reply = server.http(&format!("GET {path} HTTP/1.1\r\n{host}\r\n"));
        assert!(reply.starts_with("HTTP/1.1 404"), "{path}: {reply}");
    }
    let reply = server.http(&format!(
        "POST / HTTP/1.1\r\n{host}Content-Length: 0\r\n\r\n"
    ));
    assert!(reply.starts_with("HTTP/1.1 405"), "{reply}");
    let reply = server.http("not http at all\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
}

#[test]
fn allows_a_single_viewer_at_a_time() {
    let xvfb = Xvfb::start(30 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let mut first = server.connect(Some(&server.origin())).unwrap();
    assert!(matches!(
        next_desktop(&mut first),
        DesktopMessage::Resize { .. }
    ));
    let error = server.connect(Some(&server.origin())).unwrap_err();
    assert!(error.to_string().contains("503"), "{error}");
}

#[test]
fn drops_a_viewer_that_sends_garbage_and_accepts_the_next() {
    let xvfb = Xvfb::start(20 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let mut first = server.connect(Some(&server.origin())).unwrap();
    first.send(Message::Binary(vec![1, 2, 3].into())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "the session was not closed");
        if first.read().is_err() {
            break;
        }
    }
    let mut second = loop {
        if let Ok(socket) = server.connect(Some(&server.origin())) {
            break socket;
        }
        assert!(Instant::now() < deadline, "the next viewer never got in");
        sleep(Duration::from_millis(50));
    };
    assert!(matches!(
        next_desktop(&mut second),
        DesktopMessage::Resize { .. }
    ));
}

#[test]
fn refuses_to_bind_outside_the_loopback_interface() {
    let result = Server::bind(ServerConfig {
        listen: "0.0.0.0:0".parse().unwrap(),
        display: None,
        viewer_dir: PathBuf::from("."),
        jpeg_quality: 70,
    });
    assert!(matches!(result, Err(ServerError::NotLoopback(_))));
}
