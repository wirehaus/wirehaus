pub mod audio;
mod auth;
pub mod clipboard;
pub mod commands;
mod error;
pub mod files;
mod http;
pub mod i18n;
pub mod integrity;
#[cfg(any(windows, test))]
pub mod launcher;
mod link;
mod mode;
mod pacer;
mod platform;
mod server;
#[cfg(unix)]
pub mod service;
#[cfg(windows)]
#[path = "winservice.rs"]
pub mod service;
mod session;
pub mod state;
pub mod static_files;
pub mod store;
mod stream;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod taskxml;
pub mod terminal;
#[cfg(test)]
mod testing;
pub mod update;
#[cfg(windows)]
mod winacl;

pub use error::ServerError;
pub use http::{Exposure, ExposureError};
pub use server::{Server, ServerConfig};
pub use store::{Clock, Store, StoreError, VaultSource, default_dir, system_clock, system_vault};

#[cfg(windows)]
pub fn preflight(display: Option<&str>) -> Result<(), String> {
    platform::connect(display).map(drop).map_err(|error| {
        let hint = i18n::tr!(
            "El agente tiene que ejecutarse dentro de la sesión de escritorio del usuario, con la pantalla desbloqueada; no sirve desde SSH ni como servicio de Windows. Usa wirehaus-agent service install para que arranque solo al iniciar sesión.",
            "The agent has to run inside the user's desktop session with the screen unlocked; it does not work from SSH or as a Windows service. Use wirehaus-agent service install to have it start by itself when you sign in."
        );
        format!("{error}. {hint}")
    })
}
