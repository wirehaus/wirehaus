use crate::error::ProtocolError;

pub const MAX_PATH_LEN: usize = 4096;
pub const MAX_NAME_LEN: usize = 255;
pub const MAX_CHUNK: usize = 32 * 1024;
pub const MAX_ENTRIES: usize = 1000;
pub const MAX_ROOTS: usize = 16;
pub const FEATURE_RENAME: u8 = 1;
pub const FEATURES: u8 = FEATURE_RENAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Other,
}

impl EntryKind {
    fn tag(self) -> u8 {
        match self {
            EntryKind::File => 0,
            EntryKind::Directory => 1,
            EntryKind::Other => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(EntryKind::File),
            1 => Ok(EntryKind::Directory),
            2 => Ok(EntryKind::Other),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    NotAllowed,
    NoSuchRoot,
    InvalidPath,
    NotFound,
    AlreadyExists,
    NotADirectory,
    NotAFile,
    NotEmpty,
    TooLarge,
    Busy,
    Io,
}

impl FailureReason {
    fn tag(self) -> u8 {
        match self {
            FailureReason::NotAllowed => 0,
            FailureReason::NoSuchRoot => 1,
            FailureReason::InvalidPath => 2,
            FailureReason::NotFound => 3,
            FailureReason::AlreadyExists => 4,
            FailureReason::NotADirectory => 5,
            FailureReason::NotAFile => 6,
            FailureReason::NotEmpty => 7,
            FailureReason::TooLarge => 8,
            FailureReason::Busy => 9,
            FailureReason::Io => 10,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(FailureReason::NotAllowed),
            1 => Ok(FailureReason::NoSuchRoot),
            2 => Ok(FailureReason::InvalidPath),
            3 => Ok(FailureReason::NotFound),
            4 => Ok(FailureReason::AlreadyExists),
            5 => Ok(FailureReason::NotADirectory),
            6 => Ok(FailureReason::NotAFile),
            7 => Ok(FailureReason::NotEmpty),
            8 => Ok(FailureReason::TooLarge),
            9 => Ok(FailureReason::Busy),
            10 => Ok(FailureReason::Io),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesMessage {
    ListRoots,
    List {
        root: u8,
        path: String,
    },
    Download {
        root: u8,
        path: String,
    },
    Upload {
        root: u8,
        path: String,
        size: u64,
    },
    Mkdir {
        root: u8,
        path: String,
    },
    Remove {
        root: u8,
        path: String,
    },
    Rename {
        root: u8,
        from: String,
        to: String,
        replace: bool,
    },
    Cancel,
    Roots(Vec<String>),
    Listing {
        entries: Vec<Entry>,
        truncated: bool,
    },
    Ready {
        size: u64,
    },
    Data(Vec<u8>),
    Done,
    Failed(FailureReason),
    Ack {
        bytes: u64,
    },
}

const LIST_ROOTS: u8 = 0;
const LIST: u8 = 1;
const DOWNLOAD: u8 = 2;
const UPLOAD: u8 = 3;
const MKDIR: u8 = 4;
const REMOVE: u8 = 5;
const CANCEL: u8 = 6;
const ROOTS: u8 = 7;
const LISTING: u8 = 8;
const READY: u8 = 9;
const DATA: u8 = 10;
const DONE: u8 = 11;
const FAILED: u8 = 12;
const ACK: u8 = 13;
const RENAME: u8 = 14;

impl FilesMessage {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            FilesMessage::ListRoots => vec![LIST_ROOTS],
            FilesMessage::List { root, path } => path_message(LIST, *root, path),
            FilesMessage::Download { root, path } => path_message(DOWNLOAD, *root, path),
            FilesMessage::Mkdir { root, path } => path_message(MKDIR, *root, path),
            FilesMessage::Remove { root, path } => path_message(REMOVE, *root, path),
            FilesMessage::Upload { root, path, size } => {
                let mut out = path_message(UPLOAD, *root, path);
                out.extend_from_slice(&size.to_be_bytes());
                out
            }
            FilesMessage::Rename {
                root,
                from,
                to,
                replace,
            } => {
                let mut out = path_message(RENAME, *root, from);
                put_text(&mut out, to);
                out.push(u8::from(*replace));
                out
            }
            FilesMessage::Cancel => vec![CANCEL],
            FilesMessage::Roots(names) => {
                let mut out = vec![ROOTS, length_byte(names.len())];
                for name in names {
                    put_text(&mut out, name);
                }
                out.push(FEATURES);
                out
            }
            FilesMessage::Listing { entries, truncated } => {
                let mut out = vec![LISTING, u8::from(*truncated)];
                out.extend_from_slice(&u16_length(entries.len()).to_be_bytes());
                for entry in entries {
                    put_text(&mut out, &entry.name);
                    out.push(entry.kind.tag());
                    out.extend_from_slice(&entry.size.to_be_bytes());
                    out.extend_from_slice(&entry.modified.to_be_bytes());
                }
                out
            }
            FilesMessage::Ready { size } => {
                let mut out = vec![READY];
                out.extend_from_slice(&size.to_be_bytes());
                out
            }
            FilesMessage::Data(data) => {
                let mut out = Vec::with_capacity(1 + data.len());
                out.push(DATA);
                out.extend_from_slice(data);
                out
            }
            FilesMessage::Done => vec![DONE],
            FilesMessage::Failed(reason) => vec![FAILED, reason.tag()],
            FilesMessage::Ack { bytes } => {
                let mut out = vec![ACK];
                out.extend_from_slice(&bytes.to_be_bytes());
                out
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, body) = bytes
            .split_first()
            .ok_or(ProtocolError::Truncated { len: 0, needed: 1 })?;
        let mut reader = Reader { tag, rest: body };
        let message = match tag {
            LIST_ROOTS => FilesMessage::ListRoots,
            CANCEL => FilesMessage::Cancel,
            DONE => FilesMessage::Done,
            LIST | DOWNLOAD | MKDIR | REMOVE => {
                let (root, path) = reader.root_and_path()?;
                match tag {
                    LIST => FilesMessage::List { root, path },
                    DOWNLOAD => FilesMessage::Download { root, path },
                    MKDIR => FilesMessage::Mkdir { root, path },
                    _ => FilesMessage::Remove { root, path },
                }
            }
            RENAME => {
                let (root, from) = reader.root_and_path()?;
                let to = reader.text(MAX_PATH_LEN)?;
                let replace = reader.flag()?;
                FilesMessage::Rename {
                    root,
                    from,
                    to,
                    replace,
                }
            }
            UPLOAD => {
                let (root, path) = reader.root_and_path()?;
                let size = reader.u64()?;
                FilesMessage::Upload { root, path, size }
            }
            ROOTS => {
                let count = usize::from(reader.u8()?);
                if count > MAX_ROOTS {
                    return Err(ProtocolError::InvalidControl(tag));
                }
                let names = (0..count)
                    .map(|_| reader.text(MAX_PATH_LEN))
                    .collect::<Result<_, _>>()?;
                if !reader.rest.is_empty() {
                    reader.u8()?;
                }
                FilesMessage::Roots(names)
            }
            LISTING => {
                let truncated = reader.flag()?;
                let count = usize::from(reader.u16()?);
                if count > MAX_ENTRIES {
                    return Err(ProtocolError::InvalidControl(tag));
                }
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(Entry {
                        name: reader.text(MAX_NAME_LEN)?,
                        kind: EntryKind::from_tag(reader.u8()?)?,
                        size: reader.u64()?,
                        modified: reader.u64()?,
                    });
                }
                FilesMessage::Listing { entries, truncated }
            }
            READY => FilesMessage::Ready {
                size: reader.u64()?,
            },
            DATA => {
                if body.is_empty() || body.len() > MAX_CHUNK {
                    return Err(bad_length(tag, MAX_CHUNK, body.len()));
                }
                return Ok(FilesMessage::Data(body.to_vec()));
            }
            ACK => FilesMessage::Ack {
                bytes: reader.u64()?,
            },
            FAILED => FilesMessage::Failed(FailureReason::from_tag(reader.u8()?)?),
            other => return Err(ProtocolError::UnknownControl(other)),
        };
        reader.finish()?;
        Ok(message)
    }
}

struct Reader<'a> {
    tag: u8,
    rest: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], ProtocolError> {
        let (head, tail) = self
            .rest
            .split_at_checked(count)
            .ok_or_else(|| bad_length(self.tag, count, self.rest.len()))?;
        self.rest = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    fn flag(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(ProtocolError::InvalidControl(other)),
        }
    }

    fn u16(&mut self) -> Result<u16, ProtocolError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().unwrap_or_default();
        Ok(u16::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().unwrap_or_default();
        Ok(u64::from_be_bytes(bytes))
    }

    fn text(&mut self, limit: usize) -> Result<String, ProtocolError> {
        let length = usize::from(self.u16()?);
        if length > limit {
            return Err(bad_length(self.tag, limit, length));
        }
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| ProtocolError::InvalidText(self.tag))
    }

    fn root_and_path(&mut self) -> Result<(u8, String), ProtocolError> {
        let root = self.u8()?;
        let path = self.text(MAX_PATH_LEN)?;
        Ok((root, path))
    }

    fn finish(self) -> Result<(), ProtocolError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(bad_length(self.tag, 0, self.rest.len()))
        }
    }
}

fn path_message(tag: u8, root: u8, path: &str) -> Vec<u8> {
    let mut out = vec![tag, root];
    put_text(&mut out, path);
    out
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&u16_length(text.len()).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn u16_length(length: usize) -> u16 {
    u16::try_from(length).unwrap_or(u16::MAX)
}

fn length_byte(length: usize) -> u8 {
    u8::try_from(length).unwrap_or(u8::MAX)
}

fn bad_length(tag: u8, expected: usize, actual: usize) -> ProtocolError {
    ProtocolError::BadControlLength {
        tag,
        expected,
        actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<FilesMessage> {
        vec![
            FilesMessage::ListRoots,
            FilesMessage::List {
                root: 0,
                path: String::new(),
            },
            FilesMessage::List {
                root: 3,
                path: "docs/ñandú".to_owned(),
            },
            FilesMessage::Download {
                root: 1,
                path: "a/b.txt".to_owned(),
            },
            FilesMessage::Upload {
                root: 2,
                path: "x".to_owned(),
                size: u64::MAX,
            },
            FilesMessage::Mkdir {
                root: 0,
                path: "new".to_owned(),
            },
            FilesMessage::Remove {
                root: 0,
                path: "old".to_owned(),
            },
            FilesMessage::Rename {
                root: 1,
                from: "a/old.txt".to_owned(),
                to: "b/new.txt".to_owned(),
                replace: true,
            },
            FilesMessage::Cancel,
            FilesMessage::Roots(vec!["home".to_owned(), "data".to_owned()]),
            FilesMessage::Roots(Vec::new()),
            FilesMessage::Listing {
                entries: vec![
                    Entry {
                        name: "a.txt".to_owned(),
                        kind: EntryKind::File,
                        size: 12,
                        modified: 1_700_000_000,
                    },
                    Entry {
                        name: "dir".to_owned(),
                        kind: EntryKind::Directory,
                        size: 0,
                        modified: 0,
                    },
                    Entry {
                        name: "sock".to_owned(),
                        kind: EntryKind::Other,
                        size: 0,
                        modified: 5,
                    },
                ],
                truncated: true,
            },
            FilesMessage::Ready { size: 7 },
            FilesMessage::Data(vec![0, 1, 2, 255]),
            FilesMessage::Done,
            FilesMessage::Failed(FailureReason::NotAllowed),
            FilesMessage::Failed(FailureReason::Io),
            FilesMessage::Ack { bytes: 65_536 },
        ]
    }

    #[test]
    fn a_data_chunk_fits_in_what_the_viewer_may_send() {
        let frame = crate::HEADER_LEN + 1 + MAX_CHUNK;
        assert!(frame <= crate::MAX_VIEWER_MESSAGE);
    }

    #[test]
    fn round_trips_every_message() {
        for message in samples() {
            assert_eq!(FilesMessage::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn encodes_known_layouts() {
        assert_eq!(FilesMessage::ListRoots.encode(), [0]);
        assert_eq!(
            FilesMessage::List {
                root: 2,
                path: "ab".to_owned()
            }
            .encode(),
            [1, 2, 0, 2, b'a', b'b']
        );
        assert_eq!(
            FilesMessage::Upload {
                root: 0,
                path: String::new(),
                size: 258
            }
            .encode(),
            [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2]
        );
        assert_eq!(
            FilesMessage::Failed(FailureReason::NotFound).encode(),
            [12, 3]
        );
        assert_eq!(FilesMessage::Data(vec![9]).encode(), [10, 9]);
        assert_eq!(
            FilesMessage::Roots(vec!["a".to_owned()]).encode(),
            [7, 1, 0, 1, b'a', FEATURES]
        );
        assert_eq!(
            FilesMessage::decode(&[7, 1, 0, 1, b'a']).unwrap(),
            FilesMessage::Roots(vec!["a".to_owned()])
        );
        assert_eq!(
            FilesMessage::Rename {
                root: 1,
                from: "a".to_owned(),
                to: "bc".to_owned(),
                replace: true
            }
            .encode(),
            [14, 1, 0, 1, b'a', 0, 2, b'b', b'c', 1]
        );
    }

    #[test]
    fn rejects_empty_and_oversized_data() {
        assert!(FilesMessage::decode(&[10]).is_err());
        let mut data = vec![10];
        data.resize(1 + MAX_CHUNK + 1, 0);
        assert!(FilesMessage::decode(&data).is_err());
        data.truncate(1 + MAX_CHUNK);
        assert!(FilesMessage::decode(&data).is_ok());
    }

    #[test]
    fn rejects_trailing_bytes_and_truncation() {
        for message in samples() {
            if matches!(message, FilesMessage::Data(_)) {
                continue;
            }
            let bytes = message.encode();
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(FilesMessage::decode(&longer).is_err(), "{message:?}");
            for cut in 1..bytes.len() {
                if matches!(message, FilesMessage::Roots(_)) && cut == bytes.len() - 1 {
                    continue;
                }
                assert!(FilesMessage::decode(&bytes[..cut]).is_err(), "{message:?}");
            }
        }
    }

    #[test]
    fn rejects_unknown_tags_and_values() {
        assert!(FilesMessage::decode(&[]).is_err());
        assert!(FilesMessage::decode(&[14]).is_err());
        assert!(FilesMessage::decode(&[12, 11]).is_err());
        assert!(FilesMessage::decode(&[8, 2, 0, 0]).is_err());
        assert!(FilesMessage::decode(&[1, 0, 0, 1, 0xFF]).is_err());
    }

    #[test]
    fn rejects_lengths_over_the_limits() {
        let mut path = vec![1, 0];
        path.extend_from_slice(&u16::try_from(MAX_PATH_LEN + 1).unwrap().to_be_bytes());
        path.resize(path.len() + MAX_PATH_LEN + 1, b'a');
        assert!(FilesMessage::decode(&path).is_err());

        let mut roots = vec![7, u8::try_from(MAX_ROOTS + 1).unwrap()];
        for _ in 0..=MAX_ROOTS {
            roots.extend_from_slice(&[0, 1, b'a']);
        }
        assert!(FilesMessage::decode(&roots).is_err());

        let mut listing = vec![8, 0];
        listing.extend_from_slice(&u16::try_from(MAX_ENTRIES + 1).unwrap().to_be_bytes());
        assert!(FilesMessage::decode(&listing).is_err());
    }
}
