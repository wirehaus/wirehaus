use crate::error::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Desktop,
    Terminal,
    Files,
    Clipboard,
    Audio,
    Capture,
}

impl Channel {
    pub const ALL: [Channel; 6] = [
        Channel::Desktop,
        Channel::Terminal,
        Channel::Files,
        Channel::Clipboard,
        Channel::Audio,
        Channel::Capture,
    ];

    pub(crate) fn tag(self) -> u8 {
        match self {
            Channel::Desktop => 0,
            Channel::Terminal => 1,
            Channel::Files => 2,
            Channel::Clipboard => 3,
            Channel::Audio => 4,
            Channel::Capture => 5,
        }
    }

    pub(crate) fn from_tag(tag: u8) -> Result<Self, ProtocolError> {
        match tag {
            0 => Ok(Channel::Desktop),
            1 => Ok(Channel::Terminal),
            2 => Ok(Channel::Files),
            3 => Ok(Channel::Clipboard),
            4 => Ok(Channel::Audio),
            5 => Ok(Channel::Capture),
            other => Err(ProtocolError::UnknownChannel(other)),
        }
    }
}
