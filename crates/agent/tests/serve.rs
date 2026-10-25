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
use wirehaus_agent::state::{ClipboardMode, State};
use wirehaus_agent::{Clock, Server, ServerConfig, ServerError, Store};
use wirehaus_clipboard::{Clipboard, X11Clipboard};
use wirehaus_crypto::handshake::ViewerHandshake;
use wirehaus_crypto::keys::{PublicKey, StaticKey};
use wirehaus_crypto::pairing::PairingCode;
use wirehaus_crypto::session::{Limits, Session};
use wirehaus_crypto::totp::TotpSecret;
use wirehaus_protocol::{
    AudioFailure, AudioMessage, AuthFailure, Button, Channel, ClipboardFailure, ClipboardMessage,
    Codec, ControlMessage, DesktopMessage, EndReason, FailureReason, FilesMessage, Frame,
    HEADER_LEN, InputEvent, MAX_PAYLOAD_LEN, RefusalReason, TerminalMessage, ViewerMessage,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt, CreateGCAux, KeyButMask, Rectangle,
};
use x11rb::wrapper::ConnectionExt as _;

const START_TIME: u64 = 1_700_000_000;
const VIEWER_LIMITS: Limits = Limits {
    send: wirehaus_protocol::MAX_VIEWER_MESSAGE,
    receive: HEADER_LEN + MAX_PAYLOAD_LEN,
};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

struct Xvfb {
    child: Child,
    display: String,
}

static DISPLAY_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl Xvfb {
    fn start_unique() -> Self {
        let offset = u32::try_from(DISPLAY_COUNTER.fetch_add(1, Ordering::SeqCst)).unwrap();
        Self::start(200 + (std::process::id() % 300) * 40 + offset)
    }

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
        let number = self.display.trim_start_matches(':');
        let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
        let _ = std::fs::remove_file(format!("/tmp/.X11-unix/X{number}"));
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
        Self::start_with_idle(display, Duration::from_secs(3600))
    }

    fn start_with_idle(display: &str, idle_timeout: Duration) -> Self {
        Self::start_configured(display, idle_timeout, wirehaus_agent::Exposure::default())
    }

    fn start_configured(
        display: &str,
        idle_timeout: Duration,
        exposure: wirehaus_agent::Exposure,
    ) -> Self {
        Self::start_inner(display, idle_timeout, exposure, None)
    }

    fn start_with_vault(display: &str, vault: &wirehaus_vault::MemoryVault) -> Self {
        Self::start_inner(
            display,
            Duration::from_secs(3600),
            wirehaus_agent::Exposure::default(),
            Some(vault),
        )
    }

    fn start_inner(
        display: &str,
        idle_timeout: Duration,
        exposure: wirehaus_agent::Exposure,
        vault: Option<&wirehaus_vault::MemoryVault>,
    ) -> Self {
        let root = std::env::temp_dir().join(format!(
            "wirehaus-serve-{}-{}",
            std::process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let viewer = root.join("viewer");
        fs::create_dir_all(&viewer).unwrap();
        fs::write(viewer.join("index.html"), b"<title>viewer</title>").unwrap();
        wirehaus_agent::static_files::write_manifest(&viewer).unwrap();
        let time = Arc::new(AtomicU64::new(START_TIME));
        let clock: Clock = {
            let shared = Arc::clone(&time);
            Arc::new(move || shared.load(Ordering::SeqCst))
        };
        let state_dir = root.join("state");
        let mut store = Store::initialize(&state_dir, Arc::clone(&clock)).unwrap();
        let source: Option<wirehaus_agent::VaultSource> = vault.map(|vault| {
            let vault = vault.clone();
            Arc::new(move || Ok(Box::new(vault.clone()) as Box<dyn wirehaus_vault::Vault>))
                as wirehaus_agent::VaultSource
        });
        if let Some(source) = &source {
            store = Store::open_with(&state_dir, Arc::clone(&clock), Arc::clone(source)).unwrap();
            store
                .move_secrets(wirehaus_agent::state::SecretStorage::System)
                .unwrap();
        }
        let server = Server::bind(ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            display: Some(display.to_owned()),
            viewer_dir: viewer,
            jpeg_quality: 70,
            state_dir,
            clock: Some(clock),
            vault: source,
            idle_timeout,
            viewer_hash: None,
            exposure,
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
    ticket: Option<[u8; 32]>,
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
                        let frame = Frame::decode(&message).unwrap();
                        if frame.channel == Channel::Control
                            && let Ok(ControlMessage::Ticket { ticket }) =
                                ControlMessage::decode(&frame.payload)
                        {
                            self.ticket = Some(ticket);
                            continue;
                        }
                        return Received::Got(frame);
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

    fn resume(&mut self, ticket: [u8; 32]) -> Received<ControlMessage> {
        self.send_control(ControlMessage::Resume { ticket });
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
        ticket: None,
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
        ticket: None,
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

const SHIFT_KEYCODE: usize = 50;

fn held_input(display: &str) -> (bool, bool) {
    let (connection, number) = x11rb::connect(Some(display)).unwrap();
    let root = connection.setup().roots[number].root;
    let pointer = connection.query_pointer(root).unwrap().reply().unwrap();
    let keys = connection.query_keymap().unwrap().reply().unwrap();
    let shift = keys.keys[SHIFT_KEYCODE / 8] & (1 << (SHIFT_KEYCODE % 8)) != 0;
    let button = u16::from(pointer.mask) & u16::from(KeyButMask::BUTTON1) != 0;
    (shift, button)
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
    let xvfb = Xvfb::start_unique();
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
fn a_dropped_viewer_leaves_no_key_or_button_held_down() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.send_viewer(ViewerMessage::Input(InputEvent::Key {
        code: 42,
        pressed: true,
    }));
    client.send_viewer(ViewerMessage::Input(InputEvent::Button {
        button: Button::Left,
        pressed: true,
    }));
    let deadline = Instant::now() + Duration::from_secs(3);
    while held_input(&xvfb.display) != (true, true) {
        assert!(Instant::now() < deadline, "the input never arrived");
        sleep(Duration::from_millis(20));
    }
    drop(client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while held_input(&xvfb.display) != (false, false) {
        assert!(
            Instant::now() < deadline,
            "the agent left input held down after the viewer vanished: {:?}",
            held_input(&xvfb.display)
        );
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_agent_whose_secrets_live_in_the_vault_pairs_and_serves_like_any_other() {
    let xvfb = Xvfb::start_unique();
    let vault = wirehaus_vault::MemoryVault::new();
    let server = Running::start_with_vault(&xvfb.display, &vault);
    let state_file = std::fs::read_to_string(server.dir.join("state").join("state")).unwrap();
    assert!(state_file.contains("secrets system"));
    assert!(!state_file.contains("agent_private"));
    assert_eq!(vault.len(), 2);

    let (mut client, device) = pair_and_login(&server, "Aula 3");
    let record = client.read_frame();
    assert!(matches!(record, Received::Got(_) | Received::Timeout));
    drop(client);
    sleep(Duration::from_millis(300));
    let mut again = reconnect_and_login(&server, &device);
    assert!(matches!(
        again.read_frame(),
        Received::Got(_) | Received::Timeout
    ));
}

#[test]
fn a_paired_device_reconnects_with_a_fresh_code() {
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (client, device) = pair_and_login(&server, "Aula 3");
    drop(client);
    let impostor = StaticKey::generate().unwrap().public();
    assert!(start_reconnect(&server, &device, &impostor).is_none());
}

#[test]
fn pairing_is_refused_when_no_code_is_open() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let device = StaticKey::generate().unwrap();
    let guessed = PairingCode::generate().unwrap();
    assert!(start_pairing(&server, &device, &guessed, "Intruso").is_none());
    let count = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn wrong_pairing_codes_are_refused_and_cannot_burn_the_real_one() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let real = server.open_pairing();
    let device = StaticKey::generate().unwrap();
    for _ in 0..8 {
        let wrong = PairingCode::generate().unwrap();
        let mut client = start_pairing(&server, &device, &wrong, "Intruso")
            .expect("the handshake only fails at the last message");
        assert!(client.wait_closed(Duration::from_secs(3)));
    }
    let count = server.store.read(|state, _| state.devices().len()).unwrap();
    assert_eq!(count, 0);
    let mut owner = start_pairing(&server, &device, &real, "Dueno")
        .expect("a stranger must not be able to burn the pairing code");
    owner.expect_auth_required();
    let right = server.fresh_totp();
    assert!(matches!(
        owner.login(&right),
        Received::Got(ControlMessage::AuthOk)
    ));
}

#[test]
fn the_pairing_code_alone_does_not_add_a_device() {
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
fn a_viewer_that_logs_in_is_told_when_the_agent_cannot_open_the_display() {
    let server = Running::start(":58");
    let device = StaticKey::generate().unwrap();
    let code = server.open_pairing();
    let mut client = start_pairing(&server, &device, &code, "Aula 3").expect("pairing handshake");
    client.expect_auth_required();
    match client.login(&server.fresh_totp()) {
        Received::Got(ControlMessage::AuthFailed {
            reason: AuthFailure::Unavailable,
            retry_after: 0,
        }) => {}
        other => panic!(
            "the viewer was not told the agent is unavailable: {}",
            describe(&other)
        ),
    }
}

#[test]
fn only_a_few_unauthenticated_connections_are_accepted_at_once() {
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
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

fn send_terminal(client: &mut Client, stream: u32, message: &TerminalMessage) {
    client.send(&Frame::new(Channel::Terminal, stream, message.encode()).unwrap());
}

fn next_terminal(client: &mut Client) -> (u32, TerminalMessage) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match client.read_frame() {
            Received::Got(frame) if frame.channel == Channel::Terminal => {
                return (
                    frame.stream,
                    TerminalMessage::decode(&frame.payload).unwrap(),
                );
            }
            Received::Got(frame) => {
                if let Ok(DesktopMessage::FrameEnd { sequence }) =
                    DesktopMessage::decode(&frame.payload)
                {
                    client.send_viewer(ViewerMessage::FrameAck { sequence });
                }
            }
            Received::Timeout => {}
            Received::Closed => panic!("the connection closed"),
        }
    }
    panic!("no terminal message arrived");
}

fn terminal_text_until(client: &mut Client, stream: u32, wanted: &str) -> String {
    let mut seen = String::new();
    while !seen.contains(wanted) {
        match next_terminal(client) {
            (id, TerminalMessage::Output(data)) if id == stream => {
                seen.push_str(&String::from_utf8_lossy(&data));
            }
            (_, TerminalMessage::Exited { .. }) => break,
            _ => {}
        }
    }
    seen
}

fn terminal_texts_until(
    client: &mut Client,
    wanted: &[(u32, &str)],
) -> std::collections::HashMap<u32, String> {
    let mut seen: std::collections::HashMap<u32, String> = wanted
        .iter()
        .map(|&(stream, _)| (stream, String::new()))
        .collect();
    while wanted
        .iter()
        .any(|&(stream, text)| !seen[&stream].contains(text))
    {
        if let (id, TerminalMessage::Output(data)) = next_terminal(client)
            && let Some(text) = seen.get_mut(&id)
        {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }
    seen
}

#[test]
fn a_terminal_is_refused_until_the_owner_allows_it_and_then_runs_commands() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));

    send_terminal(
        &mut client,
        5,
        &TerminalMessage::Open {
            columns: 80,
            rows: 24,
        },
    );
    assert_eq!(
        next_terminal(&mut client),
        (5, TerminalMessage::Refused(RefusalReason::NotAllowed))
    );

    server
        .store
        .update(|state, _| state.set_terminal_allowed(true))
        .unwrap();
    send_terminal(
        &mut client,
        5,
        &TerminalMessage::Open {
            columns: 80,
            rows: 24,
        },
    );
    send_terminal(
        &mut client,
        5,
        &TerminalMessage::Input(b"echo remote-$((6*7))\n".to_vec()),
    );
    let seen = terminal_text_until(&mut client, 5, "remote-42");
    assert!(seen.contains("remote-42"), "{seen}");

    send_terminal(
        &mut client,
        5,
        &TerminalMessage::Input(b"exit 7\n".to_vec()),
    );
    loop {
        if let (5, TerminalMessage::Exited { code }) = next_terminal(&mut client) {
            assert_eq!(code, 7);
            break;
        }
    }
}

#[test]
fn removing_the_terminal_permission_closes_the_terminals_that_are_open() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    server
        .store
        .update(|state, _| state.set_terminal_allowed(true))
        .unwrap();
    for stream in [3, 4] {
        send_terminal(
            &mut client,
            stream,
            &TerminalMessage::Open {
                columns: 80,
                rows: 24,
            },
        );
    }
    send_terminal(
        &mut client,
        3,
        &TerminalMessage::Input(b"echo alive-$((6*7))\n".to_vec()),
    );
    let seen = terminal_text_until(&mut client, 3, "alive-42");
    assert!(seen.contains("alive-42"), "{seen}");

    server
        .store
        .update(|state, _| state.set_terminal_allowed(false))
        .unwrap();
    let mut ended = std::collections::BTreeSet::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while ended.len() < 2 && Instant::now() < deadline {
        if let (stream, TerminalMessage::Exited { .. }) = next_terminal(&mut client) {
            ended.insert(stream);
        }
    }
    assert_eq!(ended.into_iter().collect::<Vec<_>>(), [3, 4]);
    send_terminal(
        &mut client,
        3,
        &TerminalMessage::Input(b"echo still-here\n".to_vec()),
    );
    send_terminal(
        &mut client,
        6,
        &TerminalMessage::Open {
            columns: 80,
            rows: 24,
        },
    );
    assert_eq!(
        next_terminal(&mut client),
        (6, TerminalMessage::Refused(RefusalReason::NotAllowed))
    );
}

#[test]
fn terminal_sessions_are_independent_and_limited() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    server
        .store
        .update(|state, _| state.set_terminal_allowed(true))
        .unwrap();
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    for stream in 1..=4 {
        send_terminal(
            &mut client,
            stream,
            &TerminalMessage::Open {
                columns: 80,
                rows: 24,
            },
        );
    }
    send_terminal(
        &mut client,
        9,
        &TerminalMessage::Open {
            columns: 80,
            rows: 24,
        },
    );
    assert_eq!(
        next_terminal(&mut client),
        (9, TerminalMessage::Refused(RefusalReason::TooManySessions))
    );
    send_terminal(
        &mut client,
        1,
        &TerminalMessage::Input(b"echo one-$((1))\n".to_vec()),
    );
    send_terminal(
        &mut client,
        2,
        &TerminalMessage::Input(b"echo two-$((2))\n".to_vec()),
    );
    let seen = terminal_texts_until(&mut client, &[(1, "one-1"), (2, "two-2")]);
    assert!(seen[&1].contains("one-1"));
    assert!(!seen[&1].contains("two-2"));
    assert!(seen[&2].contains("two-2"));
    assert!(!seen[&2].contains("one-1"));
}

#[test]
fn a_malformed_terminal_message_ends_the_session() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.send(&Frame::new(Channel::Terminal, 1, vec![99, 1, 2]).unwrap());
    assert!(client.wait_closed(Duration::from_secs(5)));
}

struct SharedFolder {
    base: PathBuf,
}

impl SharedFolder {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "wirehaus-serve-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(base.join("shared/docs")).unwrap();
        fs::create_dir_all(base.join("private")).unwrap();
        fs::write(base.join("shared/docs/note.txt"), b"remote note").unwrap();
        fs::write(base.join("private/secret.txt"), b"secret").unwrap();
        Self { base }
    }

    fn shared(&self) -> PathBuf {
        self.base.join("shared").canonicalize().unwrap()
    }
}

impl Drop for SharedFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn send_files(client: &mut Client, stream: u32, message: &FilesMessage) {
    client.send(&Frame::new(Channel::Files, stream, message.encode()).unwrap());
}

fn next_files(client: &mut Client) -> (u32, FilesMessage) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match client.read_frame() {
            Received::Got(frame) if frame.channel == Channel::Files => {
                return (frame.stream, FilesMessage::decode(&frame.payload).unwrap());
            }
            Received::Got(frame) => {
                if let Ok(DesktopMessage::FrameEnd { sequence }) =
                    DesktopMessage::decode(&frame.payload)
                {
                    client.send_viewer(ViewerMessage::FrameAck { sequence });
                }
            }
            Received::Timeout => {}
            Received::Closed => panic!("the connection closed"),
        }
    }
    panic!("no files message arrived");
}

fn share(server: &Running, folder: &SharedFolder) {
    let root = folder.shared();
    server
        .store
        .update(|state, _| state.add_file_root(&root))
        .unwrap()
        .unwrap();
}

fn download(client: &mut Client, stream: u32, path: &str) -> Result<Vec<u8>, FailureReason> {
    let request = FilesMessage::Download {
        root: 0,
        path: path.to_owned(),
    };
    send_files(client, stream, &request);
    let mut data = Vec::new();
    loop {
        match next_files(client) {
            (id, FilesMessage::Ready { .. }) if id == stream => {}
            (id, FilesMessage::Data(chunk)) if id == stream => {
                data.extend_from_slice(&chunk);
                let bytes = data.len() as u64;
                send_files(client, stream, &FilesMessage::Ack { bytes });
            }
            (id, FilesMessage::Done) if id == stream => return Ok(data),
            (id, FilesMessage::Failed(reason)) if id == stream => return Err(reason),
            _ => {}
        }
    }
}

fn upload(client: &mut Client, stream: u32, path: &str, content: &[u8]) -> FilesMessage {
    let request = FilesMessage::Upload {
        root: 0,
        path: path.to_owned(),
        size: content.len() as u64,
    };
    send_files(client, stream, &request);
    let reply = next_files(client);
    if !matches!(reply, (id, FilesMessage::Ready { .. }) if id == stream) {
        return reply.1;
    }
    for chunk in content.chunks(wirehaus_protocol::MAX_CHUNK) {
        send_files(client, stream, &FilesMessage::Data(chunk.to_vec()));
    }
    send_files(client, stream, &FilesMessage::Done);
    loop {
        match next_files(client) {
            (id, FilesMessage::Ack { .. }) if id == stream => {}
            (id, other) if id == stream => return other,
            _ => {}
        }
    }
}

#[test]
fn files_are_refused_until_a_folder_is_shared_and_then_move_both_ways() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let folder = SharedFolder::new();
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));

    send_files(
        &mut client,
        1,
        &FilesMessage::List {
            root: 0,
            path: String::new(),
        },
    );
    assert_eq!(
        next_files(&mut client),
        (1, FilesMessage::Failed(FailureReason::NotAllowed))
    );

    share(&server, &folder);
    send_files(&mut client, 2, &FilesMessage::ListRoots);
    let (_, roots) = next_files(&mut client);
    assert_eq!(roots, FilesMessage::Roots(vec!["shared".to_owned()]));
    send_files(
        &mut client,
        3,
        &FilesMessage::List {
            root: 0,
            path: "docs".to_owned(),
        },
    );
    let (_, listing) = next_files(&mut client);
    let FilesMessage::Listing { entries, .. } = listing else {
        panic!("{listing:?}")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "note.txt");

    assert_eq!(
        download(&mut client, 4, "docs/note.txt").unwrap(),
        b"remote note"
    );
    let content: Vec<u8> = (0..600_000_u32).map(|n| (n % 253) as u8).collect();
    assert_eq!(
        upload(&mut client, 5, "docs/up.bin", &content),
        FilesMessage::Done
    );
    assert_eq!(
        fs::read(folder.shared().join("docs/up.bin")).unwrap(),
        content
    );
    assert_eq!(download(&mut client, 6, "docs/up.bin").unwrap(), content);
}

#[test]
fn files_outside_the_shared_folder_cannot_be_reached() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let folder = SharedFolder::new();
    share(&server, &folder);
    std::os::unix::fs::symlink(folder.base.join("private"), folder.shared().join("escape"))
        .unwrap();
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    for path in [
        "../private/secret.txt",
        "escape/secret.txt",
        "/etc/passwd",
        "docs/../../private/secret.txt",
    ] {
        assert_eq!(
            download(&mut client, 1, path),
            Err(FailureReason::InvalidPath),
            "{path}"
        );
    }
    assert_eq!(
        upload(&mut client, 2, "escape/planted.txt", b"x"),
        FilesMessage::Failed(FailureReason::InvalidPath)
    );
    assert!(!folder.base.join("private/planted.txt").exists());
}

#[test]
fn unsharing_the_folder_stops_a_transfer_in_progress() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let folder = SharedFolder::new();
    share(&server, &folder);
    fs::write(folder.shared().join("big.bin"), vec![1_u8; 8 << 20]).unwrap();
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    send_files(
        &mut client,
        1,
        &FilesMessage::Download {
            root: 0,
            path: "big.bin".to_owned(),
        },
    );
    assert!(matches!(
        next_files(&mut client).1,
        FilesMessage::Ready { .. }
    ));
    let root = folder.shared();
    server
        .store
        .update(|state, _| state.remove_file_root(&root))
        .unwrap()
        .unwrap();
    loop {
        match next_files(&mut client).1 {
            FilesMessage::Data(_) => {}
            FilesMessage::Failed(FailureReason::NotAllowed) => break,
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_malformed_files_message_ends_the_session() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.send(&Frame::new(Channel::Files, 1, vec![99, 1, 2]).unwrap());
    assert!(client.wait_closed(Duration::from_secs(5)));
}

impl Client {
    fn wait_for_ticket(&mut self) -> [u8; 32] {
        self.set_timeout(Duration::from_millis(300));
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.ticket.is_none() && Instant::now() < deadline {
            if let Received::Got(frame) = self.read_frame()
                && let Ok(DesktopMessage::FrameEnd { sequence }) =
                    DesktopMessage::decode(&frame.payload)
            {
                self.send_viewer(ViewerMessage::FrameAck { sequence });
            }
        }
        self.ticket.expect("the agent never sent a ticket")
    }

    fn read_until_session_ends(&mut self, within: Duration) -> Option<EndReason> {
        self.set_timeout(Duration::from_millis(200));
        let deadline = Instant::now() + within;
        let mut reason = None;
        while Instant::now() < deadline {
            match self.read_frame() {
                Received::Got(frame) => {
                    if frame.channel == Channel::Control
                        && let Ok(ControlMessage::SessionEnded { reason: ended }) =
                            ControlMessage::decode(&frame.payload)
                    {
                        reason = Some(ended);
                    } else if let Ok(DesktopMessage::FrameEnd { sequence }) =
                        DesktopMessage::decode(&frame.payload)
                    {
                        self.send_viewer(ViewerMessage::FrameAck { sequence });
                    }
                }
                Received::Closed => return reason,
                Received::Timeout => {}
            }
        }
        None
    }
}

fn resume_when_possible(server: &Running, device: &StaticKey, ticket: [u8; 32]) -> Option<Client> {
    for _ in 0..60 {
        let mut client =
            start_reconnect(server, device, &server.agent_key()).expect("reconnection handshake");
        client.expect_auth_required();
        match client.resume(ticket) {
            Received::Got(ControlMessage::AuthOk) => return Some(client),
            Received::Got(ControlMessage::AuthRequired) => {
                drop(client);
                sleep(Duration::from_millis(100));
            }
            other => panic!("unexpected answer to a ticket: {}", describe(&other)),
        }
    }
    None
}

fn refused_ticket(server: &Running, device: &StaticKey, ticket: [u8; 32]) -> bool {
    let mut client =
        start_reconnect(server, device, &server.agent_key()).expect("reconnection handshake");
    client.expect_auth_required();
    matches!(
        client.resume(ticket),
        Received::Got(ControlMessage::AuthRequired)
    )
}

#[test]
fn a_ticket_resumes_a_dropped_session_without_a_new_code_and_only_once() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut first, device) = pair_and_login(&server, "Aula 3");
    let ticket = first.wait_for_ticket();
    drop(first);

    let mut second = resume_when_possible(&server, &device, ticket).expect("resumption");
    let renewed = second.wait_for_ticket();
    assert_ne!(renewed, ticket);
    let _screen = next_tile_or_resize(&mut second);
    drop(second);
    sleep(Duration::from_millis(600));
    assert!(refused_ticket(&server, &device, ticket));
    assert!(resume_when_possible(&server, &device, renewed).is_some());
}

#[test]
fn a_ticket_stops_working_after_the_grace_period() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut first, device) = pair_and_login(&server, "Aula 3");
    let ticket = first.wait_for_ticket();
    drop(first);
    sleep(Duration::from_millis(600));
    server.time.fetch_add(299, Ordering::SeqCst);
    assert!(resume_when_possible(&server, &device, ticket).is_some());
    sleep(Duration::from_millis(600));
    server.time.fetch_add(301, Ordering::SeqCst);
    assert!(refused_ticket(&server, &device, ticket));
}

#[test]
fn a_wrong_ticket_falls_back_to_the_second_factor_without_counting_as_a_failure() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut first, device) = pair_and_login(&server, "Aula 3");
    first.wait_for_ticket();
    drop(first);
    sleep(Duration::from_millis(600));
    for _ in 0..5 {
        assert!(refused_ticket(&server, &device, [7; 32]));
    }
    let mut client = start_reconnect(&server, &device, &server.agent_key()).unwrap();
    client.expect_auth_required();
    assert!(matches!(
        client.resume([9; 32]),
        Received::Got(ControlMessage::AuthRequired)
    ));
    client.expect_ok(&server.fresh_totp());
}

#[test]
fn a_ticket_cannot_be_presented_twice_on_one_connection_or_while_pairing() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut first, device) = pair_and_login(&server, "Aula 3");
    first.wait_for_ticket();
    drop(first);
    sleep(Duration::from_millis(600));
    let mut client = start_reconnect(&server, &device, &server.agent_key()).unwrap();
    client.expect_auth_required();
    assert!(matches!(
        client.resume([1; 32]),
        Received::Got(ControlMessage::AuthRequired)
    ));
    client.send_control(ControlMessage::Resume { ticket: [2; 32] });
    assert!(client.wait_closed(Duration::from_secs(5)));

    let new_device = StaticKey::generate().unwrap();
    let code = server.open_pairing();
    let mut pairing = start_pairing(&server, &new_device, &code, "Otro").unwrap();
    pairing.expect_auth_required();
    pairing.send_control(ControlMessage::Resume { ticket: [3; 32] });
    assert!(pairing.wait_closed(Duration::from_secs(5)));
}

#[test]
fn a_protocol_violation_revokes_the_ticket() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, device) = pair_and_login(&server, "Aula 3");
    let ticket = client.wait_for_ticket();
    client.send(&Frame::new(Channel::Terminal, 1, vec![99, 1, 2]).unwrap());
    assert!(client.wait_closed(Duration::from_secs(5)));
    sleep(Duration::from_millis(600));
    assert!(refused_ticket(&server, &device, ticket));
}

#[test]
fn an_idle_session_is_closed_with_a_notice_and_loses_its_ticket() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start_with_idle(&xvfb.display, Duration::from_secs(2));
    let (mut client, device) = pair_and_login(&server, "Aula 3");
    let ticket = client.wait_for_ticket();
    let reason = client.read_until_session_ends(Duration::from_secs(10));
    assert_eq!(reason, Some(EndReason::Idle));
    sleep(Duration::from_millis(600));
    assert!(refused_ticket(&server, &device, ticket));
}

#[test]
fn activity_keeps_a_session_alive_past_the_idle_limit() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start_with_idle(&xvfb.display, Duration::from_secs(2));
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(100));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut moved = 0_u16;
    while Instant::now() < deadline {
        moved = (moved + 1) % 200;
        client.send_viewer(ViewerMessage::Input(InputEvent::PointerMove {
            x: moved,
            y: 10,
        }));
        if let Received::Got(frame) = client.read_frame()
            && let Ok(DesktopMessage::FrameEnd { sequence }) =
                DesktopMessage::decode(&frame.payload)
        {
            client.send_viewer(ViewerMessage::FrameAck { sequence });
        }
        sleep(Duration::from_millis(100));
    }
    assert!(!client.wait_closed(Duration::from_millis(500)));
}

#[test]
fn a_revoked_viewer_is_told_why_it_was_cut_off() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, device) = pair_and_login(&server, "Aula 3");
    let key = device.public();
    server
        .store
        .update(|state, _| state.remove_device(&key))
        .unwrap()
        .unwrap();
    assert_eq!(
        client.read_until_session_ends(Duration::from_secs(6)),
        Some(EndReason::Revoked)
    );
}

#[test]
fn a_locked_agent_tells_the_viewer_before_it_closes() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    server
        .store
        .update(|state, _| state.set_locked(true))
        .unwrap();
    assert_eq!(
        client.read_until_session_ends(Duration::from_secs(6)),
        Some(EndReason::Locked)
    );
}

fn send_clipboard(client: &mut Client, stream: u32, message: &ClipboardMessage) {
    client.send(&Frame::new(Channel::Clipboard, stream, message.encode()).unwrap());
}

fn next_clipboard(client: &mut Client) -> (u32, ClipboardMessage) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match client.read_frame() {
            Received::Got(frame) if frame.channel == Channel::Clipboard => {
                return (
                    frame.stream,
                    ClipboardMessage::decode(&frame.payload).unwrap(),
                );
            }
            Received::Got(frame) => {
                if let Ok(DesktopMessage::FrameEnd { sequence }) =
                    DesktopMessage::decode(&frame.payload)
                {
                    client.send_viewer(ViewerMessage::FrameAck { sequence });
                }
            }
            Received::Timeout => {}
            Received::Closed => panic!("the connection closed"),
        }
    }
    panic!("no clipboard message arrived");
}

fn set_clipboard_mode(server: &Running, mode: ClipboardMode) {
    server
        .store
        .update(|state, _| state.set_clipboard_mode(mode))
        .unwrap();
}

#[test]
fn the_clipboard_is_refused_until_the_owner_allows_each_direction() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    let denied = (1, ClipboardMessage::Failed(ClipboardFailure::NotAllowed));

    send_clipboard(&mut client, 1, &ClipboardMessage::Write("x".to_owned()));
    assert_eq!(next_clipboard(&mut client), denied);
    send_clipboard(&mut client, 1, &ClipboardMessage::Read);
    assert_eq!(next_clipboard(&mut client), denied);

    set_clipboard_mode(&server, ClipboardMode::ToRemote);
    send_clipboard(&mut client, 1, &ClipboardMessage::Read);
    assert_eq!(next_clipboard(&mut client), denied);
    send_clipboard(
        &mut client,
        1,
        &ClipboardMessage::Write("permitido".to_owned()),
    );
    assert_eq!(next_clipboard(&mut client), (1, ClipboardMessage::Written));

    set_clipboard_mode(&server, ClipboardMode::FromRemote);
    send_clipboard(&mut client, 2, &ClipboardMessage::Write("no".to_owned()));
    assert_eq!(
        next_clipboard(&mut client),
        (2, ClipboardMessage::Failed(ClipboardFailure::NotAllowed))
    );
    send_clipboard(&mut client, 2, &ClipboardMessage::Read);
    assert_eq!(
        next_clipboard(&mut client),
        (2, ClipboardMessage::Content("permitido".to_owned()))
    );
}

#[test]
fn text_moves_between_the_viewer_and_a_desktop_application() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    set_clipboard_mode(&server, ClipboardMode::Both);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    let mut application = X11Clipboard::connect(Some(&xvfb.display)).unwrap();

    send_clipboard(&mut client, 1, &ClipboardMessage::Read);
    assert_eq!(next_clipboard(&mut client), (1, ClipboardMessage::Empty));

    send_clipboard(
        &mut client,
        2,
        &ClipboardMessage::Write("del visor, con ñ".to_owned()),
    );
    assert_eq!(next_clipboard(&mut client), (2, ClipboardMessage::Written));
    assert_eq!(
        application.read().unwrap().as_deref(),
        Some("del visor, con ñ")
    );

    application.write("copiado en casa").unwrap();
    send_clipboard(&mut client, 3, &ClipboardMessage::Read);
    assert_eq!(
        next_clipboard(&mut client),
        (3, ClipboardMessage::Content("copiado en casa".to_owned()))
    );
}

#[test]
fn text_sent_from_the_viewer_stays_after_the_session_ends() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    set_clipboard_mode(&server, ClipboardMode::ToRemote);
    let mut application = X11Clipboard::connect(Some(&xvfb.display)).unwrap();
    {
        let (mut client, _device) = pair_and_login(&server, "Aula 3");
        client.set_timeout(Duration::from_millis(300));
        send_clipboard(
            &mut client,
            1,
            &ClipboardMessage::Write("persistente".to_owned()),
        );
        assert_eq!(next_clipboard(&mut client), (1, ClipboardMessage::Written));
    }
    sleep(Duration::from_millis(500));
    assert_eq!(application.read().unwrap().as_deref(), Some("persistente"));
}

#[test]
fn clipboard_use_is_logged_without_the_text() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    set_clipboard_mode(&server, ClipboardMode::Both);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    send_clipboard(
        &mut client,
        1,
        &ClipboardMessage::Write("contraseña-secreta".to_owned()),
    );
    next_clipboard(&mut client);
    send_clipboard(&mut client, 2, &ClipboardMessage::Read);
    next_clipboard(&mut client);
    let log = fs::read_to_string(server.dir.join("state").join("access.log")).unwrap();
    assert!(log.contains("clipboard_write 19 bytes"), "{log}");
    assert!(log.contains("clipboard_read"), "{log}");
    assert!(!log.contains("contraseña-secreta"));
}

#[test]
fn a_malformed_or_backwards_clipboard_message_ends_the_session() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.send(&Frame::new(Channel::Clipboard, 1, vec![99]).unwrap());
    assert!(client.wait_closed(Duration::from_secs(5)));
    let (mut again, _device) = pair_and_login(&server, "Aula 4");
    send_clipboard(&mut again, 1, &ClipboardMessage::Written);
    assert!(again.wait_closed(Duration::from_secs(5)));
}

fn viewer_config(dir: &std::path::Path, state: PathBuf, hash: Option<String>) -> ServerConfig {
    ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        display: None,
        viewer_dir: dir.to_path_buf(),
        jpeg_quality: 70,
        state_dir: state,
        clock: None,
        vault: None,
        idle_timeout: Duration::from_secs(3600),
        viewer_hash: hash,
        exposure: wirehaus_agent::Exposure::default(),
    }
}

#[test]
fn the_agent_refuses_to_start_with_a_viewer_that_does_not_match_its_manifest() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let viewer = server.dir.join("viewer");
    let state = server.dir.join("state");
    fs::write(viewer.join("index.html"), b"<title>tampered</title>").unwrap();
    let result = Server::bind(viewer_config(&viewer, state.clone(), None));
    assert!(matches!(result, Err(ServerError::Viewer(_))), "{result:?}");
    fs::write(viewer.join("index.html"), b"<title>viewer</title>").unwrap();
    fs::write(viewer.join("extra.js"), b"x").unwrap();
    assert!(matches!(
        Server::bind(viewer_config(&viewer, state, None)),
        Err(ServerError::Viewer(_))
    ));
}

#[test]
fn a_pinned_viewer_hash_is_enforced_and_the_manifest_is_served() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let viewer = server.dir.join("viewer");
    let state = server.dir.join("state");
    let bundle = wirehaus_agent::static_files::write_manifest(&viewer).unwrap();
    assert!(Server::bind(viewer_config(&viewer, state.clone(), Some(bundle.clone()))).is_ok());
    let wrong = "f".repeat(64);
    assert!(matches!(
        Server::bind(viewer_config(&viewer, state, Some(wrong))),
        Err(ServerError::Viewer(_))
    ));
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);
    let reply = server.http(&format!("GET /manifest.sha256 HTTP/1.1\r\n{host}\r\n"));
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("text/plain"), "{reply}");
    assert!(reply.contains("  index.html\n"), "{reply}");
}

#[test]
fn the_viewer_is_served_from_what_was_verified_even_if_the_files_change_later() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    fs::write(
        server.dir.join("viewer").join("index.html"),
        b"<title>changed after start</title>",
    )
    .unwrap();
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);
    let reply = server.http(&format!("GET / HTTP/1.1\r\n{host}\r\n"));
    assert!(reply.ends_with("<title>viewer</title>"), "{reply}");
}

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Files => "files",
        Channel::Clipboard => "clipboard",
        Channel::Terminal => "terminal",
        Channel::Desktop => "desktop",
        _ => "other",
    }
}

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, limit: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(limit).unwrap()).unwrap()
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let length = 1 + self.below(max);
        (0..length)
            .map(|_| u8::try_from(self.next() & 0xFF).unwrap())
            .collect()
    }
}

const FUZZ_PATHS: [&str; 16] = [
    "",
    "docs",
    "docs/note.txt",
    "new",
    "new/inner",
    "up.bin",
    "..",
    "../private/secret.txt",
    "docs/../up.bin",
    "/etc/passwd",
    "a\\b",
    "nul\0byte",
    "escape",
    "escape/secret.txt",
    ".wirehaus-1.part",
    "x/y/z/w",
];

fn random_files_message(dice: &mut Dice) -> FilesMessage {
    let root = u8::try_from(dice.below(3)).unwrap();
    let path = dice.pick(&FUZZ_PATHS).to_owned();
    match dice.below(10) {
        0 => FilesMessage::ListRoots,
        1 => FilesMessage::List { root, path },
        2 => FilesMessage::Download { root, path },
        3 => FilesMessage::Upload {
            root,
            path,
            size: [0, 1, 70_000, 5_000_000, u64::MAX][dice.below(5)],
        },
        4 => FilesMessage::Mkdir { root, path },
        5 => FilesMessage::Remove { root, path },
        6 => FilesMessage::Cancel,
        7 => FilesMessage::Data(dice.bytes(wirehaus_protocol::MAX_CHUNK)),
        8 => FilesMessage::Done,
        _ => FilesMessage::Ack {
            bytes: [0, 1, 65_536, 9_999_999, u64::MAX][dice.below(5)],
        },
    }
}

fn random_clipboard_message(dice: &mut Dice) -> ClipboardMessage {
    if dice.below(3) == 0 {
        return ClipboardMessage::Read;
    }
    ClipboardMessage::Write(
        ["a", "ñandú", "línea\ncon\tcontrol", &"x".repeat(30_000)][dice.below(4)].to_owned(),
    )
}

fn random_viewer_message(dice: &mut Dice) -> ViewerMessage {
    match dice.below(5) {
        0 => ViewerMessage::FrameAck {
            sequence: u32::try_from(dice.next() & 0xFFFF_FFFF).unwrap(),
        },
        1 => ViewerMessage::Capabilities {
            video: dice.below(2) == 0,
        },
        2 => ViewerMessage::Input(InputEvent::Key {
            code: u16::try_from(dice.next() & 0xFFFF).unwrap(),
            pressed: dice.below(2) == 0,
        }),
        3 => ViewerMessage::Input(InputEvent::Wheel {
            horizontal: i16::try_from(dice.next() & 0x7FFF).unwrap() - 16_000,
            vertical: i16::try_from(dice.next() & 0x7FFF).unwrap() - 16_000,
        }),
        _ => ViewerMessage::Input(InputEvent::PointerMove {
            x: u16::try_from(dice.next() & 0x1FF).unwrap(),
            y: u16::try_from(dice.next() & 0x1FF).unwrap(),
        }),
    }
}

fn random_terminal_message(dice: &mut Dice) -> TerminalMessage {
    match dice.below(4) {
        0 => TerminalMessage::Open {
            columns: u16::try_from(1 + dice.below(500)).unwrap(),
            rows: u16::try_from(1 + dice.below(500)).unwrap(),
        },
        1 => TerminalMessage::Input(dice.bytes(wirehaus_protocol::MAX_INPUT_CHUNK)),
        2 => TerminalMessage::Resize {
            columns: u16::try_from(1 + dice.below(500)).unwrap(),
            rows: u16::try_from(1 + dice.below(500)).unwrap(),
        },
        _ => TerminalMessage::Close,
    }
}

fn random_frame(dice: &mut Dice) -> Frame {
    let stream = u32::try_from(dice.below(8)).unwrap();
    let (channel, payload) = match dice.below(4) {
        0 => (Channel::Files, random_files_message(dice).encode()),
        1 => (Channel::Clipboard, random_clipboard_message(dice).encode()),
        2 => (Channel::Desktop, random_viewer_message(dice).encode()),
        _ => (Channel::Terminal, random_terminal_message(dice).encode()),
    };
    Frame::new(channel, stream, payload).unwrap()
}

#[test]
fn an_authenticated_viewer_cannot_crash_or_wedge_the_agent_with_valid_looking_messages() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let folder = SharedFolder::new();
    share(&server, &folder);
    set_clipboard_mode(&server, ClipboardMode::Both);
    std::os::unix::fs::symlink(folder.base.join("private"), folder.shared().join("escape"))
        .unwrap();
    let (mut client, device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(5));
    let seed = std::env::var("WIREHAUS_FUZZ_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0x00FF_1CE0_DEAD_BEEF_u64);
    let rounds: usize = std::env::var("WIREHAUS_FUZZ_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4000);
    let mut dice = Dice(seed);
    let mut replies = std::collections::HashMap::new();
    for round in 0..rounds {
        client.send(&random_frame(&mut dice));
        if round % 7 != 0 {
            continue;
        }
        loop {
            match client.read_frame() {
                Received::Got(frame) => {
                    if let Ok(DesktopMessage::FrameEnd { sequence }) =
                        DesktopMessage::decode(&frame.payload)
                    {
                        client.send_viewer(ViewerMessage::FrameAck { sequence });
                    }
                    *replies
                        .entry(channel_name(frame.channel))
                        .or_insert(0_usize) += 1;
                }
                Received::Timeout => break,
                Received::Closed => panic!("the agent ended the session at round {round}"),
            }
        }
    }
    assert!(
        replies.get("files").copied().unwrap_or(0) > 100,
        "{replies:?}"
    );
    assert!(
        replies.get("clipboard").copied().unwrap_or(0) > 10,
        "{replies:?}"
    );
    assert!(
        replies.get("terminal").copied().unwrap_or(0) > 10,
        "{replies:?}"
    );
    assert!(!folder.base.join("private/planted.txt").exists());
    assert_eq!(
        fs::read(folder.base.join("private/secret.txt")).unwrap(),
        b"secret"
    );
    drop(client);
    sleep(Duration::from_millis(600));
    let mut again = reconnect_and_login(&server, &device);
    again.set_timeout(Duration::from_secs(5));
    assert!(matches!(
        next_tile_or_resize(&mut again),
        DesktopMessage::Resize { .. } | DesktopMessage::Tile { .. }
    ));
}

fn websocket_status(server: &Running, host: &str, origin: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request = format!(
        "GET /ws HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut buffer = [0_u8; 64];
    let read = stream.read(&mut buffer).unwrap_or(0);
    let _ = stream.shutdown(Shutdown::Both);
    String::from_utf8_lossy(&buffer[..read])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_configured_public_host_and_viewer_origin_are_accepted_and_nothing_else() {
    let xvfb = Xvfb::start_unique();
    let exposure = wirehaus_agent::Exposure::new(
        &["agent.example.com".to_owned()],
        &["https://app.example.com".to_owned()],
    )
    .unwrap();
    let server = Running::start_configured(&xvfb.display, Duration::from_secs(3600), exposure);

    let reply = server.http("GET / HTTP/1.1\r\nHost: agent.example.com\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    let reply = server.http("GET / HTTP/1.1\r\nHost: evil.example.com\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");

    let local = format!("127.0.0.1:{}", server.port);
    for (host, origin, accepted) in [
        ("agent.example.com", "https://agent.example.com", true),
        ("agent.example.com", "https://app.example.com", true),
        ("agent.example.com", "https://evil.example.com", false),
        ("agent.example.com", "http://agent.example.com", false),
        ("agent.example.com", "null", false),
        ("evil.example.com", "https://agent.example.com", false),
        (local.as_str(), "https://app.example.com", true),
        (local.as_str(), "https://evil.example.com", false),
    ] {
        let status = websocket_status(&server, host, origin);
        let expected = if accepted {
            "HTTP/1.1 101"
        } else {
            "HTTP/1.1 403"
        };
        assert!(status.starts_with(expected), "{host} {origin}: {status}");
    }
}

#[test]
fn without_configuration_a_public_name_is_refused() {
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let reply = server.http("GET / HTTP/1.1\r\nHost: agent.example.com\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    let status = websocket_status(&server, "agent.example.com", "https://agent.example.com");
    assert!(status.starts_with("HTTP/1.1 403"), "{status}");
}

#[test]
fn rejects_foreign_origins_and_hosts() {
    let xvfb = Xvfb::start_unique();
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
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);

    let reply = server.http(&format!("GET / HTTP/1.1\r\n{host}\r\n"));
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("Content-Security-Policy: default-src 'none'"));
    assert!(reply.contains("Cross-Origin-Resource-Policy: same-origin"));
    assert!(reply.contains("Permissions-Policy: camera=(), microphone=()"));
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
        vault: None,
        idle_timeout: Duration::from_secs(3600),
        viewer_hash: None,
        exposure: wirehaus_agent::Exposure::default(),
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
        vault: None,
        idle_timeout: Duration::from_secs(3600),
        viewer_hash: None,
        exposure: wirehaus_agent::Exposure::default(),
    });
    assert!(matches!(result, Err(ServerError::Store(_))));
}

fn pulse_available() -> bool {
    std::process::Command::new("pactl")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn send_audio(client: &mut Client, message: &AudioMessage) {
    client.send(&Frame::new(Channel::Audio, 0, message.encode()).unwrap());
}

fn next_audio(client: &mut Client) -> AudioMessage {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match client.read_frame() {
            Received::Got(frame) if frame.channel == Channel::Audio => {
                return AudioMessage::decode(&frame.payload).unwrap();
            }
            Received::Got(frame) => {
                if let Ok(DesktopMessage::FrameEnd { sequence }) =
                    DesktopMessage::decode(&frame.payload)
                {
                    client.send_viewer(ViewerMessage::FrameAck { sequence });
                }
            }
            Received::Timeout => {}
            Received::Closed => panic!("the connection closed"),
        }
    }
    panic!("no audio message arrived");
}

fn audio_packets(client: &mut Client, wanted: usize) -> usize {
    let mut seen = 0;
    while seen < wanted {
        match next_audio(client) {
            AudioMessage::Packet(data) => {
                assert!(!data.is_empty() && data.len() <= wirehaus_protocol::MAX_AUDIO_PACKET);
                seen += 1;
            }
            other => panic!("expected a packet, got {other:?}"),
        }
    }
    seen
}

#[test]
fn audio_is_refused_until_the_owner_allows_it_and_then_streams_packets() {
    if !pulse_available() {
        eprintln!("skipped: no PulseAudio or PipeWire server");
        return;
    }
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));

    send_audio(&mut client, &AudioMessage::Start);
    assert_eq!(
        next_audio(&mut client),
        AudioMessage::Failed(AudioFailure::NotAllowed)
    );

    server
        .store
        .update(|state, _| state.set_audio_allowed(true))
        .unwrap();
    send_audio(&mut client, &AudioMessage::Start);
    assert_eq!(next_audio(&mut client), AudioMessage::Started);
    assert_eq!(audio_packets(&mut client, 10), 10);

    send_audio(&mut client, &AudioMessage::Stop);
    loop {
        match next_audio(&mut client) {
            AudioMessage::Packet(_) => {}
            AudioMessage::Stopped => break,
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn listening_does_not_keep_an_idle_session_alive() {
    if !pulse_available() {
        eprintln!("skipped: no PulseAudio or PipeWire server");
        return;
    }
    let xvfb = Xvfb::start_unique();
    let server = Running::start_with_idle(&xvfb.display, Duration::from_secs(2));
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    server
        .store
        .update(|state, _| state.set_audio_allowed(true))
        .unwrap();
    send_audio(&mut client, &AudioMessage::Start);
    assert_eq!(next_audio(&mut client), AudioMessage::Started);
    let reason = client.read_until_session_ends(Duration::from_secs(10));
    assert_eq!(reason, Some(EndReason::Idle));
}

#[test]
fn removing_the_audio_permission_stops_the_stream_that_is_running() {
    if !pulse_available() {
        eprintln!("skipped: no PulseAudio or PipeWire server");
        return;
    }
    let xvfb = Xvfb::start_unique();
    let server = Running::start(&xvfb.display);
    let (mut client, _device) = pair_and_login(&server, "Aula 3");
    client.set_timeout(Duration::from_millis(300));
    server
        .store
        .update(|state, _| state.set_audio_allowed(true))
        .unwrap();
    send_audio(&mut client, &AudioMessage::Start);
    assert_eq!(next_audio(&mut client), AudioMessage::Started);
    assert_eq!(audio_packets(&mut client, 5), 5);

    server
        .store
        .update(|state, _| state.set_audio_allowed(false))
        .unwrap();
    loop {
        match next_audio(&mut client) {
            AudioMessage::Packet(_) => {}
            AudioMessage::Stopped => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    send_audio(&mut client, &AudioMessage::Start);
    assert_eq!(
        next_audio(&mut client),
        AudioMessage::Failed(AudioFailure::NotAllowed)
    );
}
