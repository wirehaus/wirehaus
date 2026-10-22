use crate::Clipboard;
use crate::error::ClipboardError;
use crate::process::{ProcessClipboard, Tool};

#[derive(Debug)]
pub struct MacClipboard {
    inner: ProcessClipboard,
}

impl MacClipboard {
    #[must_use]
    pub fn connect() -> Self {
        Self {
            inner: ProcessClipboard::new(
                Tool::new("/usr/bin/pbcopy", &[]),
                Tool::new("/usr/bin/pbpaste", &[]),
            ),
        }
    }
}

impl Clipboard for MacClipboard {
    fn read(&mut self) -> Result<Option<String>, ClipboardError> {
        self.inner.read()
    }

    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.inner.write(text)
    }
}
