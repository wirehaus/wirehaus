use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::AudioError;
use crate::source::{AudioSource, Interrupter};
use crate::{CHANNELS, FRAME_VALUES, SAMPLE_RATE};

pub struct Parec {
    child: Arc<Mutex<Child>>,
    stdout: ChildStdout,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for Parec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Parec").finish_non_exhaustive()
    }
}

impl Parec {
    pub fn open() -> Result<Self, AudioError> {
        let mut child = Command::new("parec")
            .args([
                "--device=@DEFAULT_MONITOR@",
                "--format=s16le",
                &format!("--rate={SAMPLE_RATE}"),
                &format!("--channels={CHANNELS}"),
                "--latency-msec=20",
                "--client-name=wirehaus",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AudioError::Unavailable(format!("cannot run parec: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AudioError::Capture("parec has no output".to_owned()))?;
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            stdout,
            bytes: vec![0; FRAME_VALUES * 2],
        })
    }
}

impl AudioSource for Parec {
    fn read_frame(&mut self, frame: &mut [f32]) -> Result<(), AudioError> {
        self.stdout
            .read_exact(&mut self.bytes)
            .map_err(|e| AudioError::Capture(e.to_string()))?;
        for (value, pair) in frame.iter_mut().zip(self.bytes.as_chunks::<2>().0) {
            *value = f32::from(i16::from_le_bytes(*pair)) / 32768.0;
        }
        Ok(())
    }

    fn interrupter(&self) -> Interrupter {
        let child = Arc::clone(&self.child);
        Box::new(move || terminate(&child))
    }
}

fn terminate(child: &Mutex<Child>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Parec {
    fn drop(&mut self) {
        terminate(&self.child);
    }
}
