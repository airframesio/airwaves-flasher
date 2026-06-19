//! Typed error taxonomy. Each variant maps to calm, specific user copy in the
//! UI (see the error table in docs/RESEARCH-PLAN.md §5.3). The `kind()` string
//! crosses the IPC boundary so the GUI can localize without parsing messages.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlashError {
    #[error("administrator access is required to write to disks")]
    PermissionDenied,

    #[error("the drive is in use; close apps using it and retry")]
    DeviceBusy,

    #[error("the drive was disconnected")]
    DeviceRemoved,

    #[error("this image is larger than the selected drive")]
    InsufficientSpace,

    #[error("verification failed — the card may be counterfeit or failing")]
    VerifyMismatch,

    #[error("the selected device is a system or non-removable disk and was refused")]
    UnsafeTarget,

    #[error("download failed: {0}")]
    Download(String),

    #[error("could not read the image: {0}")]
    Image(String),

    #[error("could not write first-boot configuration: {0}")]
    Customize(String),

    #[error("cancelled")]
    Cancelled,

    #[error("{0}")]
    Io(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, FlashError>;

impl FlashError {
    /// Stable machine-readable classification for the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            FlashError::PermissionDenied => "PermissionDenied",
            FlashError::DeviceBusy => "DeviceBusy",
            FlashError::DeviceRemoved => "DeviceRemoved",
            FlashError::InsufficientSpace => "InsufficientSpace",
            FlashError::VerifyMismatch => "VerifyMismatch",
            FlashError::UnsafeTarget => "UnsafeTarget",
            FlashError::Download(_) => "Download",
            FlashError::Image(_) => "Image",
            FlashError::Customize(_) => "Customize",
            FlashError::Cancelled => "Cancelled",
            FlashError::Io(_) => "IoError",
            FlashError::Other(_) => "Other",
        }
    }
}

impl From<std::io::Error> for FlashError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match e.kind() {
            PermissionDenied => FlashError::PermissionDenied,
            NotFound => FlashError::DeviceRemoved,
            _ => FlashError::Io(e.to_string()),
        }
    }
}

impl From<reqwest::Error> for FlashError {
    fn from(e: reqwest::Error) -> Self {
        FlashError::Download(e.to_string())
    }
}
