use arboard::Clipboard as Board;

use crate::error::ClipboardError;
use crate::{Clipboard, MAX_TEXT_BYTES};

pub struct WindowsClipboard {
    board: Board,
}

impl std::fmt::Debug for WindowsClipboard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsClipboard")
            .finish_non_exhaustive()
    }
}

impl WindowsClipboard {
    pub fn connect() -> Result<Self, ClipboardError> {
        let board = Board::new().map_err(|error| ClipboardError::Connect(error.to_string()))?;
        Ok(Self { board })
    }
}

impl Clipboard for WindowsClipboard {
    fn read(&mut self) -> Result<Option<String>, ClipboardError> {
        let text = match self.board.get_text() {
            Ok(text) => text,
            Err(arboard::Error::ContentNotAvailable) => return Ok(None),
            Err(error) => return Err(ClipboardError::Request(error.to_string())),
        };
        if text.len() > MAX_TEXT_BYTES {
            return Err(ClipboardError::TooLarge(MAX_TEXT_BYTES));
        }
        Ok((!text.is_empty()).then_some(text))
    }

    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ClipboardError::TooLarge(MAX_TEXT_BYTES));
        }
        self.board
            .set_text(text)
            .map_err(|error| ClipboardError::Request(error.to_string()))
    }
}
