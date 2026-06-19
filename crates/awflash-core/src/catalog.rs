//! Catalog acquisition: fetch the published `catalog.json`, and if that is
//! missing/unreachable, synthesize one from the GitHub Releases API by matching
//! `Airwaves_OS_<board>-<version>.img.<ext>` assets.

use crate::error::{FlashError, Result};
use crate::model::{Catalog, CatalogDevice, CatalogImage, Compression};
use serde::Deserialize;

pub const GITHUB_REPO: &str = "airframesio/airwaves-os";
const USER_AGENT: &str = concat!("airwaves-os-flasher/", env!("CARGO_PKG_VERSION"));

/// Stable/beta floor. Legacy Armbian-style date versions (23.11.*, 24.*) are
/// always hidden (major >= 20, even though they sort above 1.0.37). The dev
/// channel allows any current-era 1.x build (the dev line currently sits below
/// stable, e.g. 1.0.24).
const MIN_VERSION: (u32, u32, u32) = (1, 0, 37);

fn version_supported(v: &str, channel: &str) -> bool {
    let parts: Vec<u32> = v
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|s| s.parse().unwrap_or(0))
        .collect();
    let major = parts.first().copied().unwrap_or(0);
    if major >= 20 {
        return false;
    }
    if channel == "dev" {
        return true;
    }
    let tuple = (
        major,
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    );
    tuple >= MIN_VERSION
}

/// Drop unsupported images, then any device left with no images.
fn prune(cat: &mut Catalog) {
    cat.images
        .retain(|i| version_supported(&i.os_version, &i.channel));
    let used: std::collections::HashSet<String> =
        cat.images.iter().map(|i| i.device.clone()).collect();
    cat.devices.retain(|d| used.contains(&d.id));
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("reqwest client")
}

/// Try the configured catalog URL first; fall back to the GitHub Releases API.
pub async fn fetch_catalog(url: &str) -> Result<Catalog> {
    match fetch_catalog_json(url).await {
        Ok(mut cat) => {
            cat.source.get_or_insert_with(|| "remote".to_string());
            prune(&mut cat);
            Ok(cat)
        }
        Err(remote_err) => match fetch_from_github().await {
            Ok(mut cat) => {
                prune(&mut cat);
                Ok(cat)
            }
            Err(gh_err) => Err(FlashError::Download(format!(
                "catalog unavailable ({remote_err}); GitHub fallback failed ({gh_err})"
            ))),
        },
    }
}

async fn fetch_catalog_json(url: &str) -> Result<Catalog> {
    let resp = client().get(url).send().await?;
    if !resp.status().is_success() {
        return Err(FlashError::Download(format!("HTTP {}", resp.status())));
    }
    let cat: Catalog = resp
        .json()
        .await
        .map_err(|e| FlashError::Download(format!("invalid catalog JSON: {e}")))?;
    Ok(cat)
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

async fn fetch_from_github() -> Result<Catalog> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=30");
    let resp = client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(FlashError::Download(format!("GitHub HTTP {}", resp.status())));
    }
    let releases: Vec<GhRelease> = resp
        .json()
        .await
        .map_err(|e| FlashError::Download(format!("invalid GitHub JSON: {e}")))?;

    let mut images: Vec<CatalogImage> = Vec::new();
    let mut device_ids: Vec<String> = Vec::new();

    for rel in &releases {
        let channel = if rel.prerelease { "beta" } else { "stable" };
        let os_version = rel.tag_name.trim_start_matches('v').to_string();
        for asset in &rel.assets {
            if let Some((slug, ext)) = parse_image_asset(&asset.name) {
                if !device_ids.iter().any(|d| d == &slug) {
                    device_ids.push(slug.clone());
                }
                images.push(CatalogImage {
                    id: format!("{slug}-{channel}-{os_version}"),
                    device: slug.clone(),
                    channel: channel.to_string(),
                    os_version: os_version.clone(),
                    codename: rel.name.clone(),
                    name: format!("Airwaves OS {os_version} ({})", device_name(&slug)),
                    released: rel.published_at.clone(),
                    notes: rel.body.clone(),
                    notes_url: Some(rel.html_url.clone()),
                    url: asset.browser_download_url.clone(),
                    compression: ext,
                    download_size: asset.size,
                    extract_size: 0, // unknown from GitHub; write shows a byte counter
                    download_sha256: None,
                    sha256: None,
                    min_storage_bytes: None,
                    latest: None,
                });
            }
        }
    }

    // drop legacy versions before choosing the latest per device+channel
    images.retain(|i| version_supported(&i.os_version, &i.channel));
    if images.is_empty() {
        return Err(FlashError::Download(
            "no supported Airwaves OS image assets found in GitHub releases".into(),
        ));
    }

    // mark the newest image per device+channel
    mark_latest(&mut images);

    let devices = device_ids.iter().map(|s| make_device(s)).collect();
    let channels = vec!["stable".to_string(), "beta".to_string(), "dev".to_string()];

    Ok(Catalog {
        schema: 1,
        name: "Airwaves OS".into(),
        updated: None,
        channels,
        devices,
        images,
        source: Some("github".into()),
    })
}

/// Returns (device-slug, compression) if `name` looks like an Airwaves image.
fn parse_image_asset(name: &str) -> Option<(String, Compression)> {
    let lower = name.to_ascii_lowercase();
    let ext = if lower.ends_with(".img.xz") {
        Compression::Xz
    } else if lower.ends_with(".img.gz") {
        Compression::Gz
    } else if lower.ends_with(".img.zst") || lower.ends_with(".img.zstd") {
        Compression::Zst
    } else if lower.ends_with(".img.zip") || lower.ends_with(".zip") {
        Compression::Zip
    } else {
        return None;
    };
    // strip the recognised compression + .img and the Airwaves_OS_ prefix
    let stem = name
        .rsplit_once(".img")
        .map(|(a, _)| a)
        .unwrap_or(name)
        .to_string();
    let rest = stem
        .strip_prefix("Airwaves_OS_")
        .or_else(|| stem.strip_prefix("airwaves-os-"))
        .or_else(|| stem.strip_prefix("Airwaves-OS-"))?;
    // rest is "<board>-<version>"; split at the last hyphen before a digit run
    let slug = match rest.rsplit_once('-') {
        Some((board, ver)) if ver.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) => {
            board.to_string()
        }
        _ => rest.to_string(),
    };
    Some((slug.to_ascii_lowercase(), ext))
}

fn mark_latest(images: &mut [CatalogImage]) {
    use std::collections::HashMap;
    let mut best: HashMap<(String, String), usize> = HashMap::new();
    for (i, img) in images.iter().enumerate() {
        let key = (img.device.clone(), img.channel.clone());
        match best.get(&key) {
            Some(&j) if images[j].os_version >= img.os_version => {}
            _ => {
                best.insert(key, i);
            }
        }
    }
    let latest_idx: std::collections::HashSet<usize> = best.values().copied().collect();
    for (i, img) in images.iter_mut().enumerate() {
        img.latest = Some(latest_idx.contains(&i));
    }
}

fn device_name(slug: &str) -> &str {
    match slug {
        s if s.contains("raspberrypi") || s.contains("rpi") => "Raspberry Pi",
        s if s.contains("rock-5") || s.contains("rock5") => "Radxa Rock 5B",
        s if s.contains("orangepi5") || s.contains("orange") => "Orange Pi 5",
        s if s.contains("x86") || s.contains("uefi") || s.contains("amd64") => "PC / x86-64",
        _ => "Generic",
    }
}

fn make_device(slug: &str) -> CatalogDevice {
    let (name, icon, arch, desc): (&str, &str, &str, &str) = match slug {
        s if s.contains("raspberrypi") || s.contains("rpi") => (
            "Raspberry Pi 3 / 4 / 5",
            "rpi",
            "arm64",
            "64-bit ARM. The default feeder board.",
        ),
        s if s.contains("rock-5") || s.contains("rock5") => (
            "Radxa Rock 5B",
            "rock",
            "arm64",
            "High-performance RK3588 board.",
        ),
        s if s.contains("orangepi5") || s.contains("orange") => {
            ("Orange Pi 5", "orangepi", "arm64", "Compact RK3588S board.")
        }
        s if s.contains("x86") || s.contains("uefi") || s.contains("amd64") => (
            "PC / Mini-PC (x86 UEFI)",
            "x86",
            "amd64",
            "Any 64-bit UEFI PC or mini-PC.",
        ),
        _ => ("Airwaves OS device", "x86", "", "Airwaves OS target."),
    };
    CatalogDevice {
        id: slug.to_string(),
        name: name.to_string(),
        description: desc.to_string(),
        icon: icon.to_string(),
        arch: arch.to_string(),
        vendor: None,
        tags: vec![],
    }
}
