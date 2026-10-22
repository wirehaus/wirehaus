mod channel;
mod control;
mod desktop;
mod error;
mod frame;
mod input;
mod viewer;

pub use channel::Channel;
pub use control::{AuthFailure, CODE_LEN, ControlMessage};
pub use desktop::{Codec, DesktopMessage};
pub use error::ProtocolError;
pub use frame::{Frame, HEADER_LEN, MAX_PAYLOAD_LEN};
pub use input::{Button, InputEvent};
pub use viewer::ViewerMessage;

pub const PROTOCOL_VERSION: u16 = 1;
