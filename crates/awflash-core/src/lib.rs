//! awflash-core — the shared image-flashing engine for the Airwaves OS Flasher.
//!
//! Used by both the unprivileged Tauri GUI (catalog, enumeration, inspection)
//! and the elevated `awflash-helper` (the destructive write/verify/customize
//! pipeline). No GUI or Tauri dependencies live here.

pub mod catalog;
pub mod customize;
pub mod decompress;
pub mod device;
pub mod enumerate;
pub mod error;
pub mod escalation;
pub mod flash;
pub mod inspect;
pub mod model;
pub mod progress;
pub mod provision;
pub mod sshkeys;

pub use error::{FlashError, Result};
pub use model::*;
