//! Build the AWCFG `airwaves-install.json` document. The field set matches
//! exactly what `airwaves-preconfig` consumes on first boot: hostname,
//! wifi{ssid,psk}, channel, timezone, ssh_keys. The unattended-install flags
//! are written with their safe defaults (currently inert in the OS).
//!
//! Writing it onto the device is handled by `provision` (which creates the
//! AWCFG partition + FAT filesystem on the raw device).

use crate::model::Customization;
use serde_json::json;

/// Render the AWCFG `airwaves-install.json` document.
pub fn build_install_json(c: &Customization) -> String {
    let (ssid, psk) = if c.wifi_enabled {
        (c.wifi.ssid.clone(), c.wifi.psk.clone())
    } else {
        (String::new(), String::new())
    };
    let ssh_keys = if c.ssh_enabled { c.ssh_keys.clone() } else { vec![] };

    let doc = json!({
        "schema": 1,
        "hostname": c.hostname.trim().to_lowercase(),
        "wifi": { "ssid": ssid, "psk": psk },
        "channel": c.channel,
        "timezone": c.timezone,
        "ssh_keys": ssh_keys,
        "auto_install": false,
        "confirm_destructive": false,
        "target": "auto",
        "ab": false,
        "self_install": false,
        "post_install": "poweroff"
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}
