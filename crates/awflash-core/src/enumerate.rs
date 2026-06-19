//! Cross-platform removable-drive enumeration. Each backend shells out to the
//! native tool (diskutil / lsblk / PowerShell) and normalizes to `DriveInfo`.
//! The GUI filters to `removable && !system && !readOnly`; the elevated helper
//! re-validates independently before writing.

use crate::error::{FlashError, Result};
use crate::model::DriveInfo;

pub fn list_drives() -> Result<Vec<DriveInfo>> {
    #[cfg(target_os = "macos")]
    {
        macos::list()
    }
    #[cfg(target_os = "linux")]
    {
        linux::list()
    }
    #[cfg(target_os = "windows")]
    {
        windows::list()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(vec![])
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use plist::Value;
    use std::process::Command;

    pub fn list() -> Result<Vec<DriveInfo>> {
        let out = Command::new("diskutil")
            .args(["list", "-plist", "physical"])
            .output()
            .map_err(|e| FlashError::Other(format!("diskutil: {e}")))?;
        let root: Value = plist::from_bytes(&out.stdout)
            .map_err(|e| FlashError::Other(format!("diskutil plist: {e}")))?;
        let whole = root
            .as_dictionary()
            .and_then(|d| d.get("WholeDisks"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut drives = Vec::new();
        for disk in whole {
            if let Some(id) = disk.as_string() {
                if let Some(info) = info_for(id) {
                    drives.push(info);
                }
            }
        }
        Ok(drives)
    }

    fn info_for(id: &str) -> Option<DriveInfo> {
        let out = Command::new("diskutil")
            .args(["info", "-plist", id])
            .output()
            .ok()?;
        let v: Value = plist::from_bytes(&out.stdout).ok()?;
        let d = v.as_dictionary()?;
        let getb = |k: &str| d.get(k).and_then(|x| x.as_boolean());
        let gets = |k: &str| d.get(k).and_then(|x| x.as_string()).map(|s| s.to_string());
        let geti = |k: &str| d.get(k).and_then(|x| x.as_signed_integer());

        let size = geti("TotalSize").or_else(|| geti("Size")).unwrap_or(0) as u64;
        let internal = getb("Internal").unwrap_or(false);
        let removable = getb("RemovableMedia").unwrap_or(false)
            || getb("Removable").unwrap_or(false)
            || !internal;
        let device = format!("/dev/{id}");
        let raw = format!("/dev/r{id}");
        let vendor = gets("MediaName").unwrap_or_default();
        let bus = gets("BusProtocol");
        let mount = gets("MountPoint").filter(|s| !s.is_empty());

        Some(DriveInfo {
            id: device.clone(),
            device,
            raw_device: Some(raw),
            description: if vendor.is_empty() { id.to_string() } else { vendor },
            size_bytes: size,
            removable,
            system: internal, // internal physical disks are treated as system
            bus_type: bus,
            mountpoints: mount.into_iter().collect(),
            read_only: getb("WritableMedia").map(|w| !w).unwrap_or(false),
        })
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use serde::Deserialize;
    use std::process::Command;

    #[derive(Deserialize)]
    struct LsblkRoot {
        blockdevices: Vec<Dev>,
    }
    #[derive(Deserialize)]
    struct Dev {
        name: String,
        #[serde(default)]
        size: Option<u64>,
        #[serde(rename = "type", default)]
        dtype: Option<String>,
        #[serde(default)]
        rm: Option<bool>,
        #[serde(default)]
        ro: Option<bool>,
        #[serde(default)]
        hotplug: Option<bool>,
        #[serde(default)]
        tran: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        vendor: Option<String>,
        #[serde(default)]
        mountpoint: Option<String>,
        #[serde(default)]
        children: Vec<Dev>,
    }

    pub fn list() -> Result<Vec<DriveInfo>> {
        let out = Command::new("lsblk")
            .args(["--json", "-b", "-o", "NAME,SIZE,TYPE,RM,RO,HOTPLUG,TRAN,MODEL,VENDOR,MOUNTPOINT"])
            .output()
            .map_err(|e| FlashError::Other(format!("lsblk: {e}")))?;
        let root: LsblkRoot = serde_json::from_slice(&out.stdout)
            .map_err(|e| FlashError::Other(format!("lsblk json: {e}")))?;

        let mut drives = Vec::new();
        for dev in root.blockdevices {
            if dev.dtype.as_deref() != Some("disk") {
                continue;
            }
            let mut mounts = Vec::new();
            let mut is_system = false;
            collect_mounts(&dev, &mut mounts, &mut is_system);
            let device = format!("/dev/{}", dev.name);
            let removable = dev.rm.unwrap_or(false) || dev.hotplug.unwrap_or(false);
            let desc = format!(
                "{} {}",
                dev.vendor.clone().unwrap_or_default().trim(),
                dev.model.clone().unwrap_or_default().trim()
            )
            .trim()
            .to_string();
            drives.push(DriveInfo {
                id: device.clone(),
                device: device.clone(),
                raw_device: None,
                description: if desc.is_empty() { dev.name.clone() } else { desc },
                size_bytes: dev.size.unwrap_or(0),
                removable,
                system: is_system || (!removable && dev.tran.as_deref() != Some("usb")),
                bus_type: dev.tran.clone().map(|t| t.to_uppercase()),
                mountpoints: mounts,
                read_only: dev.ro.unwrap_or(false),
            });
        }
        Ok(drives)
    }

    fn collect_mounts(dev: &Dev, out: &mut Vec<String>, system: &mut bool) {
        if let Some(mp) = &dev.mountpoint {
            if !mp.is_empty() {
                if mp == "/" || mp == "/boot" || mp.starts_with("/boot") {
                    *system = true;
                }
                out.push(mp.clone());
            }
        }
        for c in &dev.children {
            collect_mounts(c, out, system);
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use serde::Deserialize;
    use std::process::Command;

    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Disk {
        device_id: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        size: Option<serde_json::Value>,
        #[serde(default)]
        interface_type: Option<String>,
        #[serde(default)]
        media_type: Option<String>,
        #[serde(default)]
        index: Option<u32>,
    }

    pub fn list() -> Result<Vec<DriveInfo>> {
        // PowerShell CIM query; ConvertTo-Json yields an object for a single disk
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_DiskDrive | Select-Object DeviceID,Model,Size,InterfaceType,MediaType,Index | ConvertTo-Json -Compress",
            ])
            .output()
            .map_err(|e| FlashError::Other(format!("powershell: {e}")))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(vec![]);
        }
        let disks: Vec<Disk> = if trimmed.starts_with('[') {
            serde_json::from_str(trimmed)
        } else {
            serde_json::from_str::<Disk>(trimmed).map(|d| vec![d])
        }
        .map_err(|e| FlashError::Other(format!("cim json: {e}")))?;

        let mut drives = Vec::new();
        for d in disks {
            let size = match &d.size {
                Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
                Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
                _ => 0,
            };
            let media = d.media_type.clone().unwrap_or_default();
            let iface = d.interface_type.clone().unwrap_or_default();
            let removable = media.to_lowercase().contains("removable")
                || iface.eq_ignore_ascii_case("USB");
            drives.push(DriveInfo {
                id: d.device_id.clone(),
                device: d.device_id.clone(),
                raw_device: None,
                description: d.model.clone().unwrap_or_else(|| d.device_id.clone()),
                size_bytes: size,
                removable,
                system: d.index == Some(0) || (!removable),
                bus_type: Some(iface),
                mountpoints: vec![],
                read_only: false,
            });
        }
        Ok(drives)
    }
}
