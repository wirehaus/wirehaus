use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use tungstenite::protocol::WebSocketConfig;

use crate::auth::{self, INBOUND_LIMIT, Outcome};
use crate::clipboard::Hub;
use crate::error::ServerError;
use crate::http::{
    Exposure, HeadError, MAX_HEAD_LEN, RequestHead, ip_key, json_response, parse_head,
    real_client_ip, response,
};
use crate::linkbox::{Links, Poll, StartError, from_hex, to_hex};
use crate::pacer::Pacer;
use crate::platform;
use crate::session::{self, Policy};
use crate::state::{AuthError, State};
use crate::static_files::Viewer;
use crate::store::{Clock, Store, VaultSource, system_clock, system_vault};
use crate::stream::Stream;
use wirehaus_crypto::keys::PublicKey;
use wirehaus_crypto::ticket::Tickets;
use wirehaus_protocol::{AuthFailure, ControlMessage, EndReason, LinkFailure, TICKET_LEN};

const ACCEPT_POLL: Duration = Duration::from_millis(20);
const ACCEPT_RETRY: Duration = Duration::from_millis(100);
const MAX_ACCEPT_FAILURES: u32 = 50;
const HEAD_DEADLINE: Duration = Duration::from_secs(5);
const HEAD_POLL: Duration = Duration::from_millis(5);
const MAX_INBOUND_MESSAGE: usize = INBOUND_LIMIT + 64;
const MAX_CONNECTIONS: usize = 64;
const MAX_UNAUTHENTICATED: usize = 16;
const MAX_UNAUTHENTICATED_PER_IP: usize = 4;
const MAX_SESSIONS: usize = 5;
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
    pub vault: Option<VaultSource>,
    pub idle_timeout: Duration,
    pub viewer_hash: Option<String>,
    pub exposure: Exposure,
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
    clipboard: Arc<Hub>,
    viewer: Viewer,
    tickets: Mutex<Tickets>,
    links: Mutex<Links>,
    sessions: AtomicUsize,
    last_used: Mutex<Instant>,
    connections: AtomicUsize,
    unauthenticated: AtomicUsize,
    unauthenticated_by_ip: IpLimiter,
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

#[derive(Debug, Default)]
struct IpLimiter {
    counts: Mutex<HashMap<IpAddr, usize>>,
}

struct IpGuard<'a> {
    limiter: &'a IpLimiter,
    key: IpAddr,
}

impl IpLimiter {
    fn enter(&self, ip: IpAddr, limit: usize) -> Option<IpGuard<'_>> {
        let key = ip_key(ip);
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(key).or_insert(0);
        if *count >= limit {
            return None;
        }
        *count += 1;
        Some(IpGuard { limiter: self, key })
    }
}

impl Drop for IpGuard<'_> {
    fn drop(&mut self) {
        let mut counts = self
            .limiter
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = counts.get_mut(&self.key) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.key);
            }
        }
    }
}

#[derive(Debug, Default)]
struct AcceptFailures(u32);

impl AcceptFailures {
    fn record(&mut self) -> bool {
        self.0 += 1;
        self.0 < MAX_ACCEPT_FAILURES
    }

    fn reset(&mut self) {
        self.0 = 0;
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
        let vault = config.vault.clone().unwrap_or_else(system_vault);
        let store = Arc::new(Store::open_with(&config.state_dir, clock, vault)?);
        let viewer = Viewer::open(&config.viewer_dir, config.viewer_hash.as_deref())?;
        let clipboard = Arc::new(Hub::new(platform::clipboard_factory(
            config.display.clone(),
        )));
        Ok(Self {
            listener,
            config,
            store,
            clipboard,
            viewer,
            tickets: Mutex::new(Tickets::new()),
            links: Mutex::new(Links::default()),
            sessions: AtomicUsize::new(0),
            last_used: Mutex::new(Instant::now()),
            connections: AtomicUsize::new(0),
            unauthenticated: AtomicUsize::new(0),
            unauthenticated_by_ip: IpLimiter::default(),
        })
    }

    pub fn is_busy(&self) -> bool {
        self.sessions.load(Ordering::SeqCst) > 0
    }

    pub fn idle_for(&self) -> Duration {
        let mut last = self
            .last_used
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.is_busy() {
            *last = Instant::now();
        }
        last.elapsed()
    }

    fn touch(&self) {
        *self
            .last_used
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Instant::now();
    }

    pub fn local_addr(&self) -> Result<SocketAddr, ServerError> {
        Ok(self.listener.local_addr()?)
    }

    pub fn run(&self, stop: &AtomicBool) -> Result<(), ServerError> {
        let port = self.local_addr()?.port();
        let mut failures = AcceptFailures::default();
        thread::scope(|scope| {
            while !stop.load(Ordering::Relaxed) {
                match self.listener.accept() {
                    Ok((stream, peer)) if peer.ip().is_loopback() => {
                        failures.reset();
                        let Some(guard) = CountGuard::enter(&self.connections, MAX_CONNECTIONS)
                        else {
                            continue;
                        };
                        scope.spawn(move || {
                            let _guard = guard;
                            if let Err(error) = self.serve_connection(stream, peer, port, stop) {
                                eprintln!("connection ended: {error}");
                            }
                        });
                    }
                    Ok(_) => failures.reset(),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        failures.reset();
                        thread::sleep(ACCEPT_POLL);
                    }
                    Err(e) => {
                        if !failures.record() {
                            return Err(ServerError::from(e));
                        }
                        eprintln!("accept failed: {e}");
                        thread::sleep(ACCEPT_RETRY);
                    }
                }
            }
            Ok(())
        })
    }

    fn serve_connection(
        &self,
        mut stream: TcpStream,
        peer: SocketAddr,
        port: u16,
        stop: &AtomicBool,
    ) -> Result<(), ServerError> {
        self.touch();
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        let Some((head, head_len)) = read_head(&mut stream)? else {
            return Ok(());
        };
        if !self
            .config
            .exposure
            .host_allowed(head.host.as_deref(), port)
        {
            return reply(
                &mut stream,
                head_len,
                false,
                "403 Forbidden",
                b"forbidden host",
            );
        }
        let client = real_client_ip(peer.ip(), head.client_ip.as_deref());
        if head.wants_websocket {
            self.upgrade(stream, &head, client, port, stop)
        } else {
            self.serve_file(&mut stream, &head, head_len, client)
        }
    }

    fn serve_file(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        head_len: usize,
        client: IpAddr,
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
        if head.path == "/link/ping" || head.path.starts_with("/link/") {
            return self.serve_link(stream, head, head_len, client);
        }
        match self.viewer.get(&head.path) {
            Some(file) => {
                consume(stream, head_len)?;
                finish(stream, &response("200 OK", file.content_type, &file.body))
            }
            None => reply(stream, head_len, true, "404 Not Found", b"not found"),
        }
    }

    fn serve_link(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        head_len: usize,
        client: IpAddr,
    ) -> Result<(), ServerError> {
        consume(stream, head_len)?;
        let port = self.local_addr()?.port();
        let origin = head.origin.as_deref();
        if origin.is_some() && !self.config.exposure.origin_allowed(origin, port) {
            return finish(
                stream,
                &json_response("403 Forbidden", r#"{"error":"origin"}"#, None),
            );
        }
        let (status, body) = self.link_answer(&head.path, client);
        finish(stream, &json_response(status, &body, origin))
    }

    fn link_answer(&self, path: &str, client: IpAddr) -> (&'static str, String) {
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let param = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        };
        let now = self.store.read(|_, now| now).unwrap_or(0);
        let mut links = self.links.lock().unwrap_or_else(PoisonError::into_inner);
        match route {
            "/link/ping" => ("200 OK", r#"{"link":1}"#.to_owned()),
            "/link/start" => {
                let Some(public) = param("k").and_then(from_hex) else {
                    return ("400 Bad Request", r#"{"error":"key"}"#.to_owned());
                };
                match links.start(&public, now, client) {
                    Ok(started) => {
                        self.store
                            .log("link_started", "a computer asked to be linked");
                        (
                            "200 OK",
                            format!(
                                r#"{{"id":"{}","manual":"{}","verify":"{}","ttl":{}}}"#,
                                to_hex(&started.id),
                                started.manual,
                                started.verification,
                                crate::linkbox::TTL_SECS
                            ),
                        )
                    }
                    Err(StartError::Busy) => {
                        ("429 Too Many Requests", r#"{"error":"busy"}"#.to_owned())
                    }
                    Err(StartError::TooSoon) => {
                        ("429 Too Many Requests", r#"{"error":"slow"}"#.to_owned())
                    }
                    Err(StartError::Random) => ("400 Bad Request", r#"{"error":"key"}"#.to_owned()),
                }
            }
            "/link/poll" => {
                let id: Option<[u8; 16]> = param("i")
                    .and_then(from_hex)
                    .and_then(|bytes| bytes.try_into().ok());
                let Some(id) = id else {
                    return ("400 Bad Request", r#"{"error":"id"}"#.to_owned());
                };
                match links.poll(&id, now) {
                    Poll::Pending => ("200 OK", r#"{"state":"pending"}"#.to_owned()),
                    Poll::Ready(sealed) => (
                        "200 OK",
                        format!(r#"{{"state":"ready","sealed":"{}"}}"#, to_hex(&sealed)),
                    ),
                    Poll::Gone => ("200 OK", r#"{"state":"gone"}"#.to_owned()),
                }
            }
            _ => ("404 Not Found", r#"{"error":"not found"}"#.to_owned()),
        }
    }

    fn link_lookup(&self, key: &str) -> Option<([u8; 16], String)> {
        let now = self.store.read(|_, now| now).ok()?;
        self.links
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .find(key, now)
    }

    fn link_approve(
        &self,
        device: &PublicKey,
        id: &[u8; 16],
        code: &str,
    ) -> Result<(), LinkFailure> {
        let now = self
            .store
            .read(|_, now| now)
            .map_err(|_| LinkFailure::Unavailable)?;
        let mut links = self.links.lock().unwrap_or_else(PoisonError::into_inner);
        let public = links.public_of(id, now).ok_or(LinkFailure::Gone)?;
        let verdict = self
            .store
            .update(|state, now| state.verify_totp(Some(device), code, now))
            .map_err(|_| LinkFailure::Unavailable)?;
        match verdict {
            Ok(()) => {}
            Err(AuthError::WrongCode) => return Err(LinkFailure::WrongCode),
            Err(AuthError::Denied(_)) => return Err(LinkFailure::TooManyAttempts),
        }
        let already_open = self
            .store
            .read(|state, now| state.pairing_psk(now).is_some())
            .map_err(|_| LinkFailure::Unavailable)?;
        if already_open {
            return Err(LinkFailure::Unavailable);
        }
        let pairing = self
            .store
            .update(State::open_pairing)
            .map_err(|_| LinkFailure::Unavailable)?
            .map_err(|_| LinkFailure::Unavailable)?;
        let sealed = wirehaus_crypto::link::seal(&public, pairing.to_text().as_bytes())
            .map_err(|_| LinkFailure::Unavailable)?;
        if links.approve(id, sealed, now) {
            self.store
                .log("link_approved", "a paired device authorised a new computer");
            Ok(())
        } else {
            Err(LinkFailure::Gone)
        }
    }

    fn upgrade(
        &self,
        mut stream: TcpStream,
        head: &RequestHead,
        client: IpAddr,
        port: u16,
        stop: &AtomicBool,
    ) -> Result<(), ServerError> {
        if head.path != "/ws" {
            return reply(&mut stream, 0, false, "404 Not Found", b"not found");
        }
        if !self
            .config
            .exposure
            .origin_allowed(head.origin.as_deref(), port)
        {
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
        let Some(pending_from_ip) = self
            .unauthenticated_by_ip
            .enter(client, MAX_UNAUTHENTICATED_PER_IP)
        else {
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
        let outcome = auth::negotiate(socket, &self.store, &self.tickets)?;
        drop(pending);
        drop(pending_from_ip);
        match outcome {
            Outcome::Refused => Ok(()),
            Outcome::Authenticated(link, identity) => {
                self.serve_authenticated(*link, &identity, stop)
            }
        }
    }

    fn serve_authenticated(
        &self,
        mut link: crate::link::Link,
        identity: &auth::Identity,
        stop: &AtomicBool,
    ) -> Result<(), ServerError> {
        let name = identity.name.as_str();
        let Some(_slot) = CountGuard::enter(&self.sessions, MAX_SESSIONS) else {
            self.store.log("busy", name);
            let _ = link.send_control(ControlMessage::AuthFailed {
                reason: AuthFailure::Busy,
                retry_after: 0,
            });
            let _ = link.flush();
            link.close();
            return Ok(());
        };
        let backend = match platform::connect(self.config.display.as_deref()) {
            Ok(backend) => backend,
            Err(error) => {
                self.store.log("no_display", &error.to_string());
                let _ = link.send_control(ControlMessage::AuthFailed {
                    reason: AuthFailure::Unavailable,
                    retry_after: 0,
                });
                let _ = link.flush();
                link.close();
                return Err(error);
            }
        };
        let (ticket, superseded) = self.issue_ticket(&identity.device)?;
        link.send_control(ControlMessage::AuthOk)?;
        link.send_control(ControlMessage::Ticket { ticket })?;
        link.flush()?;
        self.store.log("session_start", name);
        let result = session::run(
            link,
            backend,
            Stream::new(self.config.jpeg_quality),
            Pacer::new(self.config.jpeg_quality),
            stop,
            &self.clipboard,
            &Policy {
                revoked: &|| {
                    self.store
                        .read(|state, _| {
                            if state.is_locked() {
                                Some(EndReason::Locked)
                            } else if state.device(&identity.device).is_none() {
                                Some(EndReason::Revoked)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(Some(EndReason::Revoked))
                },
                idle_timeout: self.config.idle_timeout,
                superseded: &|| superseded.load(Ordering::SeqCst),
                link_lookup: &|key| self.link_lookup(key),
                link_approve: &|id, code| self.link_approve(&identity.device, id, code),
                terminal_allowed: &|| {
                    self.store
                        .read(|state, _| state.terminal_allowed())
                        .unwrap_or(false)
                },
                audio_allowed: &|| {
                    self.store
                        .read(|state, _| state.audio_allowed())
                        .unwrap_or(false)
                },
                clipboard_mode: &|| {
                    self.store
                        .read(|state, _| state.clipboard_mode())
                        .unwrap_or_default()
                },
                audit: &|event, detail| self.store.log(event, detail),
                file_roots: &|| {
                    self.store
                        .read(|state, _| state.file_roots().to_vec())
                        .unwrap_or_default()
                },
            },
        );
        match &result {
            Ok(()) => self.store.log("session_end", name),
            Err(error) => self.store.log("session_end", &format!("{name}: {error}")),
        }
        self.settle_ticket(&identity.device, &ticket, &result);
        result
    }

    fn issue_ticket(
        &self,
        device: &PublicKey,
    ) -> Result<([u8; TICKET_LEN], Arc<AtomicBool>), ServerError> {
        let now = self.store.read(|_, now| now)?;
        let mut tickets = self.tickets.lock().unwrap_or_else(PoisonError::into_inner);
        let ticket = tickets.issue(*device, now)?;
        let superseded = tickets.superseded_flag(device, &ticket).unwrap_or_default();
        Ok((ticket, superseded))
    }

    fn settle_ticket(
        &self,
        device: &PublicKey,
        ticket: &[u8; TICKET_LEN],
        result: &Result<(), ServerError>,
    ) {
        let keeps_grace = !matches!(
            result,
            Err(ServerError::AccessRevoked
                | ServerError::IdleTimeout
                | ServerError::Protocol(_)
                | ServerError::UnexpectedChannel(_)
                | ServerError::UnexpectedMessage
                | ServerError::Crypto(_)
                | ServerError::Viewer(_))
        );
        let now = self.store.read(|_, now| now).unwrap_or(0);
        let mut tickets = self.tickets.lock().unwrap_or_else(PoisonError::into_inner);
        if keeps_grace {
            tickets.suspend(device, ticket, now);
        } else {
            tickets.revoke(device, ticket);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_failed_accepts_are_tolerated_and_a_stuck_listener_is_not() {
        let mut failures = AcceptFailures::default();
        for _ in 1..MAX_ACCEPT_FAILURES {
            assert!(failures.record());
        }
        assert!(!failures.record());
    }

    #[test]
    fn unauthenticated_connections_are_limited_per_client_address() {
        let limiter = IpLimiter::default();
        let first: IpAddr = "198.51.100.1".parse().unwrap();
        let second: IpAddr = "198.51.100.2".parse().unwrap();
        let mut held: Vec<_> = (0..MAX_UNAUTHENTICATED_PER_IP)
            .map(|_| limiter.enter(first, MAX_UNAUTHENTICATED_PER_IP).unwrap())
            .collect();
        assert!(limiter.enter(first, MAX_UNAUTHENTICATED_PER_IP).is_none());
        let other = limiter.enter(second, MAX_UNAUTHENTICATED_PER_IP);
        assert!(other.is_some());
        held.pop();
        assert!(limiter.enter(first, MAX_UNAUTHENTICATED_PER_IP).is_some());
        drop(held);
        drop(other);
        assert!(limiter.counts.lock().unwrap().is_empty());
    }

    #[test]
    fn a_mapped_or_same_network_address_shares_the_budget() {
        let limiter = IpLimiter::default();
        let _held: Vec<_> = (0..2)
            .map(|_| {
                limiter
                    .enter("::ffff:192.0.2.1".parse().unwrap(), 2)
                    .unwrap()
            })
            .collect();
        assert!(limiter.enter("192.0.2.1".parse().unwrap(), 2).is_none());
        let _v6 = limiter
            .enter("2001:db8:1:2::1".parse().unwrap(), 1)
            .unwrap();
        assert!(
            limiter
                .enter("2001:db8:1:2::9".parse().unwrap(), 1)
                .is_none()
        );
    }

    #[test]
    fn a_successful_accept_clears_the_count() {
        let mut failures = AcceptFailures::default();
        for _ in 1..MAX_ACCEPT_FAILURES {
            assert!(failures.record());
        }
        failures.reset();
        assert!(failures.record());
    }
}
