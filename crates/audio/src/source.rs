use crate::error::AudioError;

pub type Interrupter = Box<dyn Fn() + Send + Sync>;

pub trait AudioSource: Send {
    fn read_frame(&mut self, frame: &mut [f32]) -> Result<(), AudioError>;
    fn interrupter(&self) -> Interrupter;
}

#[cfg(target_os = "linux")]
mod parec;

#[cfg(any(windows, target_os = "macos"))]
mod loopback;

#[cfg(any(windows, target_os = "macos", test))]
mod resample;

pub fn open_default() -> Result<Box<dyn AudioSource>, AudioError> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(parec::Parec::open()?))
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        Ok(Box::new(loopback::Loopback::open()?))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(AudioError::Unavailable(
            "no capture backend for this system yet".to_owned(),
        ))
    }
}
