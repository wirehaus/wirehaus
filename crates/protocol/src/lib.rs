mod channel;
mod error;
mod frame;
mod input;

pub use channel::Channel;
pub use error::ProtocolError;
pub use frame::{Frame, HEADER_LEN, MAX_PAYLOAD_LEN};
pub use input::{Button, InputEvent};

pub const PROTOCOL_VERSION: u16 = 1;
