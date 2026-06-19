//! Authoritative flash run-state, owned by the Rust side so a webview reload
//! can't orphan an in-flight write.

use awflash_core::progress::Cancel;
use std::sync::Mutex;

#[derive(Default)]
pub struct RunState {
    pub active: bool,
    /// Set true to cancel the unprivileged download phase.
    pub cancel_flag: Option<Cancel>,
    /// Path whose creation signals the elevated helper to cancel.
    pub cancel_file: Option<String>,
}

pub type SharedState = Mutex<RunState>;
