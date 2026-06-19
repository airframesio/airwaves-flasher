//! Serde data model. Field names mirror `src/types.ts` exactly (camelCase) so
//! the same JSON crosses the Tauri boundary and the helper IPC unchanged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compression {
    Xz,
    Gz,
    Zst,
    Zip,
    None,
}

impl Compression {
    /// Detect from a filename, used only as a hint (magic bytes win at write time).
    pub fn from_name(name: &str) -> Compression {
        let n = name.to_ascii_lowercase();
        if n.ends_with(".xz") {
            Compression::Xz
        } else if n.ends_with(".gz") || n.ends_with(".tgz") {
            Compression::Gz
        } else if n.ends_with(".zst") || n.ends_with(".zstd") {
            Compression::Zst
        } else if n.ends_with(".zip") {
            Compression::Zip
        } else {
            Compression::None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDevice {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogImage {
    pub id: String,
    pub device: String,
    pub channel: String,
    pub os_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codename: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    pub url: String,
    pub compression: Compression,
    #[serde(default)]
    pub download_size: u64,
    #[serde(default)]
    pub extract_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_storage_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    pub channels: Vec<String>,
    pub devices: Vec<CatalogDevice>,
    pub images: Vec<CatalogImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub id: String,
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_device: Option<String>,
    pub description: String,
    pub size_bytes: u64,
    pub removable: bool,
    pub system: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus_type: Option<String>,
    #[serde(default)]
    pub mountpoints: Vec<String>,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalImage {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub compression: Compression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiConfig {
    pub ssid: String,
    #[serde(default)]
    pub psk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Customization {
    pub enabled: bool,
    #[serde(default)]
    pub hostname: String,
    pub wifi: WifiConfig,
    #[serde(default)]
    pub wifi_enabled: bool,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub ssh_enabled: bool,
    #[serde(default)]
    pub ssh_keys: Vec<String>,
}

/// Discriminated union mirroring `ImageSource` in types.ts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ImageSource {
    Catalog { image: CatalogImage },
    Local { file: LocalImage },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashRequest {
    pub source: ImageSource,
    pub device: DriveInfo,
    pub customization: Customization,
    pub verify: bool,
    pub eject: bool,
    /// How long (seconds) to keep a downloaded image cached for reuse. 0 disables
    /// caching. Consumed by the GUI's unprivileged download step, not the helper.
    #[serde(default)]
    pub cache_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashPhase {
    Preparing,
    Downloading,
    Writing,
    Verifying,
    Configuring,
    Finalizing,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashProgress {
    pub phase: FlashPhase,
    pub fraction: f64,
    pub bytes_processed: u64,
    pub bytes_total: u64,
    pub speed_bps: f64,
    pub eta_secs: f64,
    /// True while throughput is still settling (the first stretch of a phase) —
    /// the UI shows "Pending" instead of a wild ETA.
    #[serde(default)]
    pub eta_pending: bool,
    /// Non-fatal warning to surface on completion (e.g. first-boot config could
    /// not be written because the image has no AWCFG partition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub message: String,
}

impl FlashProgress {
    pub fn new(phase: FlashPhase, message: impl Into<String>) -> Self {
        FlashProgress {
            phase,
            fraction: 0.0,
            bytes_processed: 0,
            bytes_total: 0,
            speed_bps: 0.0,
            eta_secs: 0.0,
            eta_pending: false,
            warning: None,
            message: message.into(),
        }
    }
}
