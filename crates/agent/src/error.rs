use std::net::SocketAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("{0} is not a loopback address; this version only listens on the local machine")]
    NotLoopback(SocketAddr),
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    #[error("screen capture is not available: {0}")]
    Capture(#[from] wirehaus_capture::CaptureError),
    #[error("input injection is not available: {0}")]
    Input(#[from] wirehaus_input::InputError),
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    #[error("encryption failed: {0}")]
    Crypto(#[from] wirehaus_crypto::CryptoError),
    #[error("encoding failed: {0}")]
    Encode(#[from] wirehaus_encode::EncodeError),
    #[error("invalid message from the viewer: {0}")]
    Protocol(#[from] wirehaus_protocol::ProtocolError),
    #[error("unexpected message on the {0:?} channel")]
    UnexpectedChannel(wirehaus_protocol::Channel),
    #[error("access was revoked or the agent was locked")]
    AccessRevoked,
    #[error("the viewer stopped acknowledging frames")]
    ViewerStalled,
    #[error("the viewer sent a message that is not binary")]
    UnexpectedMessage,
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("this platform is not supported yet")]
    UnsupportedPlatform,
}

impl From<tungstenite::Error> for ServerError {
    fn from(error: tungstenite::Error) -> Self {
        ServerError::WebSocket(error.to_string())
    }
}
