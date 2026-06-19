//! Detect SSH public keys in the user's ~/.ssh directory to offer as autofill
//! on the customization screen.

use std::fs;
use std::path::PathBuf;

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

pub fn detect_ssh_keys() -> Vec<String> {
    let mut keys = Vec::new();
    let Some(home) = home_dir() else {
        return keys;
    };
    let ssh = home.join(".ssh");
    let Ok(entries) = fs::read_dir(&ssh) else {
        return keys;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("pub") {
            if let Ok(content) = fs::read_to_string(&path) {
                let line = content.trim();
                if is_public_key(line) {
                    keys.push(line.to_string());
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn is_public_key(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    matches!(
        parts.next(),
        Some(t)
            if t == "ssh-ed25519"
                || t == "ssh-rsa"
                || t == "ssh-dss"
                || t.starts_with("ecdsa-sha2-")
                || t.starts_with("sk-ssh-")
                || t.starts_with("sk-ecdsa-")
    ) && parts.next().is_some()
}
