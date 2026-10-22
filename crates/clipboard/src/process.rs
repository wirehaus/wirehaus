use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

use crate::error::ClipboardError;
use crate::{Clipboard, MAX_TEXT_BYTES};

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Tool {
    pub program: String,
    pub args: Vec<String>,
}

impl Tool {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8");
        command
    }
}

#[derive(Debug)]
pub struct ProcessClipboard {
    copy: Tool,
    paste: Tool,
}

impl ProcessClipboard {
    pub fn new(copy: Tool, paste: Tool) -> Self {
        Self { copy, paste }
    }
}

impl Clipboard for ProcessClipboard {
    fn read(&mut self) -> Result<Option<String>, ClipboardError> {
        let mut child = self
            .paste
            .command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(request_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| request_error("no output pipe"))?;
        let (sender, receiver) = channel();
        let limit = u64::try_from(MAX_TEXT_BYTES + 1).unwrap_or(u64::MAX);
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.take(limit).read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        });
        let outcome = receiver.recv_timeout(TIMEOUT);
        let _ = child.kill();
        let _ = child.wait();
        let bytes = match outcome {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => return Err(request_error(error)),
            Err(_) => return Err(ClipboardError::Timeout),
        };
        if bytes.len() > MAX_TEXT_BYTES {
            return Err(ClipboardError::TooLarge(MAX_TEXT_BYTES));
        }
        let text = String::from_utf8(bytes).map_err(request_error)?;
        Ok((!text.is_empty()).then_some(text))
    }

    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ClipboardError::TooLarge(MAX_TEXT_BYTES));
        }
        let mut child = self
            .copy
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(request_error)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| request_error("no input pipe"))?;
        let payload = text.to_owned();
        let (sender, receiver) = channel();
        thread::spawn(move || {
            let _ = sender.send(stdin.write_all(payload.as_bytes()));
        });
        let written = receiver.recv_timeout(TIMEOUT);
        if !matches!(written, Ok(Ok(()))) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(match written {
                Ok(Err(error)) => request_error(error),
                _ => ClipboardError::Timeout,
            });
        }
        let status = child.wait().map_err(request_error)?;
        if status.success() {
            Ok(())
        } else {
            Err(request_error(format!("the copy tool exited with {status}")))
        }
    }
}

fn request_error(error: impl std::fmt::Display) -> ClipboardError {
    ClipboardError::Request(error.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn clipboard(directory: &std::path::Path) -> ProcessClipboard {
        let file = directory.join("clip");
        let file = file.to_str().unwrap();
        ProcessClipboard::new(
            Tool::new("sh", &["-c", &format!("cat > '{file}'")]),
            Tool::new("sh", &["-c", &format!("cat '{file}' 2>/dev/null; true")]),
        )
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("wirehaus-clip-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn text_written_is_read_back_unchanged() {
        let directory = scratch("roundtrip");
        let mut clipboard = clipboard(&directory);
        let text = "Hola, ni\u{f1}o \u{4e16}\u{754c}\nsegunda l\u{ed}nea\t$HOME `x` 'y' \"z\"";
        clipboard.write(text).unwrap();
        assert_eq!(clipboard.read().unwrap().as_deref(), Some(text));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_empty_clipboard_reads_as_nothing() {
        let directory = scratch("empty");
        let mut clipboard = clipboard(&directory);
        assert_eq!(clipboard.read().unwrap(), None);
        clipboard.write("").unwrap();
        assert_eq!(clipboard.read().unwrap(), None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn text_over_the_limit_is_refused_both_ways() {
        let directory = scratch("limit");
        let mut clipboard = clipboard(&directory);
        let big = "a".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(
            clipboard.write(&big),
            Err(ClipboardError::TooLarge(MAX_TEXT_BYTES))
        );
        std::fs::write(directory.join("clip"), &big).unwrap();
        assert_eq!(
            clipboard.read(),
            Err(ClipboardError::TooLarge(MAX_TEXT_BYTES))
        );
        let exact = "b".repeat(MAX_TEXT_BYTES);
        clipboard.write(&exact).unwrap();
        assert_eq!(clipboard.read().unwrap().as_deref(), Some(exact.as_str()));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_failing_tool_is_an_error() {
        let mut clipboard = ProcessClipboard::new(
            Tool::new("sh", &["-c", "cat >/dev/null; exit 3"]),
            Tool::new("sh", &["-c", "printf '\\377'"]),
        );
        assert!(matches!(
            clipboard.write("x"),
            Err(ClipboardError::Request(_))
        ));
        assert!(matches!(clipboard.read(), Err(ClipboardError::Request(_))));
    }

    #[test]
    fn a_tool_that_hangs_times_out() {
        let mut clipboard =
            ProcessClipboard::new(Tool::new("true", &[]), Tool::new("sleep", &["30"]));
        assert_eq!(clipboard.read(), Err(ClipboardError::Timeout));
    }

    #[test]
    fn a_missing_tool_is_an_error() {
        let mut clipboard = ProcessClipboard::new(
            Tool::new("/nonexistent/wirehaus-copy", &[]),
            Tool::new("/nonexistent/wirehaus-paste", &[]),
        );
        assert!(matches!(
            clipboard.write("x"),
            Err(ClipboardError::Request(_))
        ));
        assert!(matches!(clipboard.read(), Err(ClipboardError::Request(_))));
    }
}
