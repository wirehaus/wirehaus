use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;

pub const TOKEN_FILE: &str = "tunnel-token";
pub const REGISTRY_FILE: &str = "registry";
pub const DEFAULT_SLEEP_MINUTES: u32 = 30;
const TICK: Duration = Duration::from_secs(5);
const SYNC_EVERY: Duration = Duration::from_secs(120);
const POLL_EVERY: Duration = Duration::from_secs(3);
const MIN_POLL: u64 = 2;
const MAX_POLL: u64 = 60;
const RETRY_EVERY: Duration = Duration::from_secs(60);
const REFUSED_EVERY: Duration = Duration::from_secs(1800);
const MAX_TOKEN: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Active,
    Asleep(Duration),
    Awake(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnectorError {
    #[error("the registry does not know this tunnel")]
    Refused,
    #[error("the registry could not be reached: {0}")]
    Unreachable(String),
    #[error("the registry answered something unexpected")]
    Malformed,
}

pub trait Remote {
    fn sync(&self, name: &str, token: &str, sleep: bool) -> Result<Reply, ConnectorError>;
}

pub trait Tunnel {
    fn stop(&self) -> std::io::Result<()>;
    fn restart(&self) -> std::io::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Awake,
    Asleep,
}

#[derive(Debug)]
pub struct Keeper<R, T> {
    remote: R,
    tunnel: T,
    state_dir: PathBuf,
    name: String,
    sleep_after: Duration,
    phase: Phase,
    due: Instant,
    awake_since: Instant,
    last_sync: Instant,
    poll: Duration,
    restart_pending: bool,
}

impl<R: Remote, T: Tunnel> Keeper<R, T> {
    pub fn new(
        remote: R,
        tunnel: T,
        state_dir: &Path,
        name: &str,
        sleep_after: Duration,
        now: Instant,
    ) -> Self {
        Self {
            remote,
            tunnel,
            state_dir: state_dir.to_path_buf(),
            name: name.to_owned(),
            sleep_after,
            phase: Phase::Starting,
            due: now,
            awake_since: now,
            last_sync: now,
            poll: POLL_EVERY,
            restart_pending: false,
        }
    }

    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn step(&mut self, now: Instant, idle: Duration, busy: bool) {
        if now < self.due {
            return;
        }
        if self.restart_pending {
            self.restart(now);
            return;
        }
        let quiet = !busy && idle.min(now.duration_since(self.awake_since)) >= self.sleep_after;
        let sleep = match self.phase {
            Phase::Starting | Phase::Asleep => false,
            Phase::Awake if quiet => true,
            Phase::Awake if now.duration_since(self.last_sync) >= SYNC_EVERY => false,
            Phase::Awake => return,
        };
        let Some(token) = read_token(&self.state_dir) else {
            self.due = now + REFUSED_EVERY.min(RETRY_EVERY * 5);
            return;
        };
        match self.remote.sync(&self.name, &token, sleep) {
            Ok(Reply::Active) => {
                self.become_awake(now);
                if sleep {
                    self.due = now + SYNC_EVERY;
                }
            }
            Ok(Reply::Asleep(poll)) => {
                if self.phase != Phase::Asleep {
                    if let Err(error) = self.tunnel.stop() {
                        eprintln!("tunnel: could not stop the connector: {error}");
                    }
                    eprintln!("tunnel: asleep, it will be rebuilt when someone connects");
                }
                self.phase = Phase::Asleep;
                self.poll = poll;
                self.due = now + poll;
            }
            Ok(Reply::Awake(fresh)) => match write_token(&self.state_dir, &fresh) {
                Ok(()) => {
                    self.restart_pending = true;
                    self.restart(now);
                }
                Err(error) => {
                    eprintln!("tunnel: could not save the new token: {error}");
                    self.due = now + RETRY_EVERY;
                }
            },
            Err(ConnectorError::Refused) => self.due = now + REFUSED_EVERY,
            Err(error) => {
                eprintln!("tunnel: {error}");
                self.due = now
                    + if self.phase == Phase::Asleep {
                        self.poll
                    } else {
                        RETRY_EVERY
                    };
            }
        }
    }

    fn restart(&mut self, now: Instant) {
        match self.tunnel.restart() {
            Ok(()) => {
                eprintln!("tunnel: rebuilt, connecting");
                self.restart_pending = false;
                self.become_awake(now);
                self.awake_since = now;
            }
            Err(error) => {
                eprintln!("tunnel: could not start the connector: {error}");
                self.due = now + RETRY_EVERY;
            }
        }
    }

    fn become_awake(&mut self, now: Instant) {
        if self.phase != Phase::Awake {
            self.awake_since = now;
        }
        self.phase = Phase::Awake;
        self.last_sync = now;
        self.due = now + TICK;
    }
}

#[must_use]
pub fn read_token(state_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(state_dir.join(TOKEN_FILE)).ok()?;
    let token = text.trim();
    valid_token(token).then(|| token.to_owned())
}

#[must_use]
pub fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_TOKEN
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'))
}

pub fn write_token(state_dir: &Path, token: &str) -> std::io::Result<()> {
    if !valid_token(token) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the registry sent a malformed token",
        ));
    }
    let target = state_dir.join(TOKEN_FILE);
    let partial = state_dir.join(format!("{TOKEN_FILE}.new"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&partial)?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, &target)
}

#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[must_use]
pub fn registry_url(state_dir: &Path, public_host: &str) -> Option<String> {
    if let Ok(url) = std::env::var("WIREHAUS_REGISTRY_URL") {
        return Some(url.trim().trim_end_matches('/').to_owned());
    }
    if let Ok(text) = fs::read_to_string(state_dir.join(REGISTRY_FILE)) {
        let line = text.trim().trim_end_matches('/');
        if line.starts_with("https://") && !line.contains([' ', '#', '?']) {
            return Some(line.to_owned());
        }
    }
    let (_, zone) = public_host.split_once('.')?;
    (!zone.is_empty()).then(|| format!("https://api.{zone}"))
}

#[derive(Debug)]
pub struct CurlRemote {
    base: String,
}

impl CurlRemote {
    #[must_use]
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
        }
    }
}

fn field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\":\"");
    let start = body.find(&marker)? + marker.len();
    let end = body[start..].find('"')? + start;
    Some(&body[start..end])
}

fn poll_of(body: &str) -> Duration {
    let marker = "\"poll\":";
    let seconds = body
        .find(marker)
        .map(|at| &body[at + marker.len()..])
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().ok()
        })
        .unwrap_or(POLL_EVERY.as_secs());
    Duration::from_secs(seconds.clamp(MIN_POLL, MAX_POLL))
}

pub fn parse_reply(status: u16, body: &str) -> Result<Reply, ConnectorError> {
    match status {
        200 => match field(body, "state") {
            Some("active") => Ok(Reply::Active),
            Some("asleep") => Ok(Reply::Asleep(poll_of(body))),
            Some("awake") => field(body, "connector_token")
                .filter(|t| valid_token(t))
                .map(|t| Reply::Awake(t.to_owned()))
                .ok_or(ConnectorError::Malformed),
            _ => Err(ConnectorError::Malformed),
        },
        401 | 403 | 404 => Err(ConnectorError::Refused),
        other => Err(ConnectorError::Unreachable(format!("HTTP {other}"))),
    }
}

impl Remote for CurlRemote {
    fn sync(&self, name: &str, token: &str, sleep: bool) -> Result<Reply, ConnectorError> {
        if !valid_name(name) || !valid_token(token) {
            return Err(ConnectorError::Malformed);
        }
        let version = env!("CARGO_PKG_VERSION");
        let body = format!(
            r#"{{"name":"{name}","token":"{token}","sleep":{sleep},"version":"{version}"}}"#
        );
        let protocols = if self.base.starts_with("http://") {
            "=http,https"
        } else {
            "=https"
        };
        let mut child = Command::new("curl")
            .args(["-sS", "--max-time", "25", "--proto", protocols])
            .args(["--proto-redir", "=https", "-X", "POST"])
            .args(["-H", "Content-Type: application/json"])
            .args(["--data-binary", "@-", "-w", "\n%{http_code}"])
            .arg(format!("{}/v1/connector/sync", self.base))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ConnectorError::Unreachable(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(body.as_bytes())
                .map_err(|e| ConnectorError::Unreachable(e.to_string()))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| ConnectorError::Unreachable(e.to_string()))?;
        if !output.status.success() {
            return Err(ConnectorError::Unreachable(format!(
                "curl exited with {}",
                output.status
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let (body, status) = text
            .rsplit_once('\n')
            .ok_or_else(|| ConnectorError::Unreachable("empty answer".to_owned()))?;
        let status = status
            .trim()
            .parse()
            .map_err(|_| ConnectorError::Malformed)?;
        parse_reply(status, body)
    }
}

#[derive(Debug, Default)]
pub struct ServiceTunnel;

fn run(program: &str, args: &[&str]) -> std::io::Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{program} exited with {status}"
        )))
    }
}

#[cfg(target_os = "linux")]
impl Tunnel for ServiceTunnel {
    fn stop(&self) -> std::io::Result<()> {
        run("systemctl", &["--user", "stop", "wirehaus-tunnel.service"])
    }

    fn restart(&self) -> std::io::Result<()> {
        run(
            "systemctl",
            &["--user", "restart", "wirehaus-tunnel.service"],
        )
    }
}

#[cfg(target_os = "macos")]
fn mac_domain() -> std::io::Result<String> {
    let output = Command::new("id").arg("-u").output()?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(std::io::Error::other("cannot find the user id"));
    }
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
impl Tunnel for ServiceTunnel {
    fn stop(&self) -> std::io::Result<()> {
        let domain = mac_domain()?;
        run(
            "launchctl",
            &["bootout", &format!("{domain}/wirehaus.tunnel")],
        )
    }

    fn restart(&self) -> std::io::Result<()> {
        let domain = mac_domain()?;
        if run(
            "launchctl",
            &["kickstart", "-k", &format!("{domain}/wirehaus.tunnel")],
        )
        .is_ok()
        {
            return Ok(());
        }
        let home = std::env::var("HOME").map_err(std::io::Error::other)?;
        let plist = format!("{home}/Library/LaunchAgents/wirehaus.tunnel.plist");
        run("launchctl", &["bootstrap", &domain, &plist])
    }
}

#[cfg(windows)]
fn stop_task() -> std::io::Result<()> {
    let _ = run("schtasks", &["/End", "/TN", "WirehausTunnel"]);
    let exe = std::env::current_exe()?;
    let connector = exe
        .parent()
        .map(|dir| dir.join("cloudflared.exe"))
        .ok_or_else(|| std::io::Error::other("no install folder"))?;
    let script = format!(
        "Get-Process cloudflared -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{}' }} | Stop-Process -Force; exit 0",
        connector.display().to_string().replace('\'', "''")
    );
    run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
}

#[cfg(windows)]
impl Tunnel for ServiceTunnel {
    fn stop(&self) -> std::io::Result<()> {
        stop_task()
    }

    fn restart(&self) -> std::io::Result<()> {
        let _ = stop_task();
        run("schtasks", &["/Run", "/TN", "WirehausTunnel"])
    }
}

pub fn run_background<R: Remote, T: Tunnel>(
    mut keeper: Keeper<R, T>,
    idle: &(dyn Fn() -> Duration + Sync),
    busy: &(dyn Fn() -> bool + Sync),
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        keeper.step(Instant::now(), idle(), busy());
        let mut left = TICK;
        while left > Duration::ZERO && !stop.load(Ordering::Relaxed) {
            let slice = left.min(Duration::from_secs(1));
            std::thread::sleep(slice);
            left -= slice;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct Calls {
        syncs: Vec<(String, bool)>,
        stops: u32,
        restarts: u32,
        replies: Vec<Result<Reply, ConnectorError>>,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<RefCell<Calls>>);

    impl Remote for Fake {
        fn sync(&self, _: &str, token: &str, sleep: bool) -> Result<Reply, ConnectorError> {
            let mut calls = self.0.borrow_mut();
            calls.syncs.push((token.to_owned(), sleep));
            if calls.replies.is_empty() {
                Ok(Reply::Active)
            } else {
                calls.replies.remove(0)
            }
        }
    }

    impl Tunnel for Fake {
        fn stop(&self) -> std::io::Result<()> {
            self.0.borrow_mut().stops += 1;
            Ok(())
        }

        fn restart(&self) -> std::io::Result<()> {
            self.0.borrow_mut().restarts += 1;
            Ok(())
        }
    }

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wirehaus-connector-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(TOKEN_FILE), "tokenviejo\n").unwrap();
        dir
    }

    const HALF_HOUR: Duration = Duration::from_secs(1800);
    const MIN: Duration = Duration::from_secs(60);

    fn keeper(
        name: &str,
        replies: Vec<Result<Reply, ConnectorError>>,
    ) -> (Keeper<Fake, Fake>, Fake, PathBuf, Instant) {
        let fake = Fake::default();
        fake.0.borrow_mut().replies = replies;
        let dir = dir(name);
        let start = Instant::now();
        let keeper = Keeper::new(fake.clone(), fake.clone(), &dir, "ana", HALF_HOUR, start);
        (keeper, fake, dir, start)
    }

    #[test]
    fn it_reports_in_at_start_and_stays_awake_while_the_computer_is_used() {
        let (mut k, fake, dir, start) = keeper("awake", vec![]);
        k.step(start, Duration::ZERO, false);
        assert_eq!(k.phase(), Phase::Awake);
        for minute in 1..=29 {
            k.step(start + MIN * minute, MIN * minute, false);
        }
        assert_eq!(k.phase(), Phase::Awake);
        assert!(fake.0.borrow().syncs.iter().all(|(_, sleep)| !sleep));
        assert_eq!(fake.0.borrow().stops, 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn half_an_hour_without_use_puts_the_tunnel_to_sleep_and_stops_the_connector() {
        let (mut k, fake, dir, start) = keeper(
            "sleep",
            vec![Ok(Reply::Active), Ok(Reply::Asleep(POLL_EVERY))],
        );
        k.step(start, Duration::ZERO, false);
        k.step(start + HALF_HOUR, HALF_HOUR, false);
        assert_eq!(k.phase(), Phase::Asleep);
        assert!(fake.0.borrow().syncs.last().unwrap().1);
        assert_eq!(fake.0.borrow().stops, 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_open_session_or_recent_use_keeps_it_awake_however_long_the_agent_has_run() {
        let (mut k, fake, dir, start) = keeper("busy", vec![]);
        k.step(start, Duration::ZERO, false);
        let later = start + HALF_HOUR * 4;
        k.step(later, Duration::ZERO, true);
        k.step(later + MIN, MIN, false);
        assert_eq!(k.phase(), Phase::Awake);
        assert!(fake.0.borrow().syncs.iter().all(|(_, sleep)| !sleep));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_sleeping_agent_polls_and_rebuilds_the_tunnel_when_the_registry_hands_it_a_token() {
        let (mut k, fake, dir, start) = keeper(
            "wake",
            vec![
                Ok(Reply::Asleep(POLL_EVERY)),
                Ok(Reply::Asleep(POLL_EVERY)),
                Ok(Reply::Awake("tokennuevo".to_owned())),
            ],
        );
        k.step(start, Duration::ZERO, false);
        assert_eq!(k.phase(), Phase::Asleep);
        k.step(start + Duration::from_secs(1), Duration::ZERO, false);
        assert_eq!(fake.0.borrow().syncs.len(), 1);
        k.step(start + POLL_EVERY, Duration::ZERO, false);
        k.step(start + POLL_EVERY * 2, Duration::ZERO, false);
        assert_eq!(k.phase(), Phase::Awake);
        assert_eq!(fake.0.borrow().restarts, 1);
        assert_eq!(fake.0.borrow().stops, 1);
        assert_eq!(
            fs::read_to_string(dir.join(TOKEN_FILE)).unwrap(),
            "tokennuevo"
        );
        k.step(start + POLL_EVERY * 2 + MIN, HALF_HOUR * 3, false);
        assert_eq!(k.phase(), Phase::Awake);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_freshly_woken_tunnel_gets_a_full_grace_period_even_if_nobody_used_the_agent_before() {
        let (mut k, _fake, dir, start) = keeper(
            "grace",
            vec![
                Ok(Reply::Asleep(POLL_EVERY)),
                Ok(Reply::Awake("tokennuevo".to_owned())),
            ],
        );
        k.step(start, Duration::ZERO, false);
        let woke = start + HALF_HOUR * 10;
        k.step(woke, HALF_HOUR * 10, false);
        assert_eq!(k.phase(), Phase::Awake);
        k.step(woke + MIN * 10, HALF_HOUR * 10, false);
        assert_eq!(k.phase(), Phase::Awake);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_network_failure_never_puts_the_tunnel_to_sleep_and_is_retried() {
        let (mut k, fake, dir, start) = keeper(
            "net",
            vec![
                Ok(Reply::Active),
                Err(ConnectorError::Unreachable("x".to_owned())),
            ],
        );
        k.step(start, Duration::ZERO, false);
        k.step(start + HALF_HOUR, HALF_HOUR, false);
        assert_eq!(k.phase(), Phase::Awake);
        assert_eq!(fake.0.borrow().stops, 0);
        k.step(
            start + HALF_HOUR + Duration::from_secs(10),
            HALF_HOUR,
            false,
        );
        assert_eq!(fake.0.borrow().syncs.len(), 2);
        k.step(start + HALF_HOUR + MIN, HALF_HOUR, false);
        assert_eq!(fake.0.borrow().syncs.len(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_registry_that_does_not_know_the_tunnel_is_left_alone_for_a_long_while() {
        let (mut k, fake, dir, start) = keeper("refused", vec![Err(ConnectorError::Refused)]);
        k.step(start, Duration::ZERO, false);
        k.step(start + MIN * 10, Duration::ZERO, false);
        assert_eq!(fake.0.borrow().syncs.len(), 1);
        assert_eq!(fake.0.borrow().stops, 0);
        k.step(start + REFUSED_EVERY, Duration::ZERO, false);
        assert_eq!(fake.0.borrow().syncs.len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn without_a_token_file_there_is_nothing_to_keep() {
        let (mut k, fake, dir, start) = keeper("notoken", vec![]);
        fs::remove_file(dir.join(TOKEN_FILE)).unwrap();
        k.step(start, Duration::ZERO, false);
        assert_eq!(fake.0.borrow().syncs.len(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_token_that_cannot_be_saved_is_not_applied_and_the_old_one_is_kept() {
        let (mut k, fake, dir, start) =
            keeper("nosave", vec![Ok(Reply::Awake("tokennuevo".to_owned()))]);
        fs::create_dir(dir.join(format!("{TOKEN_FILE}.new"))).unwrap();
        k.step(start, Duration::ZERO, false);
        assert_eq!(fake.0.borrow().restarts, 0);
        assert_eq!(k.phase(), Phase::Starting);
        assert_eq!(
            fs::read_to_string(dir.join(TOKEN_FILE)).unwrap(),
            "tokenviejo\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[derive(Clone, Default)]
    struct FailingTunnel(Rc<RefCell<(u32, bool)>>);

    impl Tunnel for FailingTunnel {
        fn stop(&self) -> std::io::Result<()> {
            Ok(())
        }

        fn restart(&self) -> std::io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.0 += 1;
            if state.1 {
                Err(std::io::Error::other("systemd said no"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_connector_that_does_not_start_is_retried_and_not_taken_for_awake() {
        let fake = Fake::default();
        fake.0.borrow_mut().replies = vec![
            Ok(Reply::Asleep(POLL_EVERY)),
            Ok(Reply::Awake("tokennuevo".to_owned())),
        ];
        let tunnel = FailingTunnel::default();
        tunnel.0.borrow_mut().1 = true;
        let dir = dir("restartfails");
        let start = Instant::now();
        let mut k = Keeper::new(fake.clone(), tunnel.clone(), &dir, "ana", HALF_HOUR, start);
        k.step(start, Duration::ZERO, false);
        let woke = start + POLL_EVERY;
        k.step(woke, Duration::ZERO, false);
        assert_eq!(k.phase(), Phase::Asleep);
        assert_eq!(tunnel.0.borrow().0, 1);
        assert_eq!(
            fs::read_to_string(dir.join(TOKEN_FILE)).unwrap(),
            "tokennuevo"
        );
        k.step(woke + Duration::from_secs(10), Duration::ZERO, false);
        assert_eq!(tunnel.0.borrow().0, 1);
        tunnel.0.borrow_mut().1 = false;
        k.step(woke + RETRY_EVERY, Duration::ZERO, false);
        assert_eq!(tunnel.0.borrow().0, 2);
        assert_eq!(k.phase(), Phase::Awake);
        assert_eq!(fake.0.borrow().syncs.len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn when_the_registry_declines_to_put_it_to_sleep_it_waits_before_asking_again() {
        let (mut k, fake, dir, start) =
            keeper("declined", vec![Ok(Reply::Active), Ok(Reply::Active)]);
        k.step(start, Duration::ZERO, false);
        let asked = start + HALF_HOUR;
        k.step(asked, HALF_HOUR, false);
        assert!(fake.0.borrow().syncs.last().unwrap().1);
        let count = fake.0.borrow().syncs.len();
        for seconds in [5, 30, 60, 110] {
            k.step(asked + Duration::from_secs(seconds), HALF_HOUR, false);
        }
        assert_eq!(fake.0.borrow().syncs.len(), count);
        k.step(asked + SYNC_EVERY, HALF_HOUR, false);
        assert_eq!(fake.0.borrow().syncs.len(), count + 1);
        assert_eq!(k.phase(), Phase::Awake);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replies_are_parsed_strictly() {
        assert_eq!(parse_reply(200, r#"{"state":"active"}"#), Ok(Reply::Active));
        assert_eq!(
            parse_reply(200, r#"{"state":"asleep"}"#),
            Ok(Reply::Asleep(POLL_EVERY))
        );
        assert_eq!(
            parse_reply(200, r#"{"poll":15,"state":"asleep"}"#),
            Ok(Reply::Asleep(Duration::from_secs(15)))
        );
        assert_eq!(
            parse_reply(200, r#"{"poll":9999,"state":"asleep"}"#),
            Ok(Reply::Asleep(Duration::from_secs(MAX_POLL)))
        );
        assert_eq!(
            parse_reply(200, r#"{"poll":0,"state":"asleep"}"#),
            Ok(Reply::Asleep(Duration::from_secs(MIN_POLL)))
        );
        assert_eq!(
            parse_reply(200, r#"{"state":"awake","connector_token":"abc="}"#),
            Ok(Reply::Awake("abc=".to_owned()))
        );
        assert_eq!(
            parse_reply(200, r#"{"state":"awake"}"#),
            Err(ConnectorError::Malformed)
        );
        assert_eq!(
            parse_reply(200, r#"{"state":"awake","connector_token":"a b"}"#),
            Err(ConnectorError::Malformed)
        );
        assert_eq!(parse_reply(200, "<html>"), Err(ConnectorError::Malformed));
        assert_eq!(parse_reply(401, ""), Err(ConnectorError::Refused));
        assert!(matches!(
            parse_reply(429, ""),
            Err(ConnectorError::Unreachable(_))
        ));
        assert!(matches!(
            parse_reply(503, ""),
            Err(ConnectorError::Unreachable(_))
        ));
    }

    #[test]
    fn the_registry_address_comes_from_the_environment_a_file_or_the_public_host() {
        let dir = dir("url");
        assert_eq!(
            registry_url(&dir, "ana.wirehaus.dev").as_deref(),
            Some("https://api.wirehaus.dev")
        );
        fs::write(
            dir.join(REGISTRY_FILE),
            "https://api-pruebas.wirehaus.dev/\n",
        )
        .unwrap();
        assert_eq!(
            registry_url(&dir, "ana.pruebas.wirehaus.dev").as_deref(),
            Some("https://api-pruebas.wirehaus.dev")
        );
        fs::write(dir.join(REGISTRY_FILE), "http://evil.example\n").unwrap();
        assert_eq!(
            registry_url(&dir, "ana.wirehaus.dev").as_deref(),
            Some("https://api.wirehaus.dev")
        );
        assert_eq!(registry_url(&dir, "localhost"), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_and_tokens_that_could_break_the_request_are_refused_before_sending() {
        assert!(valid_name("ana-2"));
        assert!(!valid_name("Ana"));
        assert!(!valid_name("a\"b"));
        assert!(!valid_name(""));
        assert!(valid_token("eyJhIjoi+/=_-"));
        assert!(!valid_token("a\"b"));
        assert!(!valid_token(&"a".repeat(MAX_TOKEN + 1)));
        let remote = CurlRemote::new("https://api.example");
        assert_eq!(
            remote.sync("ana", "a\"b", false),
            Err(ConnectorError::Malformed)
        );
    }

    #[test]
    fn the_new_token_is_written_whole_and_private() {
        let dir = dir("write");
        write_token(&dir, "tokennuevo").unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(TOKEN_FILE)).unwrap(),
            "tokennuevo"
        );
        assert!(!dir.join(format!("{TOKEN_FILE}.new")).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join(TOKEN_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
        assert!(write_token(&dir, "mal token").is_err());
        assert_eq!(
            fs::read_to_string(dir.join(TOKEN_FILE)).unwrap(),
            "tokennuevo"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
