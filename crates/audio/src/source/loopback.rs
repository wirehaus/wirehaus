use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::FRAME_VALUES;
use crate::error::AudioError;
use crate::source::resample::Converter;
use crate::source::{AudioSource, Interrupter};

const QUEUE_FRAMES: usize = 25;
const IDLE: Duration = Duration::from_millis(20);

pub struct Loopback {
    frames: Receiver<Vec<f32>>,
    failure: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Loopback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Loopback").finish_non_exhaustive()
    }
}

impl Loopback {
    pub fn open() -> Result<Self, AudioError> {
        let (frames_in, frames) = sync_channel(QUEUE_FRAMES);
        let (ready_in, ready) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(String::new()));
        let thread = {
            let stop = Arc::clone(&stop);
            let failure = Arc::clone(&failure);
            std::thread::spawn(move || run(&frames_in, &ready_in, &stop, &failure))
        };
        match ready.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                frames,
                failure,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err(AudioError::Unavailable(
                    "the audio device did not answer".to_owned(),
                ))
            }
        }
    }
}

impl AudioSource for Loopback {
    fn read_frame(&mut self, frame: &mut [f32]) -> Result<(), AudioError> {
        match self.frames.recv_timeout(IDLE) {
            Ok(data) => {
                frame.copy_from_slice(&data);
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => {
                frame.fill(0.0);
                Ok(())
            }
            Err(RecvTimeoutError::Disconnected) => {
                let reason = self
                    .failure
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                Err(AudioError::Capture(if reason.is_empty() {
                    "the audio device went away".to_owned()
                } else {
                    reason
                }))
            }
        }
    }

    fn interrupter(&self) -> Interrupter {
        let stop = Arc::clone(&self.stop);
        Box::new(move || stop.store(true, Ordering::Relaxed))
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    frames: &SyncSender<Vec<f32>>,
    ready: &std::sync::mpsc::Sender<Result<(), AudioError>>,
    stop: &Arc<AtomicBool>,
    failure: &Arc<Mutex<String>>,
) {
    let stream = match build(frames.clone(), Arc::clone(stop), Arc::clone(failure)) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(stream);
}

fn build(
    frames: SyncSender<Vec<f32>>,
    stop: Arc<AtomicBool>,
    failure: Arc<Mutex<String>>,
) -> Result<cpal::Stream, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AudioError::Unavailable("there is no audio output device".to_owned()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Unavailable(e.to_string()))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.config();
    let mut converter = Converter::new(config.sample_rate, usize::from(config.channels));
    let sink = move |samples: &[f32]| {
        converter.push(samples, |block| {
            debug_assert_eq!(block.len(), FRAME_VALUES);
            let _ = frames.try_send(block.to_vec());
        });
    };
    let on_error = move |error: cpal::Error| {
        if matches!(
            error.kind(),
            cpal::ErrorKind::Xrun
                | cpal::ErrorKind::DeviceChanged
                | cpal::ErrorKind::RealtimeDenied
        ) {
            return;
        }
        *failure.lock().unwrap_or_else(PoisonError::into_inner) = error.to_string();
        stop.store(true, Ordering::Relaxed);
    };
    let stream = match format {
        SampleFormat::F32 => {
            let mut sink = sink;
            device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| sink(data),
                on_error,
                None,
            )
        }
        SampleFormat::I16 => {
            let mut sink = sink;
            device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let scaled: Vec<f32> = data.iter().map(|v| f32::from(*v) / 32768.0).collect();
                    sink(&scaled);
                },
                on_error,
                None,
            )
        }
        other => {
            return Err(AudioError::Unavailable(format!(
                "the audio device uses an unsupported sample format ({other:?})"
            )));
        }
    }
    .map_err(|e| AudioError::Unavailable(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AudioError::Unavailable(e.to_string()))?;
    Ok(stream)
}
