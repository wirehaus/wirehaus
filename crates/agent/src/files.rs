use std::collections::HashMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use wirehaus_protocol::{
    Entry, EntryKind, FailureReason, FilesMessage, MAX_CHUNK, MAX_ENTRIES, MAX_NAME_LEN,
};

pub const MAX_TRANSFERS: usize = 4;
pub const MAX_UPLOAD_BYTES: u64 = 8 << 30;
pub const WINDOW_BYTES: u64 = 1 << 20;
const ACK_STEP: u64 = 256 * 1024;
const PART_SUFFIX: &str = ".part";
const PART_PREFIX: &str = ".wirehaus-";

#[derive(Debug)]
enum Transfer {
    Download {
        file: File,
        sent: u64,
        acked: u64,
    },
    Upload {
        file: File,
        part: PathBuf,
        target: PathBuf,
        size: u64,
        received: u64,
        acked: u64,
    },
}

#[derive(Debug, Default)]
pub struct Files {
    transfers: HashMap<u32, Transfer>,
}

type Reply = (u32, FilesMessage);

impl Files {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn active(&self) -> usize {
        self.transfers.len()
    }

    pub fn handle(&mut self, stream: u32, message: FilesMessage, roots: &[PathBuf]) -> Vec<Reply> {
        let roots = canonical_roots(roots);
        if roots.is_empty() && !matches!(message, FilesMessage::ListRoots | FilesMessage::Cancel) {
            self.abort_all();
            return vec![failed(stream, FailureReason::NotAllowed)];
        }
        let outcome = match message {
            FilesMessage::ListRoots => Ok(vec![(stream, FilesMessage::Roots(root_names(&roots)))]),
            FilesMessage::List { root, path } => list(&roots, root, &path, stream, &self.parts()),
            FilesMessage::Mkdir { root, path } => mkdir(&roots, root, &path, stream),
            FilesMessage::Remove { root, path } => {
                remove(&roots, root, &path, stream, &self.parts())
            }
            FilesMessage::Download { root, path } => self.download(&roots, root, &path, stream),
            FilesMessage::Upload { root, path, size } => {
                self.upload(&roots, root, &path, size, stream)
            }
            FilesMessage::Data(data) => self.receive(stream, &data),
            FilesMessage::Ack { bytes } => self.acknowledge(stream, bytes),
            FilesMessage::Done => self.finish_upload(stream),
            FilesMessage::Cancel => {
                self.abort(stream);
                Ok(Vec::new())
            }
            FilesMessage::Roots(_)
            | FilesMessage::Listing { .. }
            | FilesMessage::Ready { .. }
            | FilesMessage::Failed(_) => Err(FailureReason::InvalidPath),
        };
        outcome.unwrap_or_else(|reason| {
            self.abort(stream);
            vec![failed(stream, reason)]
        })
    }

    pub fn pump(&mut self, roots: &[PathBuf], budget: usize) -> Vec<Reply> {
        if self.transfers.is_empty() {
            return Vec::new();
        }
        if canonical_roots(roots).is_empty() {
            let streams: Vec<u32> = self.transfers.keys().copied().collect();
            self.abort_all();
            return streams
                .into_iter()
                .map(|stream| failed(stream, FailureReason::NotAllowed))
                .collect();
        }
        let mut out = Vec::new();
        let mut finished = Vec::new();
        let mut streams: Vec<u32> = self.transfers.keys().copied().collect();
        streams.sort_unstable();
        for stream in streams {
            let Some(Transfer::Download { file, sent, acked }) = self.transfers.get_mut(&stream)
            else {
                continue;
            };
            for _ in 0..budget {
                if *sent - *acked >= WINDOW_BYTES {
                    break;
                }
                match read_chunk(file) {
                    Ok(chunk) if chunk.is_empty() => {
                        out.push((stream, FilesMessage::Done));
                        finished.push(stream);
                        break;
                    }
                    Ok(chunk) => {
                        *sent += chunk.len() as u64;
                        out.push((stream, FilesMessage::Data(chunk)));
                    }
                    Err(_) => {
                        out.push(failed(stream, FailureReason::Io));
                        finished.push(stream);
                        break;
                    }
                }
            }
        }
        for stream in finished {
            self.transfers.remove(&stream);
        }
        out
    }

    fn download(
        &mut self,
        roots: &[PathBuf],
        root: u8,
        path: &str,
        stream: u32,
    ) -> Result<Vec<Reply>, FailureReason> {
        self.admit(stream)?;
        let real = existing(root_at(roots, root)?, path)?;
        if self.parts().contains(&real) {
            return Err(FailureReason::InvalidPath);
        }
        if !fs::metadata(&real)
            .map_err(|error| map_io(&error))?
            .is_file()
        {
            return Err(FailureReason::NotAFile);
        }
        let file = File::open(&real).map_err(|error| map_io(&error))?;
        let metadata = file.metadata().map_err(|error| map_io(&error))?;
        if !metadata.is_file() {
            return Err(FailureReason::NotAFile);
        }
        self.transfers.insert(
            stream,
            Transfer::Download {
                file,
                sent: 0,
                acked: 0,
            },
        );
        Ok(vec![(
            stream,
            FilesMessage::Ready {
                size: metadata.len(),
            },
        )])
    }

    fn upload(
        &mut self,
        roots: &[PathBuf],
        root: u8,
        path: &str,
        size: u64,
        stream: u32,
    ) -> Result<Vec<Reply>, FailureReason> {
        self.admit(stream)?;
        if size > MAX_UPLOAD_BYTES {
            return Err(FailureReason::TooLarge);
        }
        let (parent, name) = creatable(root_at(roots, root)?, path)?;
        let target = parent.join(&name);
        let part = parent.join(part_name(stream));
        let file = create_private_file(&part).map_err(|error| map_io(&error))?;
        self.transfers.insert(
            stream,
            Transfer::Upload {
                file,
                part,
                target,
                size,
                received: 0,
                acked: 0,
            },
        );
        Ok(vec![(stream, FilesMessage::Ready { size })])
    }

    fn parts(&self) -> Vec<PathBuf> {
        self.transfers
            .values()
            .filter_map(|transfer| match transfer {
                Transfer::Upload { part, .. } => Some(part.clone()),
                Transfer::Download { .. } => None,
            })
            .collect()
    }

    fn admit(&self, stream: u32) -> Result<(), FailureReason> {
        if self.transfers.contains_key(&stream) || self.transfers.len() >= MAX_TRANSFERS {
            return Err(FailureReason::Busy);
        }
        Ok(())
    }

    fn receive(&mut self, stream: u32, data: &[u8]) -> Result<Vec<Reply>, FailureReason> {
        let Some(Transfer::Upload {
            file,
            size,
            received,
            acked,
            ..
        }) = self.transfers.get_mut(&stream)
        else {
            return Err(FailureReason::InvalidPath);
        };
        let next = *received + data.len() as u64;
        if next > *size {
            return Err(FailureReason::TooLarge);
        }
        file.write_all(data).map_err(|error| map_io(&error))?;
        *received = next;
        if *received - *acked >= ACK_STEP {
            *acked = *received;
            return Ok(vec![(stream, FilesMessage::Ack { bytes: *received })]);
        }
        Ok(Vec::new())
    }

    fn acknowledge(&mut self, stream: u32, bytes: u64) -> Result<Vec<Reply>, FailureReason> {
        match self.transfers.get_mut(&stream) {
            Some(Transfer::Download { sent, acked, .. }) => {
                if bytes > *sent {
                    return Err(FailureReason::InvalidPath);
                }
                *acked = (*acked).max(bytes);
                Ok(Vec::new())
            }
            Some(Transfer::Upload { .. }) => Err(FailureReason::InvalidPath),
            None => Ok(Vec::new()),
        }
    }

    fn finish_upload(&mut self, stream: u32) -> Result<Vec<Reply>, FailureReason> {
        let Some(Transfer::Upload {
            mut file,
            part,
            target,
            size,
            received,
            ..
        }) = self.transfers.remove(&stream)
        else {
            return Err(FailureReason::InvalidPath);
        };
        let outcome = if received == size {
            file.flush()
                .and_then(|()| file.sync_all())
                .map_err(|error| map_io(&error))
                .and_then(|()| publish(&part, &target))
        } else {
            Err(FailureReason::Io)
        };
        drop(file);
        if outcome.is_err() {
            let _ = fs::remove_file(&part);
        }
        outcome.map(|()| vec![(stream, FilesMessage::Done)])
    }

    fn abort(&mut self, stream: u32) {
        if let Some(Transfer::Upload { part, .. }) = self.transfers.remove(&stream) {
            let _ = fs::remove_file(part);
        }
    }

    fn abort_all(&mut self) {
        let streams: Vec<u32> = self.transfers.keys().copied().collect();
        for stream in streams {
            self.abort(stream);
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        self.abort_all();
    }
}

fn failed(stream: u32, reason: FailureReason) -> Reply {
    (stream, FilesMessage::Failed(reason))
}

fn read_chunk(file: &mut File) -> std::io::Result<Vec<u8>> {
    let mut buffer = vec![0_u8; MAX_CHUNK];
    let mut filled = 0;
    while filled < buffer.len() {
        let count = file.read(&mut buffer[filled..])?;
        if count == 0 {
            break;
        }
        filled += count;
    }
    buffer.truncate(filled);
    Ok(buffer)
}

fn publish(part: &Path, target: &Path) -> Result<(), FailureReason> {
    match fs::hard_link(part, target) {
        Ok(()) => fs::remove_file(part).map_err(|error| map_io(&error)),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(FailureReason::AlreadyExists),
        Err(_) => {
            if fs::symlink_metadata(target).is_ok() {
                return Err(FailureReason::AlreadyExists);
            }
            fs::rename(part, target).map_err(|error| map_io(&error))
        }
    }
}

fn create_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn list(
    roots: &[PathBuf],
    root: u8,
    path: &str,
    stream: u32,
    busy: &[PathBuf],
) -> Result<Vec<Reply>, FailureReason> {
    let directory = existing(root_at(roots, root)?, path)?;
    if !directory.is_dir() {
        return Err(FailureReason::NotADirectory);
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in fs::read_dir(&directory).map_err(|error| map_io(&error))? {
        let Ok(item) = item else { continue };
        if busy.contains(&item.path()) {
            continue;
        }
        let Some(entry) = describe(&item) else {
            continue;
        };
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(entry);
    }
    entries.sort_by(|a, b| {
        let rank = |entry: &Entry| u8::from(entry.kind != EntryKind::Directory);
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(vec![(stream, FilesMessage::Listing { entries, truncated })])
}

fn describe(item: &fs::DirEntry) -> Option<Entry> {
    let name = item.file_name().into_string().ok()?;
    if name.len() > MAX_NAME_LEN {
        return None;
    }
    let file_type = item.file_type().ok()?;
    let metadata = item.metadata().ok()?;
    let kind = if file_type.is_symlink() {
        EntryKind::Other
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs());
    let size = if kind == EntryKind::File {
        metadata.len()
    } else {
        0
    };
    Some(Entry {
        name,
        kind,
        size,
        modified,
    })
}

fn part_name(stream: u32) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{PART_PREFIX}{stream}-{nanos:x}{PART_SUFFIX}")
}

fn mkdir(
    roots: &[PathBuf],
    root: u8,
    path: &str,
    stream: u32,
) -> Result<Vec<Reply>, FailureReason> {
    let (parent, name) = creatable(root_at(roots, root)?, path)?;
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(parent.join(name))
        .map_err(|error| map_io(&error))?;
    Ok(vec![(stream, FilesMessage::Done)])
}

fn remove(
    roots: &[PathBuf],
    root: u8,
    path: &str,
    stream: u32,
    busy: &[PathBuf],
) -> Result<Vec<Reply>, FailureReason> {
    let base = root_at(roots, root)?;
    let (parent, name) = creatable_parent(base, path)?;
    let target = parent.join(name);
    if busy.contains(&target) {
        return Err(FailureReason::InvalidPath);
    }
    let metadata = fs::symlink_metadata(&target).map_err(|error| map_io(&error))?;
    if metadata.is_dir() {
        fs::remove_dir(&target).map_err(|error| {
            if error.kind() == ErrorKind::DirectoryNotEmpty {
                FailureReason::NotEmpty
            } else {
                map_io(&error)
            }
        })?;
    } else {
        fs::remove_file(&target).map_err(|error| map_io(&error))?;
    }
    Ok(vec![(stream, FilesMessage::Done)])
}

fn canonical_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()))
        .collect()
}

fn root_names(roots: &[PathBuf]) -> Vec<String> {
    roots
        .iter()
        .map(|root| {
            root.file_name()
                .map_or_else(|| root.to_string_lossy(), |name| name.to_string_lossy())
                .into_owned()
        })
        .collect()
}

fn root_at(roots: &[PathBuf], index: u8) -> Result<&Path, FailureReason> {
    roots
        .get(usize::from(index))
        .map(PathBuf::as_path)
        .ok_or(FailureReason::NoSuchRoot)
}

fn components(relative: &str) -> Result<Vec<&str>, FailureReason> {
    if relative.is_empty() {
        return Ok(Vec::new());
    }
    relative.split('/').map(valid_component).collect()
}

const DEVICE_NAMES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
const WINDOWS_INVALID: [char; 5] = ['<', '>', '"', '|', '?'];

fn is_reserved(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).trim_end_matches(' ');
    let upper = stem.to_uppercase();
    if DEVICE_NAMES.contains(&upper.as_str()) {
        return true;
    }
    let mut chars = upper.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(
            chars.next(),
            Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}')
        )
        && chars.next().is_none()
}

fn valid_component(part: &str) -> Result<&str, FailureReason> {
    let forbidden = |c: char| c.is_control() || matches!(c, '\\' | ':' | '/');
    let windows_only = cfg!(windows)
        && (part.ends_with('.')
            || part.ends_with(' ')
            || part.contains(|c: char| c == '*' || WINDOWS_INVALID.contains(&c))
            || is_reserved(part));
    if part.is_empty()
        || part == "."
        || part == ".."
        || part.len() > MAX_NAME_LEN
        || part.contains(forbidden)
        || windows_only
    {
        return Err(FailureReason::InvalidPath);
    }
    Ok(part)
}

fn existing(root: &Path, relative: &str) -> Result<PathBuf, FailureReason> {
    let joined = components(relative)?
        .into_iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    let real = joined.canonicalize().map_err(|error| map_io(&error))?;
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err(FailureReason::InvalidPath)
    }
}

fn creatable_parent(root: &Path, relative: &str) -> Result<(PathBuf, String), FailureReason> {
    let mut parts = components(relative)?;
    let name = parts.pop().ok_or(FailureReason::InvalidPath)?.to_owned();
    let parent = existing(root, &parts.join("/"))?;
    if !parent.is_dir() {
        return Err(FailureReason::NotADirectory);
    }
    Ok((parent, name))
}

fn creatable(root: &Path, relative: &str) -> Result<(PathBuf, String), FailureReason> {
    let (parent, name) = creatable_parent(root, relative)?;
    if fs::symlink_metadata(parent.join(&name)).is_ok() {
        return Err(FailureReason::AlreadyExists);
    }
    Ok((parent, name))
}

fn map_io(error: &std::io::Error) -> FailureReason {
    match error.kind() {
        ErrorKind::NotFound => FailureReason::NotFound,
        ErrorKind::AlreadyExists => FailureReason::AlreadyExists,
        ErrorKind::NotADirectory => FailureReason::NotADirectory,
        ErrorKind::IsADirectory => FailureReason::NotAFile,
        ErrorKind::DirectoryNotEmpty => FailureReason::NotEmpty,
        ErrorKind::PermissionDenied => FailureReason::NotAllowed,
        _ => FailureReason::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symlink(original: impl AsRef<Path>, link: impl AsRef<Path>) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(original, link)
        }
        #[cfg(windows)]
        {
            if original.as_ref().is_dir() {
                std::os::windows::fs::symlink_dir(original, link)
            } else {
                std::os::windows::fs::symlink_file(original, link)
            }
        }
    }

    struct Sandbox {
        base: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "wirehaus-files-{name}-{}-{}",
                std::process::id(),
                UNIX_EPOCH.elapsed().unwrap().as_nanos()
            ));
            fs::create_dir_all(base.join("root/sub")).unwrap();
            fs::create_dir_all(base.join("outside")).unwrap();
            fs::write(base.join("root/a.txt"), b"hello").unwrap();
            fs::write(base.join("root/sub/b.txt"), b"nested").unwrap();
            fs::write(base.join("outside/secret.txt"), b"secret").unwrap();
            Self { base }
        }

        fn roots(&self) -> Vec<PathBuf> {
            vec![self.base.join("root")]
        }

        fn root(&self) -> PathBuf {
            self.base.join("root")
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn only(replies: Vec<Reply>) -> FilesMessage {
        assert_eq!(replies.len(), 1, "{replies:?}");
        replies.into_iter().next().unwrap().1
    }

    fn path_message(make: fn(u8, String) -> FilesMessage, path: &str) -> FilesMessage {
        make(0, path.to_owned())
    }

    fn list_message(root: u8, path: String) -> FilesMessage {
        FilesMessage::List { root, path }
    }

    fn download_message(root: u8, path: String) -> FilesMessage {
        FilesMessage::Download { root, path }
    }

    fn mkdir_message(root: u8, path: String) -> FilesMessage {
        FilesMessage::Mkdir { root, path }
    }

    fn remove_message(root: u8, path: String) -> FilesMessage {
        FilesMessage::Remove { root, path }
    }

    fn names(message: FilesMessage) -> Vec<String> {
        match message {
            FilesMessage::Listing { entries, .. } => {
                entries.into_iter().map(|entry| entry.name).collect()
            }
            other => panic!("{other:?}"),
        }
    }

    fn download_all(files: &mut Files, roots: &[PathBuf], stream: u32) -> Vec<u8> {
        let mut data = Vec::new();
        for _ in 0..10_000 {
            for (_, message) in files.pump(roots, 4) {
                match message {
                    FilesMessage::Data(chunk) => data.extend_from_slice(&chunk),
                    FilesMessage::Done => return data,
                    other => panic!("{other:?}"),
                }
            }
            let ack = FilesMessage::Ack {
                bytes: data.len() as u64,
            };
            files.handle(stream, ack, roots);
        }
        panic!("download did not finish");
    }

    #[test]
    fn lists_roots_and_directories_with_folders_first() {
        let sandbox = Sandbox::new("list");
        let mut files = Files::new();
        let roots = sandbox.roots();
        let reply = only(files.handle(1, FilesMessage::ListRoots, &roots));
        assert_eq!(reply, FilesMessage::Roots(vec!["root".to_owned()]));
        let listing = only(files.handle(1, path_message(list_message, ""), &roots));
        assert_eq!(names(listing), ["sub", "a.txt"]);
        let nested = only(files.handle(1, path_message(list_message, "sub"), &roots));
        assert_eq!(names(nested), ["b.txt"]);
    }

    #[test]
    fn nothing_works_without_roots() {
        let mut files = Files::new();
        for message in [
            path_message(list_message, ""),
            path_message(download_message, "a"),
            path_message(mkdir_message, "a"),
            path_message(remove_message, "a"),
        ] {
            assert_eq!(
                only(files.handle(1, message, &[])),
                FilesMessage::Failed(FailureReason::NotAllowed)
            );
        }
        assert_eq!(
            only(files.handle(1, FilesMessage::ListRoots, &[])),
            FilesMessage::Roots(Vec::new())
        );
    }

    #[test]
    fn paths_that_try_to_leave_the_root_are_refused() {
        let sandbox = Sandbox::new("escape");
        let mut files = Files::new();
        let roots = sandbox.roots();
        let outside = sandbox.base.join("outside/secret.txt");
        for path in [
            "../outside/secret.txt",
            "sub/../../outside/secret.txt",
            "sub/../a.txt",
            "/etc/passwd",
            outside.to_str().unwrap(),
            "sub//b.txt",
            "./a.txt",
            "a.txt/",
            "..",
            "sub\\..\\a.txt",
            "a\0b",
            "c:\\windows",
            "C:",
        ] {
            let reply = only(files.handle(1, path_message(download_message, path), &roots));
            assert_eq!(
                reply,
                FilesMessage::Failed(FailureReason::InvalidPath),
                "{path:?}"
            );
            assert_eq!(files.active(), 0);
        }
    }

    #[test]
    fn symbolic_links_that_leave_the_root_are_refused() {
        let sandbox = Sandbox::new("symlink");
        symlink(sandbox.base.join("outside"), sandbox.root().join("link")).unwrap();
        symlink(
            sandbox.base.join("outside/secret.txt"),
            sandbox.root().join("secret-link"),
        )
        .unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        for message in [
            path_message(list_message, "link"),
            path_message(download_message, "link/secret.txt"),
            path_message(download_message, "secret-link"),
            path_message(mkdir_message, "link/new"),
        ] {
            assert_eq!(
                only(files.handle(1, message.clone(), &roots)),
                FilesMessage::Failed(FailureReason::InvalidPath),
                "{message:?}"
            );
        }
        assert!(!sandbox.base.join("outside/new").exists());
        let upload = FilesMessage::Upload {
            root: 0,
            path: "link/up.txt".to_owned(),
            size: 1,
        };
        assert_eq!(
            only(files.handle(1, upload, &roots)),
            FilesMessage::Failed(FailureReason::InvalidPath)
        );
        let listing = only(files.handle(1, path_message(list_message, ""), &roots));
        let FilesMessage::Listing { entries, .. } = listing else {
            panic!()
        };
        assert!(
            entries
                .iter()
                .filter(|entry| entry.name.contains("link"))
                .all(|entry| entry.kind == EntryKind::Other)
        );
    }

    #[test]
    fn symbolic_links_inside_the_root_are_followed() {
        let sandbox = Sandbox::new("inside");
        symlink(sandbox.root().join("sub"), sandbox.root().join("alias")).unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        let listing = only(files.handle(1, path_message(list_message, "alias"), &roots));
        assert_eq!(names(listing), ["b.txt"]);
    }

    #[test]
    fn a_root_that_is_a_link_is_confined_to_its_target() {
        let sandbox = Sandbox::new("rootlink");
        let link = sandbox.base.join("rootlink");
        symlink(sandbox.root(), &link).unwrap();
        let mut files = Files::new();
        let roots = vec![link];
        let listing = only(files.handle(1, path_message(list_message, ""), &roots));
        assert_eq!(names(listing), ["sub", "a.txt"]);
        assert_eq!(
            only(files.handle(
                1,
                path_message(download_message, "../outside/secret.txt"),
                &roots
            )),
            FilesMessage::Failed(FailureReason::InvalidPath)
        );
    }

    #[test]
    fn unknown_roots_are_refused() {
        let sandbox = Sandbox::new("noroot");
        let mut files = Files::new();
        let reply = only(files.handle(
            1,
            FilesMessage::List {
                root: 5,
                path: String::new(),
            },
            &sandbox.roots(),
        ));
        assert_eq!(reply, FilesMessage::Failed(FailureReason::NoSuchRoot));
    }

    #[test]
    fn a_file_downloads_in_chunks_and_respects_the_window() {
        let sandbox = Sandbox::new("download");
        let big: Vec<u8> = (0..3_000_000_u32).map(|n| (n % 251) as u8).collect();
        fs::write(sandbox.root().join("big.bin"), &big).unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        let ready = only(files.handle(3, path_message(download_message, "big.bin"), &roots));
        assert_eq!(
            ready,
            FilesMessage::Ready {
                size: big.len() as u64
            }
        );
        let mut sent = 0_u64;
        for _ in 0..100 {
            for (_, message) in files.pump(&roots, 1000) {
                if let FilesMessage::Data(chunk) = message {
                    sent += chunk.len() as u64;
                }
            }
        }
        assert_eq!(sent, WINDOW_BYTES);
        let mut files = Files::new();
        files.handle(3, path_message(download_message, "big.bin"), &roots);
        assert_eq!(download_all(&mut files, &roots, 3), big);
        assert_eq!(files.active(), 0);
    }

    #[test]
    fn an_empty_file_downloads_as_just_done() {
        let sandbox = Sandbox::new("empty");
        fs::write(sandbox.root().join("empty"), b"").unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(1, path_message(download_message, "empty"), &roots);
        assert_eq!(files.pump(&roots, 4), [(1, FilesMessage::Done)]);
        assert_eq!(files.active(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn downloading_a_named_pipe_fails_instead_of_blocking() {
        let sandbox = Sandbox::new("fifo");
        let pipe = sandbox.root().join("pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&pipe)
                .status()
                .unwrap()
                .success()
        );
        let mut files = Files::new();
        let roots = sandbox.roots();
        assert_eq!(
            only(files.handle(1, path_message(download_message, "pipe"), &roots)),
            FilesMessage::Failed(FailureReason::NotAFile)
        );
        assert_eq!(files.active(), 0);
    }

    #[test]
    fn downloading_a_directory_or_a_missing_file_fails() {
        let sandbox = Sandbox::new("badtarget");
        let mut files = Files::new();
        let roots = sandbox.roots();
        assert_eq!(
            only(files.handle(1, path_message(download_message, "sub"), &roots)),
            FilesMessage::Failed(FailureReason::NotAFile)
        );
        assert_eq!(
            only(files.handle(1, path_message(download_message, "nope"), &roots)),
            FilesMessage::Failed(FailureReason::NotFound)
        );
    }

    #[test]
    fn a_late_acknowledgement_after_the_transfer_ended_is_ignored() {
        let sandbox = Sandbox::new("lateack");
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(1, path_message(download_message, "a.txt"), &roots);
        assert_eq!(files.pump(&roots, 4).len(), 2);
        assert_eq!(files.active(), 0);
        assert_eq!(
            files
                .handle(1, FilesMessage::Ack { bytes: 5 }, &roots)
                .len(),
            0
        );
    }

    #[test]
    fn an_acknowledgement_beyond_what_was_sent_aborts_the_transfer() {
        let sandbox = Sandbox::new("badack");
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(1, path_message(download_message, "a.txt"), &roots);
        let reply = only(files.handle(1, FilesMessage::Ack { bytes: 999 }, &roots));
        assert_eq!(reply, FilesMessage::Failed(FailureReason::InvalidPath));
        assert_eq!(files.active(), 0);
    }

    fn upload(files: &mut Files, roots: &[PathBuf], path: &str, content: &[u8]) -> FilesMessage {
        let start = FilesMessage::Upload {
            root: 0,
            path: path.to_owned(),
            size: content.len() as u64,
        };
        let ready = only(files.handle(9, start, roots));
        if !matches!(ready, FilesMessage::Ready { .. }) {
            return ready;
        }
        for chunk in content.chunks(MAX_CHUNK) {
            for (_, reply) in files.handle(9, FilesMessage::Data(chunk.to_vec()), roots) {
                assert!(matches!(reply, FilesMessage::Ack { .. }), "{reply:?}");
            }
        }
        only(files.handle(9, FilesMessage::Done, roots))
    }

    #[test]
    fn an_upload_appears_only_when_it_completes() {
        let sandbox = Sandbox::new("upload");
        let mut files = Files::new();
        let roots = sandbox.roots();
        let content: Vec<u8> = (0..700_000_u32).map(|n| (n % 7) as u8).collect();
        let start = FilesMessage::Upload {
            root: 0,
            path: "sub/new.bin".to_owned(),
            size: content.len() as u64,
        };
        files.handle(9, start, &roots);
        files.handle(9, FilesMessage::Data(content[..1000].to_vec()), &roots);
        assert!(!sandbox.root().join("sub/new.bin").exists());
        let listing = only(files.handle(1, path_message(list_message, "sub"), &roots));
        assert_eq!(names(listing), ["b.txt"]);
        drop(files);
        let leftovers: Vec<_> = fs::read_dir(sandbox.root().join("sub")).unwrap().collect();
        assert_eq!(leftovers.len(), 1);

        let mut files = Files::new();
        let reply = upload(&mut files, &roots, "sub/new.bin", &content);
        assert_eq!(reply, FilesMessage::Done);
        assert_eq!(
            fs::read(sandbox.root().join("sub/new.bin")).unwrap(),
            content
        );
        assert_eq!(files.active(), 0);
        let leftovers = fs::read_dir(sandbox.root().join("sub")).unwrap().count();
        assert_eq!(leftovers, 2);
    }

    #[test]
    fn uploads_never_overwrite_and_files_are_private() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let sandbox = Sandbox::new("noclobber");
        let mut files = Files::new();
        let roots = sandbox.roots();
        assert_eq!(
            upload(&mut files, &roots, "a.txt", b"x"),
            FilesMessage::Failed(FailureReason::AlreadyExists)
        );
        assert_eq!(fs::read(sandbox.root().join("a.txt")).unwrap(), b"hello");
        assert_eq!(
            upload(&mut files, &roots, "fresh.txt", b"x"),
            FilesMessage::Done
        );
        #[cfg(unix)]
        {
            let mode = fs::metadata(sandbox.root().join("fresh.txt"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
    }

    #[test]
    fn an_upload_that_sends_more_or_less_than_declared_is_discarded() {
        let sandbox = Sandbox::new("size");
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(
            2,
            FilesMessage::Upload {
                root: 0,
                path: "more".to_owned(),
                size: 3,
            },
            &roots,
        );
        let reply = only(files.handle(2, FilesMessage::Data(vec![1, 2, 3, 4]), &roots));
        assert_eq!(reply, FilesMessage::Failed(FailureReason::TooLarge));
        files.handle(
            2,
            FilesMessage::Upload {
                root: 0,
                path: "less".to_owned(),
                size: 10,
            },
            &roots,
        );
        files.handle(2, FilesMessage::Data(vec![1, 2, 3]), &roots);
        let reply = only(files.handle(2, FilesMessage::Done, &roots));
        assert_eq!(reply, FilesMessage::Failed(FailureReason::Io));
        assert_eq!(files.active(), 0);
        let found: Vec<_> = fs::read_dir(sandbox.root()).unwrap().collect();
        assert_eq!(found.len(), 2);
        assert!(!sandbox.root().join("more").exists());
        assert!(!sandbox.root().join("less").exists());
    }

    #[test]
    fn an_oversized_upload_is_refused_before_creating_anything() {
        let sandbox = Sandbox::new("huge");
        let mut files = Files::new();
        let roots = sandbox.roots();
        let reply = only(files.handle(
            1,
            FilesMessage::Upload {
                root: 0,
                path: "huge".to_owned(),
                size: MAX_UPLOAD_BYTES + 1,
            },
            &roots,
        ));
        assert_eq!(reply, FilesMessage::Failed(FailureReason::TooLarge));
        assert_eq!(fs::read_dir(sandbox.root()).unwrap().count(), 2);
    }

    #[test]
    fn cancelling_an_upload_removes_its_partial_file() {
        let sandbox = Sandbox::new("cancel");
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(
            4,
            FilesMessage::Upload {
                root: 0,
                path: "x".to_owned(),
                size: 10,
            },
            &roots,
        );
        assert_eq!(fs::read_dir(sandbox.root()).unwrap().count(), 3);
        assert_eq!(files.handle(4, FilesMessage::Cancel, &roots).len(), 0);
        assert_eq!(fs::read_dir(sandbox.root()).unwrap().count(), 2);
    }

    #[test]
    fn the_partial_file_of_a_running_upload_is_hidden_and_cannot_be_addressed() {
        let sandbox = Sandbox::new("part");
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(
            4,
            FilesMessage::Upload {
                root: 0,
                path: "x".to_owned(),
                size: 10,
            },
            &roots,
        );
        let listing = only(files.handle(1, path_message(list_message, ""), &roots));
        assert_eq!(names(listing), ["sub", "a.txt"]);
        let part = fs::read_dir(sandbox.root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .find(|name| name.ends_with(PART_SUFFIX))
            .unwrap();
        for message in [
            path_message(download_message, &part),
            path_message(remove_message, &part),
        ] {
            assert_eq!(
                only(files.handle(2, message, &roots)),
                FilesMessage::Failed(FailureReason::InvalidPath)
            );
        }
        assert!(sandbox.root().join(&part).exists());
        files.handle(4, FilesMessage::Cancel, &roots);
        assert!(!sandbox.root().join(&part).exists());
    }

    #[test]
    fn a_partial_file_left_by_a_crash_does_not_block_new_uploads() {
        let sandbox = Sandbox::new("stale");
        fs::write(sandbox.root().join(".wirehaus-9.part"), b"left over").unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        assert_eq!(
            upload(&mut files, &roots, "fresh.txt", b"data"),
            FilesMessage::Done
        );
        assert_eq!(fs::read(sandbox.root().join("fresh.txt")).unwrap(), b"data");
    }

    #[test]
    fn a_stale_partial_file_is_listed_and_can_be_removed_by_the_owner() {
        let sandbox = Sandbox::new("stale-visible");
        fs::write(sandbox.root().join(".wirehaus-4.part"), b"left over").unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        let listing = only(files.handle(1, path_message(list_message, ""), &roots));
        assert!(names(listing).contains(&".wirehaus-4.part".to_owned()));
        assert_eq!(
            only(files.handle(1, path_message(remove_message, ".wirehaus-4.part"), &roots)),
            FilesMessage::Done
        );
        assert!(!sandbox.root().join(".wirehaus-4.part").exists());
    }

    #[test]
    fn at_most_four_transfers_and_unique_streams() {
        let sandbox = Sandbox::new("busy");
        let mut files = Files::new();
        let roots = sandbox.roots();
        for stream in 1..=4 {
            files.handle(stream, path_message(download_message, "a.txt"), &roots);
        }
        for stream in [5, 1] {
            assert_eq!(
                only(files.handle(stream, path_message(download_message, "a.txt"), &roots)),
                FilesMessage::Failed(FailureReason::Busy)
            );
        }
        assert_eq!(files.active(), 3);
    }

    #[test]
    fn folders_can_be_made_and_removed_but_not_the_root() {
        let sandbox = Sandbox::new("mkdir");
        let mut files = Files::new();
        let roots = sandbox.roots();
        assert_eq!(
            only(files.handle(1, path_message(mkdir_message, "sub/inner"), &roots)),
            FilesMessage::Done
        );
        assert!(sandbox.root().join("sub/inner").is_dir());
        assert_eq!(
            only(files.handle(1, path_message(mkdir_message, "sub/inner"), &roots)),
            FilesMessage::Failed(FailureReason::AlreadyExists)
        );
        assert_eq!(
            only(files.handle(1, path_message(mkdir_message, "missing/inner"), &roots)),
            FilesMessage::Failed(FailureReason::NotFound)
        );
        assert_eq!(
            only(files.handle(1, path_message(remove_message, "sub"), &roots)),
            FilesMessage::Failed(FailureReason::NotEmpty)
        );
        assert_eq!(
            only(files.handle(1, path_message(remove_message, "sub/inner"), &roots)),
            FilesMessage::Done
        );
        assert_eq!(
            only(files.handle(1, path_message(remove_message, "a.txt"), &roots)),
            FilesMessage::Done
        );
        assert!(!sandbox.root().join("a.txt").exists());
        assert_eq!(
            only(files.handle(1, path_message(remove_message, ""), &roots)),
            FilesMessage::Failed(FailureReason::InvalidPath)
        );
        assert!(sandbox.root().is_dir());
    }

    #[test]
    fn removing_a_link_removes_the_link_and_not_its_target() {
        let sandbox = Sandbox::new("rmlink");
        symlink(
            sandbox.base.join("outside/secret.txt"),
            sandbox.root().join("secret-link"),
        )
        .unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        assert_eq!(
            only(files.handle(1, path_message(remove_message, "secret-link"), &roots)),
            FilesMessage::Done
        );
        assert!(!sandbox.root().join("secret-link").exists());
        assert!(sandbox.base.join("outside/secret.txt").exists());
    }

    #[test]
    fn losing_every_root_stops_running_transfers() {
        let sandbox = Sandbox::new("revoked");
        let big = vec![7_u8; 500_000];
        fs::write(sandbox.root().join("big"), &big).unwrap();
        let mut files = Files::new();
        let roots = sandbox.roots();
        files.handle(1, path_message(download_message, "big"), &roots);
        files.handle(
            2,
            FilesMessage::Upload {
                root: 0,
                path: "up".to_owned(),
                size: 10,
            },
            &roots,
        );
        let replies = files.pump(&[], 4);
        assert_eq!(replies.len(), 2);
        assert!(
            replies
                .iter()
                .all(|(_, message)| *message == FilesMessage::Failed(FailureReason::NotAllowed))
        );
        assert_eq!(files.active(), 0);
        assert_eq!(fs::read_dir(sandbox.root()).unwrap().count(), 3);
    }

    #[test]
    fn messages_only_the_agent_sends_are_refused() {
        let sandbox = Sandbox::new("direction");
        let mut files = Files::new();
        let roots = sandbox.roots();
        for message in [
            FilesMessage::Roots(Vec::new()),
            FilesMessage::Ready { size: 1 },
            FilesMessage::Failed(FailureReason::Io),
        ] {
            assert!(matches!(
                only(files.handle(1, message, &roots)),
                FilesMessage::Failed(_)
            ));
        }
    }

    #[test]
    fn reserved_device_names_are_recognised_with_any_case_and_extension() {
        for name in [
            "CON",
            "con",
            "Nul",
            "aux.txt",
            "PRN",
            "COM1",
            "lpt9.log",
            "CONIN$",
            "CONOUT$",
            "com3.tar.gz",
            "COM\u{b9}",
            "nul .txt",
        ] {
            assert!(is_reserved(name), "{name}");
        }
        for name in [
            "console",
            "com10",
            "comx",
            "con2",
            "a.con",
            "LPT0x",
            "report.txt",
            "NUL1",
        ] {
            assert!(!is_reserved(name), "{name}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_refuses_device_names_and_invalid_characters_everywhere() {
        let sandbox = Sandbox::new("devices");
        let mut files = Files::new();
        let roots = sandbox.roots();
        for name in [
            "NUL", "con.txt", "COM1", "a<b", "a|b", "q?", "star*", "quote\"x",
        ] {
            assert_eq!(
                upload(&mut files, &roots, name, b"x"),
                FilesMessage::Failed(FailureReason::InvalidPath),
                "{name}"
            );
            assert_eq!(
                only(files.handle(1, path_message(mkdir_message, name), &roots)),
                FilesMessage::Failed(FailureReason::InvalidPath),
                "{name}"
            );
            assert_eq!(
                only(files.handle(1, path_message(download_message, name), &roots)),
                FilesMessage::Failed(FailureReason::InvalidPath),
                "{name}"
            );
        }
        assert!(!sandbox.root().join("sub").join("NUL").exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_refuses_drive_letters_streams_and_backslashes() {
        let sandbox = Sandbox::new("drives");
        let mut files = Files::new();
        let roots = sandbox.roots();
        for path in [
            "C:\\Windows",
            "C:Windows",
            "a.txt:stream",
            "..\\outside",
            "sub\\..\\..\\outside",
            "\\\\server\\share",
            "//server/share",
            "a.txt.",
            "a.txt ",
        ] {
            assert_eq!(
                only(files.handle(1, path_message(download_message, path), &roots)),
                FilesMessage::Failed(FailureReason::InvalidPath),
                "{path}"
            );
        }
    }
}
