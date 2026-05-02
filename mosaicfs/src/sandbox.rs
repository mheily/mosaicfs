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
    apply_seccomp().context("seccomp")?;
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

    // V3 = kernel 6.2+ (truncate control). BestEffort compatibility (the
    // default) means older kernels silently apply only what they support.
    // install.sh enforces kernel ≥ 6.1, so V2 is the floor in practice.
    let abi = ABI::V3;
    let ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?;

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/run/mosaicfs".to_owned());

    let mut ruleset = ruleset
        // state, runtime: read+write
        .add_rule(PathBeneath::new(PathFd::new("/var/lib/mosaicfs")?, AccessFs::from_all(abi)))?
        .add_rule(PathBeneath::new(PathFd::new(runtime_dir.as_str())?, AccessFs::from_all(abi)))?
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

#[cfg(target_os = "linux")]
fn apply_seccomp() -> Result<()> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;

    // Default-allow with explicit denies. See design-notes §"Seccomp deny list".
    // An empty condition list means "deny this syscall unconditionally".
    let denied: &[i64] = &[
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_ptrace,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_bpf,
        libc::SYS_kexec_load,
        libc::SYS_kexec_file_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_perf_event_open,
    ];

    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
    for s in denied {
        rules.insert(*s, vec![]);
    }

    let deny_action = if std::env::var("MOSAICFS_SECCOMP_LOG").is_ok() {
        SeccompAction::Log
    } else {
        SeccompAction::Errno(libc::EPERM as u32)
    };

    let target = if cfg!(target_arch = "x86_64") {
        TargetArch::x86_64
    } else if cfg!(target_arch = "aarch64") {
        TargetArch::aarch64
    } else {
        anyhow::bail!("seccomp: unsupported target_arch");
    };

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow, // mismatch: allow by default
        deny_action,          // match: deny (or log)
        target,
    )?;
    let program: BpfProgram = filter.try_into()?;
    seccompiler::apply_filter(&program)?;
    tracing::info!("seccomp filter applied");
    Ok(())
}
