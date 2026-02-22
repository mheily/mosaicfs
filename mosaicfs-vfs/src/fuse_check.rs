//! Utility to detect whether FUSE is available on the current system.
//!
//! Other test modules can call [`fuse_available()`] to decide whether to skip
//! FUSE-dependent tests at runtime, or use the [`skip_without_fuse!`] macro.

use std::sync::OnceLock;

/// Returns `true` if the current environment supports FUSE mounts.
///
/// The result is cached after the first call so the probe runs at most once.
pub fn fuse_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(probe_fuse)
}

/// Minimal no-op filesystem used only for the FUSE probe.
struct ProbeFs;

impl fuser::Filesystem for ProbeFs {}

fn probe_fuse() -> bool {
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(_) => return false,
    };

    let mount_point = dir.path().to_path_buf();
    let options = vec![
        fuser::MountOption::RO,
        fuser::MountOption::FSName("mosaicfs_probe".to_string()),
        fuser::MountOption::AutoUnmount,
    ];

    // spawn_mount2 returns a BackgroundSession that unmounts on drop.
    let session = match fuser::spawn_mount2(ProbeFs, &mount_point, &options) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Give the mount a moment to register.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let mounted = is_mounted(&mount_point);

    // Dropping the session triggers unmount via AutoUnmount.
    drop(session);

    mounted
}

/// Check whether `path` appears as a mount point.
fn is_mounted(path: &std::path::Path) -> bool {
    // On Linux, check /proc/mounts.
    if let Ok(contents) = std::fs::read_to_string("/proc/mounts") {
        if let Some(path_str) = path.to_str() {
            return contents.contains(path_str);
        }
    }
    // Fallback: try `mount` command.
    if let Ok(output) = std::process::Command::new("mount").output() {
        if let (Ok(stdout), Some(path_str)) = (String::from_utf8(output.stdout), path.to_str()) {
            return stdout.contains(path_str);
        }
    }
    false
}

/// Convenience macro for skipping a test when FUSE is not available.
///
/// Place `skip_without_fuse!();` at the top of any test function body.
///
/// ```rust,ignore
/// #[test]
/// fn test_something_needing_fuse() {
///     mosaicfs_vfs::skip_without_fuse!();
///     // ... rest of test
/// }
/// ```
#[macro_export]
macro_rules! skip_without_fuse {
    () => {
        if !$crate::fuse_check::fuse_available() {
            eprintln!("FUSE not available – skipping test");
            return;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuse_probe_runs() {
        // Validates that the probe completes without panicking.
        // The actual boolean depends on the environment.
        let available = fuse_available();
        eprintln!("FUSE available: {available}");
    }

    #[test]
    fn test_fuse_probe_is_consistent() {
        // Calling twice should return the same cached result.
        let first = fuse_available();
        let second = fuse_available();
        assert_eq!(first, second);
    }
}
