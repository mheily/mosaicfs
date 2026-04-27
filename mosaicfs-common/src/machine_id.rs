//! Stable per-machine identity, used as a lookup key on node documents.
//!
//! Resolution order:
//!   1. `MOSAICFS_MACHINE_ID` env var — for dev/test environments where two
//!      processes on the same machine must not share a node identity.
//!   2. Platform hardware UUID:
//!        macOS   — `kern.uuid` via sysctlbyname(3) — the IOPlatformUUID,
//!                  read as a direct kernel call (works inside the app sandbox)
//!        Linux   — `/etc/machine-id` (readable without root)
//!        Windows — `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`
//!   3. Persisted random UUID at `~/.mosaicfs/node-id.toml` — generated once
//!      and reused on all subsequent calls when the platform source is unavailable.

use std::path::PathBuf;

/// Return this machine's stable ID.
pub fn get() -> String {
    if let Ok(id) = std::env::var("MOSAICFS_MACHINE_ID") {
        if !id.is_empty() {
            return id;
        }
    }
    platform_id()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(persisted_id)
}

// ── Platform sources ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn platform_id() -> Option<String> {
    // sysctlbyname is a direct kernel call — no subprocess, works in the
    // macOS app sandbox (unlike spawning the `sysctl` CLI tool).
    use std::ffi::CStr;
    let name = b"kern.uuid\0";
    let mut buf = [0u8; 64];
    let mut len = buf.len() as libc::size_t;
    let ret = unsafe {
        libc::sysctlbyname(
            name.as_ptr() as *const libc::c_char,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return None;
    }
    let s = unsafe { CStr::from_ptr(buf.as_ptr() as *const libc::c_char) }
        .to_str()
        .ok()?
        .to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(target_os = "linux")]
fn platform_id() -> Option<String> {
    std::fs::read_to_string("/etc/machine-id")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "windows")]
fn platform_id() -> Option<String> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()
        .ok()?;
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if line.contains("MachineGuid") {
                if let Some(guid) = line.split_whitespace().last() {
                    return Some(guid.to_string());
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_id() -> Option<String> {
    None
}

// ── Persistent fallback ───────────────────────────────────────────────────────

fn persisted_id() -> String {
    let path = persisted_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(table) = content.parse::<toml::Table>() {
            if let Some(id) = table.get("node_id").and_then(|v| v.as_str()) {
                if !id.is_empty() {
                    return id.to_string();
                }
            }
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, format!("node_id = \"{}\"\n", id));
    id
}

fn persisted_path() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mosaicfs")
        .join("node-id.toml")
}
