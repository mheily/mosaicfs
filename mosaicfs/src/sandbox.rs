//! Linux process sandbox: applied early in main(), before subsystem spawn.
//!
//! Order: NoNewPrivs first (so subsequent steps can't be undone by a setuid
//! binary), then drop caps, then Landlock, then seccomp (added in phase 6).
//! macOS is a no-op stub; the desktop App Sandbox covers that side.

use anyhow::Result;
#[cfg(target_os = "linux")]
use anyhow::Context;

#[cfg(not(target_os = "linux"))]
pub fn apply(_watch_paths: &[std::path::PathBuf]) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn apply(watch_paths: &[std::path::PathBuf]) -> Result<()> {
    set_no_new_privs().context("PR_SET_NO_NEW_PRIVS")?;
    drop_capabilities().context("drop capabilities")?;
    apply_landlock(watch_paths).context("Landlock")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_no_new_privs() -> Result<()> {
    // safe: prctl is a single syscall with no aliasing
    let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1u64, 0u64, 0u64, 0u64) };
    if rc != 0 {
        anyhow::bail!("prctl failed: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn drop_capabilities() -> Result<()> {
    use caps::{CapSet, CapsHashSet};
    caps::set(None, CapSet::Permitted, &CapsHashSet::new())?;
    caps::set(None, CapSet::Inheritable, &CapsHashSet::new())?;
    caps::set(None, CapSet::Effective, &CapsHashSet::new())?;
    let remaining = caps::read(None, CapSet::Effective)?;
    if !remaining.is_empty() {
        anyhow::bail!("capabilities not fully dropped: {:?}", remaining);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_landlock(watch_paths: &[std::path::PathBuf]) -> Result<()> {
    use landlock::{
        Access, AccessFs, ABI, PathBeneath, PathFd, Ruleset, RulesetAttr,
        RulesetCreatedAttr,
    };

    let abi = ABI::new_current();
    let ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?;

    let mut ruleset = ruleset
        // state, runtime: read+write
        .add_rule(PathBeneath::new(PathFd::new("/var/lib/mosaicfs")?, AccessFs::from_all(abi)))?
        .add_rule(PathBeneath::new(PathFd::new("/run/mosaicfs")?, AccessFs::from_all(abi)))?
        // config, certs, zoneinfo: read-only
        .add_rule(PathBeneath::new(PathFd::new("/etc/mosaicfs")?, AccessFs::from_read(abi)))?
        .add_rule(PathBeneath::new(PathFd::new("/etc/ssl/certs")?, AccessFs::from_read(abi)))?
        .add_rule(PathBeneath::new(PathFd::new("/usr/share/zoneinfo")?, AccessFs::from_read(abi)))?;

    // watch_paths: read-only
    for p in watch_paths {
        if let Ok(fd) = PathFd::new(p) {
            ruleset = ruleset.add_rule(PathBeneath::new(fd, AccessFs::from_read(abi)))?;
        } else {
            tracing::warn!(path = %p.display(), "Landlock: watch path missing at startup, not allow-listed");
        }
    }

    let status = ruleset.restrict_self()?;
    tracing::info!(?status, "Landlock applied");
    Ok(())
}
