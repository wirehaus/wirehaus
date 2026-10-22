use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tungstenite::protocol::WebSocketConfig;

use crate::auth::{self, INBOUND_LIMIT, Outcome};
use crate::error::ServerError;
use crate::http::{
    HeadError, MAX_HEAD_LEN, RequestHead, host_allowed, origin_allowed, parse_head, response,
};
use crate::pacer::Pacer;
use crate::platform;
use crate::session;
use crate::static_files;
use crate::store::{Clock, Store, system_clock};
use crate::stream::Stream;
use wirehaus_protocol::{AuthFailure, ControlMessage};

const ACCEPT_POLL: Duration = Duration::from_millis(20);
const HEAD_DEADLINE: Duration = Duration::from_secs(5);
const HEAD_POLL: Duration = Duration::from_millis(5);
const MAX_INBOUND_MESSAGE: usize = INBOUND_LIMIT + 64;
const MAX_CONNECTIONS: usize = 64;
const MAX_UNAUTHENTICATED: usize = 16;
const HEAD_END: &[u8] = b"\r\n\r\n";
const TEXT: &str = "text/plain; charset=utf-8";

#[derive(Clone)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub display: Option<String>,
    pub viewer_dir: PathBuf,
    pub jpeg_quality: u8,
    pub state_dir: PathBuf,
    pub clock: Option<Clock>,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("listen", &self.listen)
            .field("display", &self.display)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("listener", &self.listener)
            .finish_non_exhaustive()
    }
}

pub struct Server {
    listener: TcpListener,
    config: ServerConfig,
    store: Arc<Store>,
    connections: AtomicUsize,
    unauthenticated: AtomicUsize,
}

struct CountGuard<'a>(&'a AtomicUsize);

impl<'a> CountGuard<'a> {
    fn enter(counter: &'a AtomicUsize, limit: usize) -> Option<Self> {
        if counter.fetch_add(1, Ordering::SeqCst) >= limit {
            counter.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self(counter))
    }
}

impl Drop for CountGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Server {
    pub fn bind(config: ServerConfig) -> Result<Self, ServerError> {
        if !config.listen.ip().is_loopback() {
            return Err(ServerError::NotLoopback(config.listen));
        }
        let listener = TcpListener::bind(config.listen).map_err(|source| ServerError::Bind {
            addr: config.listen,
            source,
        })?;
        listener.set_nonblocking(true)?;
        let clock = config.clock.clone().unwrap_or_else(system_clock);
        let store = Arc::new(Store::open(&config.state_dir, clock)?);
        Ok(Self {
            listener,
            config,
            store,
            connections: AtomicUsize::new(0),
            unauthenticated: AtomicUsize::new(0),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, ServerError> {
        Ok(self.listener.local_addr()?)
    }

    pub fn run(&self, stop: &AtomicBool) -> Result<(), ServerError> {
        let busy = AtomicBool::new(false);
        let port = self.local_addr()?.port();
        thread::scope(|scope| {
            while !stop.load(Ordering::Relaxed) {
                match self.listener.accept() {
                    Ok((stream, peer)) if peer.ip().is_loopback() => {
                        let busy = &busy;
                        let Some(guard) = CountGuard::enter(&self.connections, MAX_CONNECTIONS)
                        else {
                            continue;
                        };
                        scope.spawn(move || {
                            let _guard = guard;
                            if let Err(error) = self.serve_connection(stream, port, busy, stop) {
                                eprintln!("connection ended: {error}");
                            }
                        });
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
                    Err(e) => return Err(ServerError::from(e)),
                }
            }
            Ok(())
        })
    }

    fn serve_connection(
        &self,
        mut stream: TcpStream,
        port: u16,
        busy: &AtomicBool,
        stop: &AtomicBool,
    ) -> Result<(), ServerError> {
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        let Some((head, head_len)) = read_head(&mut stream)? else {
            return Ok(());
        };
        if !host_allowed(head.host.as_deref(), port) {
            return reply(
                &mut stream,
                head_len,
                false,
                "403 Forbidden",
                b"forbidden host",
            );
        }
        if head.wants_websocket {
            self.upgrade(stream, &head, port, busy, stop)
        } else {
            self.serve_file(&mut stream, &head, head_len)
        }
    }

    fn serve_file(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        head_len: usize,
    ) -> Result<(), ServerError> {
        if head.method != "GET" {
            return reply(
                stream,
                head_len,
                true,
                "405 Method Not Allowed",
                b"method not allowed",
            );
        }
        match static_files::load(&self.config.viewer_dir, &head.path) {
            Some(file) => {
                consume(stream, head_len)?;
                finish(stream, &response("200 OK", file.content_type, &file.body))
            }
            None => reply(stream, head_len, true, "404 Not Found", b"not found"),
        }
    }

    fn upgrade(
        &self,
        mut stream: TcpStream,
        head: &RequestHead,
        port: u16,
        busy: &AtomicBool,
        stop: &AtomicBool,
    ) -> Result<(), ServerError> {
        if head.path != "/ws" {
            return reply(&mut stream, 0, false, "404 Not Found", b"not found");
        }
        if !origin_allowed(head.origin.as_deref(), port) {
            return reply(&mut stream, 0, false, "403 Forbidden", b"forbidden origin");
        }
        let Some(pending) = CountGuard::enter(&self.unauthenticated, MAX_UNAUTHENTICATED) else {
            return reply(
                &mut stream,
                0,
                false,
                "503 Service Unavailable",
                b"too many connections in progress",
            );
        };
        stream.set_read_timeout(Some(HEAD_DEADLINE))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_INBOUND_MESSAGE))
            .max_frame_size(Some(MAX_INBOUND_MESSAGE));
        let socket = tungstenite::accept_with_config(stream, Some(config))
            .map_err(|e| ServerError::WebSocket(e.to_string()))?;
        let outcome = auth::negotiate(socket, &self.store)?;
        drop(pending);
        match outcome {
            Outcome::Refused => Ok(()),
            Outcome::Authenticated(link, identity) => {
                self.serve_authenticated(*link, &identity, busy, stop)
            }
        }
    }

    fn serve_authenticated(
        &self,
        mut link: crate::link::Link,
        identity: &auth::Identity,
        busy: &AtomicBool,
        stop: &AtomicBool,
    ) -> Result<(), ServerError> {
        let name = identity.name.as_str();
        if busy.swap(true, Ordering::SeqCst) {
            self.store.log("busy", name);
            let _ = link.send_control(ControlMessage::AuthFailed {
                reason: AuthFailure::Busy,
                retry_after: 0,
            });
            let _ = link.flush();
            link.close();
            return Ok(());
        }
        let _guard = BusyGuard(busy);
        let backend = match platform::connect(self.config.display.as_deref()) {
            Ok(backend) => backend,
            Err(error) => {
                self.store.log("no_display", &error.to_string());
                link.close();
                return Err(error);
            }
        };
        link.send_control(ControlMessage::AuthOk)?;
        link.flush()?;
        self.store.log("session_start", name);
        let result = session::run(
            link,
            backend,
            Stream::new(self.config.jpeg_quality),
            Pacer::new(self.config.jpeg_quality),
            stop,
            &|| {
                self.store
                    .read(|state, _| state.is_locked() || state.device(&identity.device).is_none())
                    .unwrap_or(true)
            },
        );
        self.store.log("session_end", name);
        result
    }
}

fn read_head(stream: &mut TcpStream) -> Result<Option<(RequestHead, usize)>, ServerError> {
    let deadline = Instant::now() + HEAD_DEADLINE;
    stream.set_read_timeout(Some(HEAD_POLL))?;
    let mut buffer = [0_u8; MAX_HEAD_LEN];
    while Instant::now() < deadline {
        let seen = match stream.peek(&mut buffer) {
            Ok(0) => return Ok(None),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e.into()),
        };
        if let Some(end) = find(&buffer[..seen], HEAD_END) {
            let head_len = end + HEAD_END.len();
            return match parse_head(&buffer[..head_len]) {
                Ok(Some(head)) => Ok(Some((head, head_len))),
                Ok(None) | Err(HeadError::Malformed | HeadError::DuplicateHost) => {
                    reply(stream, 0, false, "400 Bad Request", b"bad request")?;
                    Ok(None)
                }
            };
        }
        if seen == buffer.len() {
            reply(
                stream,
                0,
                false,
                "431 Request Header Fields Too Large",
                b"too large",
            )?;
            return Ok(None);
        }
        thread::sleep(HEAD_POLL);
    }
    reply(stream, 0, false, "408 Request Timeout", b"timeout")?;
    Ok(None)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn consume(stream: &mut TcpStream, len: usize) -> Result<(), ServerError> {
    let mut sink = vec![0_u8; len];
    stream.read_exact(&mut sink)?;
    Ok(())
}

fn reply(
    stream: &mut TcpStream,
    head_len: usize,
    consume_head: bool,
    status: &str,
    body: &[u8],
) -> Result<(), ServerError> {
    if consume_head {
        consume(stream, head_len)?;
    }
    finish(stream, &response(status, TEXT, body))
}

fn finish(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), ServerError> {
    stream.write_all(bytes)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}
