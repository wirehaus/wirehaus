use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, SystemTime};

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256, Sha512};
use thiserror::Error;

pub const NAMESPACE: &str = "wirehaus-release";
pub const DEFAULT_BASE: &str = "https://wirehaus.dev/download";
pub const RESTART_EXIT_CODE: u8 = 75;
pub const MAX_ATTEMPTS: u32 = 3;
const TRUSTED_KEYS: &[&str] =
    &["AAAAC3NzaC1lZDI1NTE5AAAAIALukD9B1SrSO6OWUzWaWSoqVJ6IVl87Dwkm6oGhpYFu"];
const MAX_SUMS: u64 = 64 * 1024;
const MAX_SIGNATURE: u64 = 8 * 1024;
const MAX_PACKAGE: u64 = 200 * 1024 * 1024;
const PENDING: &str = "update-pending";
const SKIP: &str = "update-skip";
const DISABLED: &str = "update-disabled";

static RESTART: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn restart_requested() -> bool {
    RESTART.load(AtomicOrdering::SeqCst)
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("the release signature is invalid: {0}")]
    Signature(&'static str),
    #[error("the release list is malformed")]
    Malformed,
    #[error("download failed: {0}")]
    Download(String),
    #[error("the package does not match the signed checksum")]
    Checksum,
    #[error("the package is incomplete: {0}")]
    Package(&'static str),
    #[error("could not extract the package")]
    Extract,
    #[error("no managed installation was found next to the agent")]
    NotManaged,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let version = Self(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        parts.next().is_none().then_some(version)
    }

    #[must_use]
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub file: String,
    pub sha256: String,
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut bits = 0_u32;
    let mut count = 0;
    for byte in text.bytes().filter(|b| !b.is_ascii_whitespace()) {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push(u8::try_from((bits >> count) & 0xff).ok()?);
        }
    }
    Some(out)
}

struct Wire<'a>(&'a [u8]);

impl<'a> Wire<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.0.split_at_checked(count)?;
        self.0 = tail;
        Some(head)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<&'a [u8]> {
        let length = usize::try_from(self.u32()?).ok()?;
        self.take(length)
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(value);
}

#[must_use]
pub fn parse_key_blob(blob: &[u8]) -> Option<[u8; 32]> {
    let mut wire = Wire(blob);
    if wire.string()? != b"ssh-ed25519" {
        return None;
    }
    let key: [u8; 32] = wire.string()?.try_into().ok()?;
    wire.done().then_some(key)
}

#[must_use]
pub fn trusted_keys() -> Vec<[u8; 32]> {
    TRUSTED_KEYS
        .iter()
        .filter_map(|text| parse_key_blob(&base64_decode(text)?))
        .collect()
}

pub fn verify_signature(
    armored: &str,
    message: &[u8],
    trusted: &[[u8; 32]],
) -> Result<(), UpdateError> {
    const BEGIN: &str = "-----BEGIN SSH SIGNATURE-----";
    const END: &str = "-----END SSH SIGNATURE-----";
    let bad = UpdateError::Signature;
    let body = armored
        .trim()
        .strip_prefix(BEGIN)
        .and_then(|rest| rest.trim_end().strip_suffix(END))
        .ok_or(bad("not an SSH signature"))?;
    let blob = base64_decode(body).ok_or(bad("not base64"))?;
    let mut wire = Wire(&blob);
    if wire.take(6) != Some(b"SSHSIG") || wire.u32() != Some(1) {
        return Err(bad("unknown format"));
    }
    let key =
        parse_key_blob(wire.string().ok_or(bad("truncated"))?).ok_or(bad("unsupported key"))?;
    let namespace = wire.string().ok_or(bad("truncated"))?;
    let reserved = wire.string().ok_or(bad("truncated"))?;
    let algorithm = wire.string().ok_or(bad("truncated"))?;
    let signature_blob = wire.string().ok_or(bad("truncated"))?;
    if !wire.done() {
        return Err(bad("trailing data"));
    }
    if namespace != NAMESPACE.as_bytes() {
        return Err(bad("wrong namespace"));
    }
    if !trusted.contains(&key) {
        return Err(bad("untrusted key"));
    }
    let digest: Vec<u8> = match algorithm {
        b"sha512" => Sha512::digest(message).to_vec(),
        b"sha256" => Sha256::digest(message).to_vec(),
        _ => return Err(bad("unsupported hash")),
    };
    let mut inner = Wire(signature_blob);
    if inner.string() != Some(b"ssh-ed25519") {
        return Err(bad("unsupported signature"));
    }
    let raw: [u8; 64] = inner
        .string()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(bad("bad signature length"))?;
    if !inner.done() {
        return Err(bad("trailing data"));
    }
    let mut signed = b"SSHSIG".to_vec();
    put_string(&mut signed, namespace);
    put_string(&mut signed, reserved);
    put_string(&mut signed, algorithm);
    put_string(&mut signed, &digest);
    VerifyingKey::from_bytes(&key)
        .map_err(|_| bad("bad key"))?
        .verify_strict(&signed, &Signature::from_bytes(&raw))
        .map_err(|_| bad("does not verify"))
}

pub fn parse_sums(text: &str) -> Result<Vec<(String, String)>, UpdateError> {
    let mut out = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (hash, rest) = line.split_once(' ').ok_or(UpdateError::Malformed)?;
        let file = rest.trim_start_matches([' ', '*']);
        let hash_ok =
            hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        let file_ok = !file.is_empty()
            && file
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'/'))
            && !file.starts_with('/')
            && !file.split('/').any(|part| part == ".." || part.is_empty());
        if !hash_ok || !file_ok {
            return Err(UpdateError::Malformed);
        }
        out.push((hash.to_owned(), file.to_owned()));
    }
    Ok(out)
}

#[must_use]
pub fn asset_suffix() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64.tar.gz")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("windows-x64.zip")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("darwin-arm64.tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("darwin-x86_64.tar.gz")
    } else {
        None
    }
}

#[must_use]
pub fn select_release(
    sums: &[(String, String)],
    suffix: &str,
    current: Version,
    skip: Option<Version>,
) -> Option<Release> {
    sums.iter()
        .filter_map(|(hash, file)| {
            let version = file
                .strip_prefix("wirehaus-")?
                .strip_suffix(suffix)?
                .strip_suffix('-')?;
            let version = Version::parse(version)?;
            Some(Release {
                version,
                file: file.clone(),
                sha256: hash.clone(),
            })
        })
        .filter(|release| release.version > current && Some(release.version) != skip)
        .max_by(|a, b| a.version.cmp(&b.version))
}

pub trait Fetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, UpdateError>;
}

#[derive(Debug, Default)]
pub struct Curl {
    pub allow_http: bool,
}

impl Fetch for Curl {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, UpdateError> {
        let protocols = if self.allow_http {
            "=http,https"
        } else {
            "=https"
        };
        let output = Command::new("curl")
            .args([
                "-fsSL",
                "--max-time",
                "600",
                "--max-filesize",
                &max.to_string(),
            ])
            .args(["--proto", protocols, "--proto-redir", "=https"])
            .arg(url)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|error| UpdateError::Download(error.to_string()))?;
        if !output.status.success() {
            return Err(UpdateError::Download(format!(
                "curl exited with {}",
                output.status
            )));
        }
        if u64::try_from(output.stdout.len()).unwrap_or(u64::MAX) > max {
            return Err(UpdateError::Download("response too large".to_owned()));
        }
        Ok(output.stdout)
    }
}

pub fn check(
    fetch: &dyn Fetch,
    base: &str,
    current: Version,
    skip: Option<Version>,
    trusted: &[[u8; 32]],
) -> Result<Option<Release>, UpdateError> {
    let base = base.trim_end_matches('/');
    let sums = fetch.get(&format!("{base}/SHA256SUMS"), MAX_SUMS)?;
    let signature = fetch.get(&format!("{base}/SHA256SUMS.sig"), MAX_SIGNATURE)?;
    let signature = String::from_utf8(signature).map_err(|_| UpdateError::Signature("not text"))?;
    verify_signature(&signature, &sums, trusted)?;
    let text = String::from_utf8(sums).map_err(|_| UpdateError::Malformed)?;
    let Some(suffix) = asset_suffix() else {
        return Ok(None);
    };
    Ok(select_release(&parse_sums(&text)?, suffix, current, skip))
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}

fn tar_program() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        root.join("System32").join("tar.exe")
    } else {
        PathBuf::from("tar")
    }
}

fn verify_extracted(root: &Path) -> Result<Vec<(String, String)>, UpdateError> {
    let sums = fs::read_to_string(root.join("SHA256SUMS"))
        .map_err(|_| UpdateError::Package("SHA256SUMS missing"))?;
    let listed = parse_sums(&sums)?;
    for (hash, file) in &listed {
        if sha256_file(&root.join(file)).map_err(|_| UpdateError::Package("listed file missing"))?
            != *hash
        {
            return Err(UpdateError::Checksum);
        }
    }
    Ok(listed)
}

pub fn stage(
    fetch: &dyn Fetch,
    base: &str,
    release: &Release,
    staging: &Path,
) -> Result<PathBuf, UpdateError> {
    let base = base.trim_end_matches('/');
    let package = fetch.get(&format!("{base}/{}", release.file), MAX_PACKAGE)?;
    if hex(&Sha256::digest(&package)) != release.sha256 {
        return Err(UpdateError::Checksum);
    }
    let _ = fs::remove_dir_all(staging);
    fs::create_dir_all(staging)?;
    let archive = staging.join("package");
    fs::File::create(&archive)?.write_all(&package)?;
    let extracted = staging.join("files");
    fs::create_dir_all(&extracted)?;
    let status = Command::new(tar_program())
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&extracted)
        .stdin(Stdio::null())
        .status()
        .map_err(|_| UpdateError::Extract)?;
    if !status.success() {
        return Err(UpdateError::Extract);
    }
    let mut roots = fs::read_dir(&extracted)?.collect::<Result<Vec<_>, _>>()?;
    let root = match (roots.pop(), roots.is_empty()) {
        (Some(entry), true) if entry.path().is_dir() => entry.path(),
        _ => return Err(UpdateError::Package("unexpected layout")),
    };
    verify_extracted(&root)?;
    let binary = if cfg!(windows) {
        "wirehaus-agent.exe"
    } else {
        "wirehaus-agent"
    };
    if !root.join(binary).is_file()
        || !root.join("viewer").is_dir()
        || !root.join("viewer").join("manifest.sha256").is_file()
    {
        return Err(UpdateError::Package("agent or viewer missing"));
    }
    Ok(root)
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub install_dir: PathBuf,
    pub state_dir: PathBuf,
    pub exe_name: String,
}

fn real_exe() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if cfg!(unix) {
        Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
    } else {
        Ok(exe)
    }
}

impl Layout {
    pub fn detect(state_dir: &Path) -> Result<Self, UpdateError> {
        let exe = real_exe()?;
        let install_dir = exe.parent().ok_or(UpdateError::NotManaged)?;
        let exe_name = exe
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(UpdateError::NotManaged)?;
        Self::detect_in(install_dir, state_dir, exe_name)
    }

    pub fn detect_in(
        install_dir: &Path,
        state_dir: &Path,
        exe_name: &str,
    ) -> Result<Self, UpdateError> {
        if !install_dir.join("viewer").is_dir() && !install_dir.join("viewer.prev").is_dir() {
            return Err(UpdateError::NotManaged);
        }
        Ok(Self {
            install_dir: install_dir.to_path_buf(),
            state_dir: state_dir.to_path_buf(),
            exe_name: exe_name.to_owned(),
        })
    }

    #[must_use]
    pub fn staging(&self) -> PathBuf {
        self.install_dir.join(".update")
    }

    fn exe(&self) -> PathBuf {
        self.install_dir.join(&self.exe_name)
    }

    fn side(&self, suffix: &str) -> PathBuf {
        self.install_dir.join(format!("{}.{suffix}", self.exe_name))
    }

    fn viewer(&self) -> PathBuf {
        self.install_dir.join("viewer")
    }

    fn viewer_prev(&self) -> PathBuf {
        self.install_dir.join("viewer.prev")
    }
}

fn remove_any(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(path);
}

pub fn smoke_test(root: &Path, exe_name: &str, expected: Version) -> Result<(), UpdateError> {
    let output = Command::new(root.join(exe_name))
        .arg("info")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| UpdateError::Package("the new agent does not run"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let reported = text
        .strip_prefix("wirehaus-agent ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(Version::parse);
    if output.status.success() && reported == Some(expected) {
        Ok(())
    } else {
        Err(UpdateError::Package(
            "the new agent reports another version",
        ))
    }
}

pub fn recover(layout: &Layout) {
    if !layout.viewer().is_dir() && layout.viewer_prev().is_dir() {
        let _ = fs::rename(layout.viewer_prev(), layout.viewer());
    }
}

pub fn swap(layout: &Layout, root: &Path, from: Version, to: Version) -> Result<(), UpdateError> {
    write_pending(&layout.state_dir, from, to, 0)?;
    let outcome = swap_files(layout, root);
    if outcome.is_err() {
        mark_healthy(&layout.state_dir);
    } else {
        let _ = fs::remove_dir_all(layout.staging());
    }
    outcome
}

fn swap_files(layout: &Layout, root: &Path) -> Result<(), UpdateError> {
    let listed = verify_extracted(root)?;
    let new_exe = layout.side("new");
    remove_any(&new_exe);
    fs::copy(root.join(&layout.exe_name), &new_exe)?;
    if let Some((hash, _)) = listed.iter().find(|(_, file)| *file == layout.exe_name)
        && sha256_file(&new_exe)? != *hash
    {
        remove_any(&new_exe);
        return Err(UpdateError::Checksum);
    }
    #[cfg(unix)]
    fs::set_permissions(
        &new_exe,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )?;
    remove_any(&layout.viewer_prev());
    let had_viewer = layout.viewer().is_dir();
    if had_viewer {
        fs::rename(layout.viewer(), layout.viewer_prev())?;
    }
    if let Err(error) = fs::rename(root.join("viewer"), layout.viewer()) {
        if had_viewer {
            let _ = fs::rename(layout.viewer_prev(), layout.viewer());
        }
        return Err(error.into());
    }
    remove_any(&layout.side("prev"));
    let kept = fs::hard_link(layout.exe(), layout.side("prev")).is_ok()
        || fs::copy(layout.exe(), layout.side("prev")).is_ok();
    if !kept {
        revert_viewer(layout);
        return Err(UpdateError::Package("could not keep the previous agent"));
    }
    if let Err(error) = fs::rename(&new_exe, layout.exe()) {
        if !layout.exe().exists() {
            let _ = fs::rename(layout.side("prev"), layout.exe());
        }
        revert_viewer(layout);
        return Err(error.into());
    }
    Ok(())
}

fn revert_viewer(layout: &Layout) {
    if layout.viewer_prev().is_dir() {
        remove_any(&layout.viewer());
        let _ = fs::rename(layout.viewer_prev(), layout.viewer());
    }
}

fn write_pending(
    state_dir: &Path,
    from: Version,
    to: Version,
    attempts: u32,
) -> Result<(), UpdateError> {
    fs::write(
        state_dir.join(PENDING),
        format!("from={from}\nto={to}\nattempts={attempts}\n"),
    )?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pending {
    pub from: Version,
    pub to: Version,
    pub attempts: u32,
}

#[must_use]
pub fn read_pending(state_dir: &Path) -> Option<Pending> {
    let text = fs::read_to_string(state_dir.join(PENDING)).ok()?;
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    };
    Some(Pending {
        from: Version::parse(value("from")?)?,
        to: Version::parse(value("to")?)?,
        attempts: value("attempts")?.parse().ok()?,
    })
}

pub fn mark_healthy(state_dir: &Path) {
    let _ = fs::remove_file(state_dir.join(PENDING));
}

pub fn rollback(layout: &Layout) -> Result<Option<Version>, UpdateError> {
    let failed =
        read_pending(&layout.state_dir).map_or_else(Version::current, |pending| pending.to);
    if !layout.side("prev").is_file() {
        mark_healthy(&layout.state_dir);
        return Ok(None);
    }
    remove_any(&layout.side("bad"));
    fs::rename(layout.exe(), layout.side("bad"))?;
    fs::rename(layout.side("prev"), layout.exe())?;
    revert_viewer(layout);
    fs::write(layout.state_dir.join(SKIP), failed.to_string())?;
    mark_healthy(&layout.state_dir);
    Ok(Some(failed))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Probation {
    None,
    Started,
    RolledBack(Version),
}

pub fn begin_probation(layout: &Layout) -> Result<Probation, UpdateError> {
    let Some(pending) = read_pending(&layout.state_dir) else {
        return Ok(Probation::None);
    };
    if pending.attempts >= MAX_ATTEMPTS {
        let version = rollback(layout)?.unwrap_or(pending.to);
        return Ok(Probation::RolledBack(version));
    }
    write_pending(
        &layout.state_dir,
        pending.from,
        pending.to,
        pending.attempts + 1,
    )?;
    Ok(Probation::Started)
}

#[must_use]
pub fn skipped(state_dir: &Path) -> Option<Version> {
    Version::parse(&fs::read_to_string(state_dir.join(SKIP)).ok()?)
}

#[must_use]
pub fn enabled(state_dir: &Path) -> bool {
    !state_dir.join(DISABLED).exists()
}

pub fn set_enabled(state_dir: &Path, on: bool) -> Result<(), UpdateError> {
    let marker = state_dir.join(DISABLED);
    if on {
        let _ = fs::remove_file(marker);
    } else {
        fs::write(marker, b"")?;
    }
    Ok(())
}

#[must_use]
pub fn base_url() -> String {
    std::env::var("WIREHAUS_UPDATE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_owned())
}

#[must_use]
pub fn fetcher() -> Curl {
    Curl {
        allow_http: std::env::var_os("WIREHAUS_UPDATE_URL").is_some(),
    }
}

pub fn apply_now(
    layout: &Layout,
    fetch: &dyn Fetch,
    base: &str,
) -> Result<Option<Version>, UpdateError> {
    let current = Version::current();
    let Some(release) = check(
        fetch,
        base,
        current,
        skipped(&layout.state_dir),
        &trusted_keys(),
    )?
    else {
        return Ok(None);
    };
    let root = stage(fetch, base, &release, &layout.staging())?;
    smoke_test(&root, &layout.exe_name, release.version)?;
    swap(layout, &root, current, release.version)?;
    Ok(Some(release.version))
}

pub fn run_background(
    layout: &Layout,
    is_busy: &(dyn Fn() -> bool + Sync),
    stop: &AtomicBool,
    interval: Duration,
    first_delay: Duration,
) {
    let sleep = |total: Duration| {
        let deadline = SystemTime::now() + total;
        let mut left = total;
        while left > Duration::ZERO && !stop.load(AtomicOrdering::Relaxed) {
            let slice = left.min(Duration::from_secs(1));
            std::thread::sleep(slice);
            left -= slice;
            if SystemTime::now() >= deadline {
                break;
            }
        }
    };
    sleep(first_delay);
    let mut failures = 0u32;
    let mut refused: Option<Version> = None;
    while !stop.load(AtomicOrdering::Relaxed) {
        let mut wait = interval;
        if enabled(&layout.state_dir) {
            let fetch = fetcher();
            let base = base_url();
            match check(
                &fetch,
                &base,
                Version::current(),
                skipped(&layout.state_dir),
                &trusted_keys(),
            ) {
                Ok(Some(release)) if refused == Some(release.version) => failures = 0,
                Ok(Some(release)) => {
                    failures = 0;
                    eprintln!("update: version {} is available", release.version);
                    match stage(&fetch, &base, &release, &layout.staging()) {
                        Ok(root) => {
                            if let Err(error) = smoke_test(&root, &layout.exe_name, release.version)
                            {
                                eprintln!("update: {} was refused: {error}", release.version);
                                refused = Some(release.version);
                                sleep(interval);
                                continue;
                            }
                            loop {
                                wait_until_idle(is_busy, stop, &sleep);
                                if stop.load(AtomicOrdering::Relaxed) {
                                    return;
                                }
                                if !is_busy() {
                                    break;
                                }
                            }
                            match swap(layout, &root, Version::current(), release.version) {
                                Ok(()) => {
                                    eprintln!("update: installed {}, restarting", release.version);
                                    RESTART.store(true, AtomicOrdering::SeqCst);
                                    stop.store(true, AtomicOrdering::SeqCst);
                                    return;
                                }
                                Err(error) => {
                                    eprintln!("update: could not install: {error}");
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("update: could not prepare {}: {error}", release.version);
                        }
                    }
                }
                Ok(None) => failures = 0,
                Err(error) => {
                    eprintln!("update: check failed: {error}");
                    failures += 1;
                    wait = retry_delay(failures, interval);
                }
            }
        }
        sleep(wait);
    }
}

#[must_use]
pub fn retry_delay(failures: u32, interval: Duration) -> Duration {
    let exponent = failures.saturating_sub(1).min(6);
    Duration::from_secs(30u64 << exponent)
        .min(Duration::from_secs(30 * 60))
        .min(interval)
}

fn wait_until_idle(
    is_busy: &(dyn Fn() -> bool + Sync),
    stop: &AtomicBool,
    sleep: &dyn Fn(Duration),
) {
    let mut quiet = 0;
    while !stop.load(AtomicOrdering::Relaxed) && quiet < 12 {
        if is_busy() {
            quiet = 0;
        } else {
            quiet += 1;
        }
        sleep(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    const RELEASE_KEY: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIALukD9B1SrSO6OWUzWaWSoqVJ6IVl87Dwkm6oGhpYFu";
    const SUMS: &str = "7122726c00ee93c81763444d19223e20fed84f31c61fa1dee151576b2452d274  wirehaus-0.2.1-linux-x86_64.tar.gz\na7b0aae2410d0f7bf76e283ac6571657803f5e5ee016c63adb781d08aede8bd7  wirehaus-0.2.1-windows-x64.zip\n";
    const SUMS_SIG: &str = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgAu6QP0HVKtI7o5ZTNZpZKipUno\nhWXzsPCSbqgaGlgW4AAAAQd2lyZWhhdXMtcmVsZWFzZQAAAAAAAAAGc2hhNTEyAAAAUwAA\nAAtzc2gtZWQyNTUxOQAAAEDI5OlNBDiEYwlrvTqyCt6rF+ZGN6azIUiO87PyZA559I2g4S\nhPK4GP+Oo2GGj5UMY2Kl5hRWS/vudegJboi3kK\n-----END SSH SIGNATURE-----\n";
    const OTHER_KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIKbmLqK43lMtzVpvCrhwFuwnpwb944Mc8FSE/fBFtmO2";
    const OTHER_NAMESPACE_SIG: &str = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgpuYuorjeUy3NWm8KuHAW7CenBv\n3jgxzwVIT98EW2Y7YAAAAFb3RoZXIAAAAAAAAABnNoYTUxMgAAAFMAAAALc3NoLWVkMjU1\nMTkAAABA+B/LxanN8Qgrsc6rh3vT8bI+7K8Hm52uSOvzudj69K7SrNtZCu0Of+u2C0xDV4\nSngxTeobrtrpZ7B+YE5j0kAQ==\n-----END SSH SIGNATURE-----\n";
    const OTHER_RELEASE_SIG: &str = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgpuYuorjeUy3NWm8KuHAW7CenBv\n3jgxzwVIT98EW2Y7YAAAAQd2lyZWhhdXMtcmVsZWFzZQAAAAAAAAAGc2hhNTEyAAAAUwAA\nAAtzc2gtZWQyNTUxOQAAAEADYyPiowhDYJUGbJHWU+ZZJm9ITZQVgyPvvBYVj/3yywsEAt\nJ8h79D4Me8lcQI/n/TtakZt3j0cMkf7GpOHGoI\n-----END SSH SIGNATURE-----\n";

    fn key(text: &str) -> [u8; 32] {
        parse_key_blob(&base64_decode(text).unwrap()).unwrap()
    }

    #[test]
    fn the_pinned_key_is_the_release_signer() {
        assert_eq!(trusted_keys(), vec![key(RELEASE_KEY)]);
    }

    #[test]
    fn a_real_signed_release_list_verifies() {
        verify_signature(SUMS_SIG, SUMS.as_bytes(), &[key(RELEASE_KEY)]).unwrap();
    }

    #[test]
    fn a_changed_list_a_wrong_key_or_a_wrong_namespace_is_refused() {
        let trusted = [key(RELEASE_KEY)];
        let altered = SUMS.replace("7122", "7123");
        assert!(verify_signature(SUMS_SIG, altered.as_bytes(), &trusted).is_err());
        assert!(verify_signature(SUMS_SIG, SUMS.as_bytes(), &[key(OTHER_KEY)]).is_err());
        let other = [key(OTHER_KEY)];
        assert!(verify_signature(OTHER_RELEASE_SIG, b"hello release\n", &other).is_ok());
        assert!(verify_signature(OTHER_RELEASE_SIG, b"hello release\n", &trusted).is_err());
        assert!(verify_signature(OTHER_NAMESPACE_SIG, b"hello release\n", &other).is_err());
    }

    #[test]
    fn garbage_signatures_are_refused_without_panicking() {
        let trusted = [key(RELEASE_KEY)];
        for text in [
            "",
            "nope",
            "-----BEGIN SSH SIGNATURE-----\n!!!\n-----END SSH SIGNATURE-----",
            "-----BEGIN SSH SIGNATURE-----\nU1NIU0lH\n-----END SSH SIGNATURE-----",
        ] {
            assert!(
                verify_signature(text, SUMS.as_bytes(), &trusted).is_err(),
                "{text}"
            );
        }
        let truncated = &SUMS_SIG[..SUMS_SIG.len() / 2];
        assert!(verify_signature(truncated, SUMS.as_bytes(), &trusted).is_err());
    }

    #[test]
    fn versions_parse_compare_and_reject_junk() {
        assert_eq!(Version::parse("0.3.10"), Some(Version(0, 3, 10)));
        assert!(Version(0, 3, 10) > Version(0, 3, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        for bad in ["", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.-3", "1.2.3-rc1"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_newest_greater_version_is_chosen_and_downgrades_are_never_offered() {
        let sums = parse_sums(&format!(
            "{h}  wirehaus-0.2.1-linux-x86_64.tar.gz\n{h}  wirehaus-0.3.0-linux-x86_64.tar.gz\n{h}  wirehaus-0.10.0-linux-x86_64.tar.gz\n{h}  wirehaus-0.99.0-windows-x64.zip\n",
            h = "ab".repeat(32)
        ))
        .unwrap();
        let suffix = "linux-x86_64.tar.gz";
        let pick = |current, skip| select_release(&sums, suffix, current, skip).map(|r| r.version);
        assert_eq!(pick(Version(0, 2, 1), None), Some(Version(0, 10, 0)));
        assert_eq!(pick(Version(0, 10, 0), None), None);
        assert_eq!(pick(Version(0, 11, 0), None), None);
        assert_eq!(
            pick(Version(0, 2, 1), Some(Version(0, 10, 0))),
            Some(Version(0, 3, 0))
        );
    }

    #[test]
    fn the_sums_parser_refuses_unsafe_names_and_bad_hashes() {
        let hash = "ab".repeat(32);
        assert!(parse_sums(&format!("{hash}  a.tar.gz\n")).is_ok());
        assert!(parse_sums(&format!("{hash} *a.tar.gz\n")).is_ok());
        for line in [
            format!("{hash}  ../a"),
            format!("{hash}  /etc/passwd"),
            format!("{hash}  a b"),
            format!("{}  a", "AB".repeat(32)),
            "short  a".to_owned(),
            hash.clone(),
        ] {
            assert!(parse_sums(&line).is_err(), "{line}");
        }
    }

    struct Fixed(Vec<(String, Vec<u8>)>);

    impl Fetch for Fixed {
        fn get(&self, url: &str, _max: u64) -> Result<Vec<u8>, UpdateError> {
            self.0
                .iter()
                .find(|(suffix, _)| url.ends_with(suffix.as_str()))
                .map(|(_, body)| body.clone())
                .ok_or_else(|| UpdateError::Download("missing".to_owned()))
        }
    }

    #[test]
    fn check_offers_nothing_for_a_tampered_list_even_if_a_newer_version_is_listed() {
        let signed = Fixed(vec![
            ("SHA256SUMS".to_owned(), SUMS.as_bytes().to_vec()),
            ("SHA256SUMS.sig".to_owned(), SUMS_SIG.as_bytes().to_vec()),
        ]);
        let trusted = [key(RELEASE_KEY)];
        let found = check(&signed, "https://x", Version(0, 1, 0), None, &trusted).unwrap();
        assert_eq!(
            found.map(|r| r.version),
            asset_suffix().map(|_| Version(0, 2, 1))
        );
        let forged = SUMS.replace("0.2.1", "9.9.9");
        let fake = Fixed(vec![
            ("SHA256SUMS".to_owned(), forged.into_bytes()),
            ("SHA256SUMS.sig".to_owned(), SUMS_SIG.as_bytes().to_vec()),
        ]);
        assert!(check(&fake, "https://x", Version(0, 1, 0), None, &trusted).is_err());
    }

    fn layout(name: &str) -> (PathBuf, Layout) {
        let base = std::env::temp_dir().join(format!(
            "wirehaus-update-{name}-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        ));
        let install = base.join("install");
        let state = base.join("state");
        fs::create_dir_all(install.join("viewer")).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::write(install.join("agent"), b"old").unwrap();
        fs::write(install.join("viewer/index.html"), b"old viewer").unwrap();
        let layout = Layout {
            install_dir: install,
            state_dir: state,
            exe_name: "agent".to_owned(),
        };
        (base, layout)
    }

    fn package(base: &Path) -> PathBuf {
        let root = base.join("pkg");
        fs::create_dir_all(root.join("viewer")).unwrap();
        fs::write(root.join("agent"), b"new").unwrap();
        fs::write(root.join("viewer/index.html"), b"new viewer").unwrap();
        let mut sums = String::new();
        for file in ["agent", "viewer/index.html"] {
            let hash = sha256_file(&root.join(file)).unwrap();
            writeln!(sums, "{hash}  {file}").unwrap();
        }
        fs::write(root.join("SHA256SUMS"), sums).unwrap();
        root
    }

    #[test]
    fn swap_rechecks_the_extracted_files_and_installs_nothing_if_they_changed() {
        let (base, layout) = layout("tampered");
        let root = package(&base);
        fs::write(root.join("agent"), b"evil").unwrap();
        assert!(matches!(
            swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)),
            Err(UpdateError::Checksum)
        ));
        assert_eq!(fs::read(layout.install_dir.join("agent")).unwrap(), b"old");
        assert!(!layout.install_dir.join("agent.prev").exists());
        assert_eq!(read_pending(&layout.state_dir), None);
        let root = package(&base);
        fs::write(root.join("viewer/index.html"), b"evil viewer").unwrap();
        assert!(matches!(
            swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)),
            Err(UpdateError::Checksum)
        ));
        assert_eq!(
            fs::read(layout.install_dir.join("viewer/index.html")).unwrap(),
            b"old viewer"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn swap_replaces_binary_and_viewer_keeps_the_previous_ones_and_opens_probation() {
        let (base, layout) = layout("swap");
        let root = package(&base);
        swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)).unwrap();
        assert_eq!(fs::read(layout.install_dir.join("agent")).unwrap(), b"new");
        assert_eq!(
            fs::read(layout.install_dir.join("agent.prev")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::read(layout.install_dir.join("viewer/index.html")).unwrap(),
            b"new viewer"
        );
        assert_eq!(
            fs::read(layout.install_dir.join("viewer.prev/index.html")).unwrap(),
            b"old viewer"
        );
        assert_eq!(
            read_pending(&layout.state_dir),
            Some(Pending {
                from: Version(0, 3, 0),
                to: Version(0, 3, 1),
                attempts: 0
            })
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(layout.install_dir.join("agent"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0o111
            );
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_new_version_that_keeps_failing_to_start_is_rolled_back_and_skipped() {
        let (base, layout) = layout("rollback");
        let root = package(&base);
        swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(begin_probation(&layout).unwrap(), Probation::Started);
        }
        assert_eq!(
            begin_probation(&layout).unwrap(),
            Probation::RolledBack(Version(0, 3, 1))
        );
        assert_eq!(fs::read(layout.install_dir.join("agent")).unwrap(), b"old");
        assert_eq!(
            fs::read(layout.install_dir.join("viewer/index.html")).unwrap(),
            b"old viewer"
        );
        assert_eq!(skipped(&layout.state_dir), Some(Version(0, 3, 1)));
        assert_eq!(read_pending(&layout.state_dir), None);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_healthy_start_clears_probation_and_there_is_nothing_to_roll_back_afterwards() {
        let (base, layout) = layout("healthy");
        let root = package(&base);
        swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)).unwrap();
        assert_eq!(begin_probation(&layout).unwrap(), Probation::Started);
        mark_healthy(&layout.state_dir);
        assert_eq!(begin_probation(&layout).unwrap(), Probation::None);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_failed_swap_leaves_the_installation_and_no_probation_behind() {
        let (base, layout) = layout("failswap");
        let root = base.join("empty-pkg");
        fs::create_dir_all(root.join("viewer")).unwrap();
        assert!(swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)).is_err());
        assert_eq!(fs::read(layout.install_dir.join("agent")).unwrap(), b"old");
        assert_eq!(
            fs::read(layout.install_dir.join("viewer/index.html")).unwrap(),
            b"old viewer"
        );
        assert_eq!(read_pending(&layout.state_dir), None);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_manual_rollback_discards_the_running_version_so_it_is_not_reinstalled() {
        let (base, layout) = layout("manual");
        let root = package(&base);
        swap(&layout, &root, Version(0, 3, 0), Version(0, 3, 1)).unwrap();
        mark_healthy(&layout.state_dir);
        assert_eq!(rollback(&layout).unwrap(), Some(Version::current()));
        assert_eq!(fs::read(layout.install_dir.join("agent")).unwrap(), b"old");
        assert_eq!(skipped(&layout.state_dir), Some(Version::current()));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_viewer_left_aside_by_an_interrupted_swap_is_put_back() {
        let (base, layout) = layout("recover");
        fs::rename(
            layout.install_dir.join("viewer"),
            layout.install_dir.join("viewer.prev"),
        )
        .unwrap();
        assert!(Layout::detect_in(&layout.install_dir, &layout.state_dir, "agent").is_ok());
        recover(&layout);
        assert!(layout.install_dir.join("viewer/index.html").is_file());
        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn the_smoke_test_refuses_binaries_that_do_not_run_or_report_another_version() {
        use std::os::unix::fs::PermissionsExt;
        let (base, _layout) = layout("smoke");
        let dir = base.join("candidate");
        fs::create_dir_all(&dir).unwrap();
        let script = |body: &str| {
            fs::write(dir.join("agent"), format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(dir.join("agent"), fs::Permissions::from_mode(0o755)).unwrap();
        };
        script("echo 'wirehaus-agent 0.3.1 (protocol 1)'");
        assert!(smoke_test(&dir, "agent", Version(0, 3, 1)).is_ok());
        assert!(smoke_test(&dir, "agent", Version(0, 3, 2)).is_err());
        script("exit 3");
        assert!(smoke_test(&dir, "agent", Version(0, 3, 1)).is_err());
        script("echo garbage");
        assert!(smoke_test(&dir, "agent", Version(0, 3, 1)).is_err());
        fs::write(dir.join("agent"), b"not a program").unwrap();
        assert!(smoke_test(&dir, "agent", Version(0, 3, 1)).is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_failed_check_is_retried_soon_and_backs_off_up_to_half_an_hour() {
        let interval = Duration::from_secs(3600);
        assert_eq!(retry_delay(1, interval), Duration::from_secs(30));
        assert_eq!(retry_delay(2, interval), Duration::from_secs(60));
        assert_eq!(retry_delay(4, interval), Duration::from_secs(240));
        assert_eq!(retry_delay(50, interval), Duration::from_secs(1800));
        assert_eq!(
            retry_delay(50, Duration::from_secs(600)),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn updates_can_be_disabled_and_enabled() {
        let (base, layout) = layout("toggle");
        assert!(enabled(&layout.state_dir));
        set_enabled(&layout.state_dir, false).unwrap();
        assert!(!enabled(&layout.state_dir));
        set_enabled(&layout.state_dir, true).unwrap();
        assert!(enabled(&layout.state_dir));
        let _ = fs::remove_dir_all(base);
    }
}
