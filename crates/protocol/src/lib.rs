mod audio;
mod channel;
mod clipboard;
mod control;
mod desktop;
mod error;
mod files;
mod frame;
mod input;
mod link;
mod terminal;
mod viewer;

pub use audio::{AudioFailure, AudioMessage, MAX_AUDIO_PACKET};
pub use channel::Channel;
pub use clipboard::{ClipboardFailure, ClipboardMessage, MAX_CLIPBOARD_TEXT};
pub use control::{AuthFailure, CODE_LEN, ControlMessage, EndReason, TICKET_LEN};
pub use desktop::{Codec, DesktopMessage, MAX_MONITORS, MonitorInfo};
pub use error::ProtocolError;
pub use files::{
    Entry, EntryKind, FailureReason, FilesMessage, MAX_CHUNK, MAX_ENTRIES, MAX_NAME_LEN,
    MAX_PATH_LEN, MAX_ROOTS,
};
pub use frame::{Frame, HEADER_LEN, MAX_PAYLOAD_LEN, MAX_VIEWER_MESSAGE};
pub use input::{Button, InputEvent};
pub use link::{
    LINK_CODE_LEN, LINK_ID_LEN, LINK_KEY_MAX, LINK_KEY_MIN, LINK_VERIFY_LEN, LinkFailure,
    LinkMessage,
};
pub use terminal::{
    MAX_DIMENSION, MAX_INPUT_CHUNK, MAX_OUTPUT_CHUNK, RefusalReason, TerminalMessage,
};
pub use viewer::ViewerMessage;

pub const PROTOCOL_VERSION: u16 = 1;
