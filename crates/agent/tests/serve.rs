#![cfg(target_os = "linux")]

use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};
use wirehaus_agent::state::State;
use wirehaus_agent::{Clock, Server, ServerConfig, ServerError, Store};
use wirehaus_crypto::handshake::ViewerHandshake;
use wirehaus_crypto::keys::{PublicKey, StaticKey};
use wirehaus_crypto::pairing::PairingCode;
use wirehaus_crypto::session::{Limits, Session};
use wirehaus_crypto::totp::TotpSecret;
use wirehaus_protocol::{
    AuthFailure, Channel, Codec, ControlMessage, DesktopMessage, Frame, HEADER_LEN, InputEvent,
    MAX_PAYLOAD_LEN, ViewerMessage,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt, CreateGCAux, Rectangle};
use x11rb::wrapper::ConnectionExt as _;

const START_TIME: u64 = 1_700_000_000;
const VIEWER_LIMITS: Limits = Limits {
    send: 4096,
    receive: HEADER_LEN + MAX_PAYLOAD_LEN,
};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    fn start(number: u32) -> Self {
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
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static RUN_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Running {
    port: u16,
    stop: &'static AtomicBool,
    thread: Option<thread::JoinHandle<()>>,
    dir: PathBuf,
    time: Arc<AtomicU64>,
    store: Store,
}

impl Running {
    fn start(display: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "wirehaus-serve-{}-{}",
            std::process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let viewer = root.join("viewer");
        fs::create_dir_all(&viewer).unwrap();
        fs::write(viewer.join("index.html"), b"<title>viewer</title>").unwrap();
        let time = Arc::new(AtomicU64::new(START_TIME));
        let clock: Clock = {
            let shared = Arc::clone(&time);
            Arc::new(move || shared.load(Ordering::SeqCst))
        };
        let state_dir = root.join("state");
        let store = Store::initialize(&state_dir, Arc::clone(&clock)).unwrap();
        let server = Server::bind(ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            display: Some(display.to_owned()),
            viewer_dir: viewer,
            jpeg_quality: 70,
            state_dir,
            clock: Some(clock),
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
            dir: root,
            time,
            store,
        }
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn agent_key(&self) -> PublicKey {
        self.store
            .read(|state, _| state.agent_key().public())
            .unwrap()
    }

    fn open_pairing(&self) -> PairingCode {
        self.store.update(State::open_pairing).unwrap().unwrap()
    }

    fn fresh_totp(&self) -> String {
        let now = self.time.fetch_add(31, Ordering::SeqCst) + 31;
        self.totp_at(now)
    }

    fn totp_at(&self, now: u64) -> String {
        self.store
            .read(|state, _| state.totp().code_at_step(TotpSecret::step_at(now)))
            .unwrap()
    }

    fn connect_raw(&self, origin: Option<&str>) -> Result<Socket, tungstenite::Error> {
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

    fn connect(&self) -> Socket {
        self.connect_raw(Some(&self.origin())).unwrap()
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

enum Received<T> {
    Got(T),
    Closed,
    Timeout,
}

struct Client {
    socket: Socket,
    session: Session,
}

impl Client {
    fn send(&mut self, frame: &Frame) {
        for record in self.session.seal(&frame.encode().unwrap()).unwrap() {
            self.socket.send(Message::Binary(record.into())).unwrap();
        }
    }

    fn send_viewer(&mut self, message: ViewerMessage) {
        self.send(&Frame::new(Channel::Desktop, 0, message.encode()).unwrap());
    }

    fn send_control(&mut self, message: ControlMessage) {
        self.send(&Frame::new(Channel::Control, 0, message.encode()).unwrap());
    }

    fn read_frame(&mut self) -> Received<Frame> {
        loop {
            match self.socket.read() {
                Ok(Message::Binary(bytes)) => {
                    if let Some(message) = self.session.open(&bytes).unwrap() {
                        return Received::Got(Frame::decode(&message).unwrap());
                    }
                }
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
                Ok(Message::Close(_)) => return Received::Closed,
                Ok(other) => panic!("unexpected message {other:?}"),
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Received::Timeout;
                }
                Err(_) => return Received::Closed,
            }
        }
    }

    fn read_control(&mut self) -> Received<ControlMessage> {
        match self.read_frame() {
            Received::Got(frame) => {
                assert_eq!(frame.channel, Channel::Control);
                Received::Got(ControlMessage::decode(&frame.payload).unwrap())
            }
            Received::Closed => Received::Closed,
            Received::Timeout => Received::Timeout,
        }
    }

    fn read_desktop(&mut self) -> Received<DesktopMessage> {
        match self.read_frame() {
            Received::Got(frame) => {
                assert_eq!(frame.channel, Channel::Desktop);
                Received::Got(DesktopMessage::decode(&frame.payload).unwrap())
            }
            Received::Closed => Received::Closed,
            Received::Timeout => Received::Timeout,
        }
    }

    fn wait_closed(&mut self, within: Duration) -> bool {
        self.set_timeout(Duration::from_millis(200));
        let deadline = Instant::now() + within;
        let mut closed = false;
        while Instant::now() < deadline {
            match self.read_frame() {
                Received::Closed => {
                    closed = true;
                    break;
                }
                Received::Got(_) | Received::Timeout => {}
            }
        }
        self.set_timeout(Duration::from_secs(10));
        closed
    }

    fn set_timeout(&mut self, timeout: Duration) {
        let MaybeTlsStream::Plain(stream) = self.socket.get_mut() else {
            unreachable!();
        };
        stream.set_read_timeout(Some(timeout)).unwrap();
    }

    fn login(&mut self, code: &str) -> Received<ControlMessage> {
        let digits: [u8; 6] = code.as_bytes().try_into().unwrap();
        self.send_control(ControlMessage::Totp { code: digits });
        self.read_control()
    }

    fn expect_auth_required(&mut self) {
        match self.read_control() {
            Received::Got(ControlMessage::AuthRequired) => {}
            Received::Got(other) => panic!("expected AuthRequired, got {other:?}"),
            Received::Closed => panic!("the connection closed before AuthRequired"),
            Received::Timeout => panic!("timed out waiting for AuthRequired"),
        }
    }

    fn expect_ok(&mut self, code: &str) {
        match self.login(code) {
            Received::Got(ControlMessage::AuthOk) => {}
            Received::Got(other) => panic!("expected AuthOk, got {other:?}"),
            Received::Closed => panic!("the connection closed during login"),
            Received::Timeout => panic!("timed out during login"),
        }
    }
}

fn start_pairing(
    server: &Running,
    device: &StaticKey,
    code: &PairingCode,
    name: &str,
) -> Option<Client> {
    let mut socket = server.connect();
    let (handshake, first) = ViewerHandshake::pairing(device, code, name, VIEWER_LIMITS).unwrap();
    socket.send(Message::Binary(first.into())).unwrap();
    let Ok(Message::Binary(reply)) = socket.read() else {
        return None;
    };
    let outcome = handshake.complete(&reply).ok()?;
    socket
        .send(Message::Binary(outcome.last_message.unwrap().into()))
        .unwrap();
    Some(Client {
        socket,
        session: outcome.session,
    })
}

fn start_reconnect(server: &Running, device: &StaticKey, agent: &PublicKey) -> Option<Client> {
    let mut socket = server.connect();
    let (handshake, first) = ViewerHandshake::reconnect(device, agent, VIEWER_LIMITS).unwrap();
    socket.send(Message::Binary(first.into())).unwrap();
    let Ok(Message::Binary(reply)) = socket.read() else {
        return None;
    };
    let outcome = handshake.complete(&reply).ok()?;
    Some(Client {
        socket,
        session: outcome.session,
    })
}

fn pair_and_login(server: &Running, name: &str) -> (Client, StaticKey) {
    let device = StaticKey::generate().unwrap();
    let code = server.open_pairing();
    let mut client = start_pairing(server, &device, &code, name).expect("pairing handshake");
    client.expect_auth_required();
    client.expect_ok(&server.fresh_totp());
    (client, device)
}

fn reconnect_and_login(server: &Running, device: &StaticKey) -> Client {
    let mut client =
        start_reconnect(server, device, &server.agent_key()).expect("reconnection handshake");
    client.expect_auth_required();
    client.expect_ok(&server.fresh_totp());
    client
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

struct Painter {
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
}

impl Painter {
    fn connect(display: &str) -> Self {
        let (connection, number) = x11rb::connect(Some(display)).unwrap();
        let root = connection.setup().roots[number].root;
        Self { connection, root }
    }

    fn fill_area(&self, area: Rectangle, color: u32) {
        let gc = self.connection.generate_id().unwrap();
        self.connection
            .create_gc(gc, self.root, &CreateGCAux::new().foreground(color))
            .unwrap();
        self.connection
            .poly_fill_rectangle(self.root, gc, &[area])
            .unwrap();
        self.connection.free_gc(gc).unwrap();
        self.connection.sync().unwrap();
    }

    fn fill_rectangle(&self, x: i16, color: u32) {
        let gc = self.connection.generate_id().unwrap();
        self.connection
            .create_gc(gc, self.root, &CreateGCAux::new().foreground(color))
            .unwrap();
        self.connection
            .poly_fill_rectangle(
                self.root,
                gc,
                &[Rectangle {
                    x,
                    y: 10,
                    width: 20,
                    height: 20,
                }],
            )
            .unwrap();
        self.connection.free_gc(gc).unwrap();
        self.connection.sync().unwrap();
    }
}

fn pointer_at(display: &str) -> (i16, i16) {
    let (connection, number) = x11rb::connect(Some(display)).unwrap();
    let root = connection.setup().roots[number].root;
    let reply = connection.query_pointer(root).unwrap().reply().unwrap();
    (reply.root_x, reply.root_y)
}

fn next_tile_or_resize(client: &mut Client) -> DesktopMessage {
    loop {
        match client.read_desktop() {
            Received::Got(DesktopMessage::FrameEnd { sequence }) => {
                client.send_viewer(ViewerMessage::FrameAck { sequence });
            }
            Received::Got(other) => return other,
            Received::Closed => panic!("the stream closed"),
            Received::Timeout => panic!("timed out waiting for the stream"),
        }
    }
}

#[test]
fn pairs_a_new_device_streams_the_screen_and_applies_input() {
    let xvfb = Xvfb::start(60 + std::process::id() % 9);
    paint_root(&xvfb.display, 0x0000_ff00);
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");

    assert_eq!(
        next_tile_or_resize(&mut client),
        DesktopMessage::Resize {
            width: 320,
            height: 200
        }
    );
    let mut covered = 0_u32;
    while covered < 320 * 200 {
        match next_tile_or_resize(&mut client) {
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
            other => panic!("expected a tile, got {other:?}"),
        }
    }

    client.send_viewer(ViewerMessage::Input(InputEvent::PointerMove {
        x: 100,
        y: 50,
    }));
    let deadline = Instant::now() + Duration::from_secs(3);
    while pointer_at(&xvfb.display) != (100, 50) {
        assert!(Instant::now() < deadline, "the pointer never moved");
        sleep(Duration::from_millis(20));
    }
    let painter = Painter::connect(&xvfb.display);
    painter.fill_rectangle(40, 0x00ff_0000);
    assert!(matches!(
        next_tile_or_resize(&mut client),
        DesktopMessage::Tile { .. }
    ));
}

#[test]
fn a_paired_device_reconnects_with_a_fresh_code() {
    let xvfb = Xvfb::start(50 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (client, device) = pair_and_login(&server, "Portatil");
    drop(client);
    sleep(Duration::from_millis(300));
    let mut again = reconnect_and_login(&server, &device);
    assert!(matches!(
        next_tile_or_resize(&mut again),
        DesktopMessage::Resize { .. }
    ));
    let names = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(names, 1);
}

#[test]
fn a_totp_code_cannot_be_used_twice() {
    let xvfb = Xvfb::start(40 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (client, device) = pair_and_login(&server, "Aula 3");
    drop(client);
    sleep(Duration::from_millis(300));
    let used = server.totp_at(server.time.load(Ordering::SeqCst));
    let mut again = start_reconnect(&server, &device, &server.agent_key()).unwrap();
    again.expect_auth_required();
    match again.login(&used) {
        Received::Got(ControlMessage::AuthFailed {
            reason: AuthFailure::WrongCode,
            ..
        }) => {}
        other => panic!("a repeated code was not refused: {}", describe(&other)),
    }
}

fn describe(read: &Received<ControlMessage>) -> String {
    match read {
        Received::Got(message) => format!("{message:?}"),
        Received::Closed => "closed".to_owned(),
        Received::Timeout => "timed out".to_owned(),
    }
}

#[test]
fn an_unknown_device_gets_no_answer_at_all() {
    let xvfb = Xvfb::start(30 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let stranger = StaticKey::generate().unwrap();
    let mut socket = server.connect();
    let (_handshake, first) =
        ViewerHandshake::reconnect(&stranger, &server.agent_key(), VIEWER_LIMITS).unwrap();
    socket.send(Message::Binary(first.into())).unwrap();
    assert!(!matches!(socket.read(), Ok(Message::Binary(_))));
    let log = fs::read_to_string(server.dir.join("state").join("access.log")).unwrap();
    assert!(log.contains("unknown_device"));
}

#[test]
fn a_viewer_that_pinned_the_wrong_agent_key_is_refused() {
    let xvfb = Xvfb::start(20 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (client, device) = pair_and_login(&server, "Aula 3");
    drop(client);
    let impostor = StaticKey::generate().unwrap().public();
    assert!(start_reconnect(&server, &device, &impostor).is_none());
}

#[test]
fn pairing_is_refused_when_no_code_is_open() {
    let xvfb = Xvfb::start(10 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let device = StaticKey::generate().unwrap();
    let guessed = PairingCode::generate().unwrap();
    assert!(start_pairing(&server, &device, &guessed, "Intruso").is_none());
    let count = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn a_wrong_pairing_code_is_refused_and_five_failures_burn_the_real_one() {
    let xvfb = Xvfb::start(70 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let real = server.open_pairing();
    let device = StaticKey::generate().unwrap();
    for _ in 0..5 {
        let wrong = PairingCode::generate().unwrap();
        let mut client = start_pairing(&server, &device, &wrong, "Intruso")
            .expect("the handshake only fails at the last message");
        assert!(client.wait_closed(Duration::from_secs(3)));
    }
    assert!(start_pairing(&server, &device, &real, "Dueno").is_none());
    let count = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn the_pairing_code_alone_does_not_add_a_device() {
    let xvfb = Xvfb::start(80 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let device = StaticKey::generate().unwrap();
    let code = server.open_pairing();
    let mut client = start_pairing(&server, &device, &code, "Sin segundo factor").unwrap();
    client.expect_auth_required();
    for _ in 0..3 {
        let wrong = client.login("000000");
        assert!(matches!(
            wrong,
            Received::Got(ControlMessage::AuthFailed { .. }) | Received::Closed
        ));
    }
    let count = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn wrong_second_factor_codes_during_pairing_also_burn_the_code() {
    let xvfb = Xvfb::start(60 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let real = server.open_pairing();
    let device = StaticKey::generate().unwrap();
    let mut failures = 0;
    while failures < 5 {
        let mut client = start_pairing(&server, &device, &real, "Intruso").unwrap();
        client.expect_auth_required();
        for _ in 0..3 {
            if failures == 5 {
                break;
            }
            match client.login("000000") {
                Received::Got(ControlMessage::AuthFailed { .. }) => failures += 1,
                Received::Closed => break,
                other => panic!("unexpected answer: {}", describe(&other)),
            }
        }
    }
    sleep(Duration::from_millis(300));
    let right = server.fresh_totp();
    if let Some(mut late) = start_pairing(&server, &device, &real, "Dueno") {
        let paired = matches!(
            late.read_control(),
            Received::Got(ControlMessage::AuthRequired)
        ) && matches!(late.login(&right), Received::Got(ControlMessage::AuthOk));
        assert!(!paired, "a burned pairing code still paired a device");
    }
    let count = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn a_pairing_code_works_once() {
    let xvfb = Xvfb::start(90 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let device = StaticKey::generate().unwrap();
    let code = server.open_pairing();
    let mut first = start_pairing(&server, &device, &code, "Primero").unwrap();
    first.expect_auth_required();
    first.expect_ok(&server.fresh_totp());
    drop(first);
    sleep(Duration::from_millis(300));
    let other = StaticKey::generate().unwrap();
    assert!(start_pairing(&server, &other, &code, "Segundo").is_none());
}

#[test]
fn three_wrong_codes_lock_the_device_and_the_lock_ends() {
    let xvfb = Xvfb::start(60 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (client, device) = pair_and_login(&server, "Aula 3");
    drop(client);
    sleep(Duration::from_millis(300));

    let mut attacker = start_reconnect(&server, &device, &server.agent_key()).unwrap();
    attacker.expect_auth_required();
    let mut last = None;
    for _ in 0..3 {
        last = Some(attacker.login("000000"));
        if matches!(last, Some(Received::Closed)) {
            break;
        }
    }
    match last.unwrap() {
        Received::Got(ControlMessage::AuthFailed {
            reason: AuthFailure::Locked,
            retry_after,
        }) => assert!((1..=30).contains(&retry_after)),
        other => panic!("the device was not locked: {}", describe(&other)),
    }
    drop(attacker);
    sleep(Duration::from_millis(300));

    let mut during = start_reconnect(&server, &device, &server.agent_key()).unwrap();
    match during.read_control() {
        Received::Got(ControlMessage::AuthFailed {
            reason: AuthFailure::Locked,
            ..
        }) => {}
        other => panic!("a locked device was asked for a code: {}", describe(&other)),
    }
    drop(during);

    server.time.fetch_add(120, Ordering::SeqCst);
    let mut owner = start_reconnect(&server, &device, &server.agent_key()).unwrap();
    owner.expect_auth_required();
    owner.expect_ok(&server.fresh_totp());
}

#[test]
fn a_lock_on_one_device_does_not_stop_another() {
    let xvfb = Xvfb::start(50 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (first, victim) = pair_and_login(&server, "Robado");
    drop(first);
    sleep(Duration::from_millis(300));
    let (second, owner) = pair_and_login(&server, "Mio");
    drop(second);
    sleep(Duration::from_millis(300));

    let mut attacker = start_reconnect(&server, &victim, &server.agent_key()).unwrap();
    attacker.expect_auth_required();
    for _ in 0..3 {
        if matches!(attacker.login("000000"), Received::Closed) {
            break;
        }
    }
    drop(attacker);
    sleep(Duration::from_millis(300));
    let _mine = reconnect_and_login(&server, &owner);
}

#[test]
fn a_locked_agent_refuses_every_connection() {
    let xvfb = Xvfb::start(40 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (client, device) = pair_and_login(&server, "Aula 3");
    drop(client);
    sleep(Duration::from_millis(300));
    server
        .store
        .update(|state, _| state.set_locked(true))
        .unwrap();
    assert!(start_reconnect(&server, &device, &server.agent_key()).is_none());
    server
        .store
        .update(|state, _| state.set_locked(false))
        .unwrap();
    let _again = reconnect_and_login(&server, &device);
}

#[test]
fn a_revoked_device_is_cut_off_while_connected_and_cannot_return() {
    let xvfb = Xvfb::start(30 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (mut client, device) = pair_and_login(&server, "Aula 3");
    assert!(matches!(
        next_tile_or_resize(&mut client),
        DesktopMessage::Resize { .. }
    ));
    let key = device.public();
    server
        .store
        .update(|state, _| state.remove_device(&key))
        .unwrap()
        .unwrap();
    assert!(
        client.wait_closed(Duration::from_secs(6)),
        "the revoked viewer stayed connected"
    );
    assert!(start_reconnect(&server, &device, &server.agent_key()).is_none());
}

#[test]
fn an_idle_unauthenticated_connection_does_not_take_the_viewer_slot() {
    let xvfb = Xvfb::start(20 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (first, device_a) = pair_and_login(&server, "A");
    drop(first);
    sleep(Duration::from_millis(300));
    let (second, device_b) = pair_and_login(&server, "B");
    drop(second);
    sleep(Duration::from_millis(300));

    let mut idle = start_reconnect(&server, &device_b, &server.agent_key()).unwrap();
    idle.expect_auth_required();
    let _owner = reconnect_and_login(&server, &device_a);
    match idle.login(&server.fresh_totp()) {
        Received::Got(ControlMessage::AuthFailed {
            reason: AuthFailure::Busy,
            ..
        }) => {}
        other => panic!(
            "a second viewer was not told it is busy: {}",
            describe(&other)
        ),
    }
}

#[test]
fn only_a_few_unauthenticated_connections_are_accepted_at_once() {
    let xvfb = Xvfb::start(10 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let mut idle = Vec::new();
    for _ in 0..16 {
        idle.push(server.connect());
        sleep(Duration::from_millis(20));
    }
    let error = server.connect_raw(Some(&server.origin())).unwrap_err();
    assert!(error.to_string().contains("503"), "{error}");
    drop(idle);
}

#[test]
fn garbage_after_authentication_ends_the_session_and_the_next_viewer_gets_in() {
    let xvfb = Xvfb::start(70 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (mut client, device) = pair_and_login(&server, "Aula 3");
    client
        .socket
        .send(Message::Binary(
            vec![
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
            ]
            .into(),
        ))
        .unwrap();
    assert!(
        client.wait_closed(Duration::from_secs(5)),
        "the session was not closed"
    );
    let mut next_attempt = Instant::now() + Duration::from_secs(5);
    loop {
        server.time.fetch_add(31, Ordering::SeqCst);
        if let Some(mut again) = start_reconnect(&server, &device, &server.agent_key()) {
            again.expect_auth_required();
            let code = server.totp_at(server.time.load(Ordering::SeqCst));
            if matches!(again.login(&code), Received::Got(ControlMessage::AuthOk)) {
                break;
            }
        }
        assert!(
            Instant::now() < next_attempt,
            "the next viewer never got in"
        );
        next_attempt = next_attempt.max(Instant::now());
        sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_tampered_record_ends_the_session() {
    let xvfb = Xvfb::start(80 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    let mut records = client
        .session
        .seal(
            &Frame::new(
                Channel::Desktop,
                0,
                ViewerMessage::Input(InputEvent::PointerMove { x: 1, y: 1 }).encode(),
            )
            .unwrap()
            .encode()
            .unwrap(),
        )
        .unwrap();
    records[0][5] ^= 0x40;
    client
        .socket
        .send(Message::Binary(records.remove(0).into()))
        .unwrap();
    assert!(
        client.wait_closed(Duration::from_secs(5)),
        "the tampered session stayed open"
    );
}

const BATCH_LIMIT: u32 = 8;

fn next_frame_end(client: &mut Client) -> u32 {
    loop {
        match client.read_desktop() {
            Received::Got(DesktopMessage::FrameEnd { sequence }) => return sequence,
            Received::Got(_) => {}
            Received::Closed => panic!("the stream closed"),
            Received::Timeout => panic!("timed out waiting for the stream"),
        }
    }
}

fn nothing_arrives_within(client: &mut Client, wait: Duration) -> bool {
    let MaybeTlsStream::Plain(stream) = client.socket.get_mut() else {
        unreachable!();
    };
    stream.set_read_timeout(Some(wait)).unwrap();
    let silent = matches!(
        client.socket.read(),
        Err(tungstenite::Error::Io(ref e))
            if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
    );
    let MaybeTlsStream::Plain(stream) = client.socket.get_mut() else {
        unreachable!();
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    silent
}

#[test]
fn stops_sending_until_the_viewer_acknowledges_batches() {
    let xvfb = Xvfb::start(10 + std::process::id() % 9);
    let painter = Painter::connect(&xvfb.display);
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");

    assert_eq!(next_frame_end(&mut client), 0);
    for expected in 1..BATCH_LIMIT {
        painter.fill_rectangle(
            i16::try_from(expected * 30).unwrap(),
            0x00ff_0000 + expected,
        );
        assert_eq!(next_frame_end(&mut client), expected);
    }
    painter.fill_rectangle(130, 0x0000_ff00);
    assert!(
        nothing_arrives_within(&mut client, Duration::from_millis(800)),
        "the agent kept sending although the batch limit was reached"
    );
    client.send_viewer(ViewerMessage::FrameAck { sequence: 0 });
    assert_eq!(next_frame_end(&mut client), BATCH_LIMIT);
}

fn decoded_center(picture: &openh264::decoder::DecodedYUV<'_>) -> [u8; 3] {
    use openh264::formats::YUVSource;
    let (width, height) = picture.dimensions();
    let (luma_stride, chroma_stride, _) = picture.strides();
    let (row, column) = (height / 2, width / 2);
    let luma = (f64::from(picture.y()[row * luma_stride + column]) - 16.0) / 219.0;
    let blue_difference =
        (f64::from(picture.u()[(row / 2) * chroma_stride + column / 2]) - 128.0) / 224.0;
    let red_difference =
        (f64::from(picture.v()[(row / 2) * chroma_stride + column / 2]) - 128.0) / 224.0;
    let channels = [
        luma + 1.5748 * red_difference,
        luma - 0.1873 * blue_difference - 0.4681 * red_difference,
        luma + 1.8556 * blue_difference,
    ];
    channels.map(|value| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (value * 255.0).round().clamp(0.0, 255.0) as u8;
        byte
    })
}

#[test]
fn a_capable_viewer_gets_h264_under_heavy_activity_and_tiles_again_when_it_calms_down() {
    let xvfb = Xvfb::start(30 + std::process::id() % 9);
    let painter = Painter::connect(&xvfb.display);
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.send_viewer(ViewerMessage::Capabilities { video: true });
    client.set_timeout(Duration::from_millis(150));

    let palette: [(u32, [u8; 3]); 3] = [
        (0x00ff_0000, [255, 0, 0]),
        (0x0000_ff00, [0, 255, 0]),
        (0x0000_00ff, [0, 0, 255]),
    ];
    let painting = AtomicBool::new(true);
    let mut viewer = openh264::decoder::Decoder::new().unwrap();
    let mut videos = 0;
    let mut matching = 0;
    let mut keyframes = 0;
    thread::scope(|scope| {
        scope.spawn(|| {
            let mut round = 0_usize;
            while painting.load(Ordering::SeqCst) {
                let (color, _) = palette[round % palette.len()];
                painter.fill_area(
                    Rectangle {
                        x: 0,
                        y: 0,
                        width: 320,
                        height: 160,
                    },
                    color,
                );
                round += 1;
                sleep(Duration::from_millis(20));
            }
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && !(videos >= 6 && matching >= 3) {
            match client.read_desktop() {
                Received::Got(DesktopMessage::FrameEnd { sequence }) => {
                    client.send_viewer(ViewerMessage::FrameAck { sequence });
                }
                Received::Got(DesktopMessage::Video {
                    keyframe,
                    data,
                    width,
                    height,
                }) => {
                    assert_eq!((width, height), (320, 200));
                    videos += 1;
                    keyframes += usize::from(keyframe);
                    if let Some(picture) = viewer.decode(&data).unwrap() {
                        let center = decoded_center(&picture);
                        if palette.iter().any(|(_, rgb)| {
                            rgb.iter().zip(center).all(|(a, b)| a.abs_diff(b) <= 14)
                        }) {
                            matching += 1;
                        }
                    }
                }
                Received::Got(_) | Received::Timeout => {}
                Received::Closed => panic!("the stream closed"),
            }
        }
        painting.store(false, Ordering::SeqCst);
    });
    assert!(videos >= 6, "only {videos} video pictures arrived");
    assert!(keyframes >= 1, "no keyframe arrived");
    assert!(
        matching >= 3,
        "only {matching} decoded pictures had the painted colours"
    );

    let mut refreshed = 0_u64;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && refreshed < 320 * 200 {
        match client.read_desktop() {
            Received::Got(DesktopMessage::FrameEnd { sequence }) => {
                client.send_viewer(ViewerMessage::FrameAck { sequence });
            }
            Received::Got(DesktopMessage::Tile { width, height, .. }) => {
                refreshed += u64::from(width) * u64::from(height);
            }
            Received::Got(_) | Received::Timeout => {}
            Received::Closed => panic!("the stream closed"),
        }
    }
    assert!(
        refreshed >= 320 * 200,
        "the screen was not refreshed with tiles: {refreshed}"
    );
}

#[test]
fn a_viewer_that_does_not_declare_video_support_keeps_getting_tiles_under_heavy_activity() {
    let xvfb = Xvfb::start(40 + std::process::id() % 9);
    let painter = Painter::connect(&xvfb.display);
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(150));
    let mut tiles = 0;
    for round in 0..40_u32 {
        painter.fill_area(
            Rectangle {
                x: 0,
                y: 0,
                width: 320,
                height: 160,
            },
            0x00ff_0000 + round,
        );
        loop {
            match client.read_desktop() {
                Received::Got(DesktopMessage::FrameEnd { sequence }) => {
                    client.send_viewer(ViewerMessage::FrameAck { sequence });
                }
                Received::Got(DesktopMessage::Video { .. }) => {
                    panic!("video sent to a viewer that cannot decode it")
                }
                Received::Got(DesktopMessage::Tile { .. }) => tiles += 1,
                Received::Got(_) => {}
                Received::Timeout => break,
                Received::Closed => panic!("the stream closed"),
            }
        }
    }
    assert!(tiles > 10);
}

#[test]
fn rejects_foreign_origins_and_hosts() {
    let xvfb = Xvfb::start(50 + std::process::id() % 9);
    let server = Running::start(&xvfb.display);

    let error = server.connect_raw(Some("http://evil.example")).unwrap_err();
    assert!(error.to_string().contains("403"), "{error}");
    let error = server.connect_raw(Some("null")).unwrap_err();
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
fn refuses_to_bind_outside_the_loopback_interface() {
    let result = Server::bind(ServerConfig {
        listen: "0.0.0.0:0".parse().unwrap(),
        display: None,
        viewer_dir: PathBuf::from("."),
        jpeg_quality: 70,
        state_dir: PathBuf::from("."),
        clock: None,
    });
    assert!(matches!(result, Err(ServerError::NotLoopback(_))));
}

#[test]
fn refuses_to_start_without_an_agent_identity() {
    let dir = std::env::temp_dir().join(format!("wirehaus-noinit-{}", std::process::id()));
    let result = Server::bind(ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        display: None,
        viewer_dir: PathBuf::from("."),
        jpeg_quality: 70,
        state_dir: dir,
        clock: None,
    });
    assert!(matches!(result, Err(ServerError::Store(_))));
}
