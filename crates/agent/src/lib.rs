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
#[cfg(windows)]
mod winacl;

pub use error::ServerError;
pub use http::{Exposure, ExposureError};
pub use server::{Server, ServerConfig};
pub use store::{Clock, Store, StoreError, VaultSource, default_dir, system_clock, system_vault};
