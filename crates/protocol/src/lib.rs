mod channel;
mod error;
mod frame;

pub use channel::Channel;
pub use error::ProtocolError;
pub use frame::{Frame, MAX_PAYLOAD_LEN};

pub const PROTOCOL_VERSION: u16 = 1;
