# Change 015 — Design notes

Companion to `./architecture.md`. The architecture doc states the goal,
deltas, and phases. This doc covers the implementation specifics: exact
file contents, crate choices, the seccomp/Landlock allow-list with rationale
per entry, the bring-up runbook, and the install script.

## Crate choices

- **`nix`** for `statvfs` — already a common low-friction wrapper. Add at
  workspace level, used by `mosaicfs-agent`. (Avoids pulling raw `libc`
  bindings.)
- **`landlock`** (crates.io `landlock`) — the canonical Rust binding,
  maintained by the Landlock project. Use `ABI::new_current()` so the
  binary adapts to whatever ABI the running kernel supports.
- **`caps`** (crates.io `caps`) — small, stable, drops capabilities. Used
  only at startup; one call.
- **`seccompiler`** (crates.io `seccompiler`) — pure-Rust seccomp filter
  builder from the Firecracker/Cloud-Hypervisor lineage. Preferred over
  `libseccomp` because it has no C dependency, which keeps the static-link
  story simple and avoids shipping libseccomp on the NAS.

All four go under a Linux-only target table in `mosaicfs/Cargo.toml`:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
landlock     = "0.4"
caps         = "0.5"
seccompiler  = "0.4"
libc         = "0.2"   # for prctl(PR_SET_NO_NEW_PRIVS)
```

(Pin to whatever is current at implementation time; the major versions
above are correct as of 2026-Q1.)

`nix` lands in workspace deps and is consumed by `mosaicfs-agent`:

```toml
# workspace Cargo.toml
nix = { version = "0.29", features = ["fs"] }
```

## Phase 1 — `df` → `statvfs`

Replace the body of `check_storage_capacity` in
`mosaicfs-agent/src/start.rs:228`:

```rust
async fn check_storage_capacity(db: &CouchClient, node_id: &str, watch_paths: &[PathBuf]) {
    for watch_path in watch_paths {
        let path = watch_path.clone();
        let pct = tokio::task::spawn_blocking(move || {
            nix::sys::statvfs::statvfs(&path).ok().map(|s| {
                let total = s.blocks() as u64;
                let avail = s.blocks_available() as u64;
                if total == 0 { 0u32 } else { ((total - avail) * 100 / total) as u32 }
            })
        })
        .await
        .ok()
        .flatten();

        if let Some(pct) = pct {
            if pct >= 90 {
                notifications::emit_notification(
                    db, node_id, "storage", "storage_near_capacity",
                    "warning", "Storage near capacity",
                    &format!("Disk usage at {}% for {}.", pct, watch_path.display()),
                    None,
                ).await;
                return;
            }
        }
    }
    notifications::resolve_notification(db, node_id, "storage_near_capacity").await;
}
```

`statvfs` is a blocking syscall, so it goes through `spawn_blocking` to
keep the runtime non-blocking. Same threshold, same output path. macOS:
`statvfs` exists, behaves the same — no `cfg` gating needed.

## Phase 2 — Install footprint

### `deploy/systemd/mosaicfs.service` (Phase 2 minimal version)

```ini
[Unit]
Description=MosaicFS agent
Documentation=https://github.com/...
After=network-online.target couchdb.service
Wants=network-online.target

[Service]
Type=simple
User=mosaicfs
Group=mosaicfs
ExecStart=/usr/local/bin/mosaicfs --config /etc/mosaicfs/mosaicfs.toml
Restart=on-failure
RestartSec=5s
StartLimitIntervalSec=60s
StartLimitBurst=3

StateDirectory=mosaicfs
RuntimeDirectory=mosaicfs
LogsDirectory=mosaicfs

[Install]
WantedBy=multi-user.target
```

`Type=simple` (not `notify`) because the binary doesn't currently send
sd_notify readiness signals. If we want `Type=notify` later, that's a
separate change.

### `deploy/systemd/install.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

KERNEL_MAJOR=$(uname -r | cut -d. -f1)
KERNEL_MINOR=$(uname -r | cut -d. -f2)
if [ "$KERNEL_MAJOR" -lt 6 ] || { [ "$KERNEL_MAJOR" -eq 6 ] && [ "$KERNEL_MINOR" -lt 1 ]; }; then
    echo "ERROR: kernel $KERNEL_MAJOR.$KERNEL_MINOR < 6.1 — Landlock ABI v2 not guaranteed"
    exit 1
fi
grep -q landlock /sys/kernel/security/lsm || {
    echo "ERROR: Landlock LSM not enabled on this kernel"
    exit 1
}

# 1. system user
id mosaicfs &>/dev/null || \
    useradd --system --shell /usr/sbin/nologin --home-dir /nonexistent mosaicfs

# 2. directories (StateDirectory= will create /var/lib/mosaicfs, but
#    /etc/mosaicfs is on us)
install -d -o mosaicfs -g mosaicfs -m 0750 /etc/mosaicfs
install -d -o mosaicfs -g mosaicfs -m 0750 /var/lib/mosaicfs

# 3. binary
install -m 0755 target/release/mosaicfs /usr/local/bin/mosaicfs

# 4. config (only if missing — don't clobber)
[ -f /etc/mosaicfs/mosaicfs.toml ] || \
    install -m 0640 -o mosaicfs -g mosaicfs deploy/systemd/mosaicfs.example.toml /etc/mosaicfs/mosaicfs.toml

# 5. unit
install -m 0644 deploy/systemd/mosaicfs.service /etc/systemd/system/mosaicfs.service
systemctl daemon-reload

echo "Installed. Edit /etc/mosaicfs/mosaicfs.toml then: systemctl enable --now mosaicfs"
```

A minimal `deploy/systemd/mosaicfs.example.toml` ships alongside, with
`features.agent=true`, `features.web_ui=false`, `features.vfs=false`, the
state dir and watch_paths placeholders, and a CouchDB URL of
`http://127.0.0.1:5984`.

## Phase 3 — Add the systemd sandbox directives

Append to the `[Service]` section of `mosaicfs.service`:

```ini
# --- privilege ---
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=

# --- filesystem ---
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ReadWritePaths=/var/lib/mosaicfs /run/mosaicfs

# --- proc/kernel ---
ProtectProc=invisible
ProcSubset=pid
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes

# --- misc ---
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
RemoveIPC=yes
MemoryDenyWriteExecute=yes
UMask=0027

# --- resource caps ---
MemoryMax=512M
CPUQuota=75%
TasksMax=256
```

Notes:
- `MemoryDenyWriteExecute=yes` is safe with our stack (pure-Rust + rustls,
  no JIT). Watch for failures if a future dep adds JIT (`v8`, `wasmtime`,
  certain regex engines).
- `PrivateDevices=yes` blocks `/dev/fuse`. Fine today (no Linux FUSE
  serving). Listed as a future blocker in arch §6.
- No `DynamicUser=` — we want a stable UID for filesystem ownership of
  watch paths and state dir.

## Phase 4 — IP allow-list

Append:

```ini
IPAddressDeny=any
IPAddressAllow=127.0.0.0/8
IPAddressAllow=::1/128
```

Validation:

```bash
sudo -u mosaicfs curl -sS --max-time 2 http://1.1.1.1/        # blocked
sudo -u mosaicfs curl -sS --max-time 2 http://127.0.0.1:5984/ # OK
journalctl -u mosaicfs --since "1 minute ago" | grep -i ipaddress
```

## Phase 5 — `mosaicfs/src/sandbox.rs`

```rust
//! Linux process sandbox: applied early in main(), before subsystem spawn.
//!
//! Order matters: NoNewPrivs first (so subsequent steps can't be undone by
//! a setuid binary), then drop caps, then Landlock, then seccomp.
//! macOS is a no-op stub; the desktop App Sandbox covers that side.

use anyhow::{Context, Result};
use std::path::Path;

#[cfg(not(target_os = "linux"))]
pub fn apply(_watch_paths: &[std::path::PathBuf]) -> Result<()> { Ok(()) }

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
    if rc != 0 { anyhow::bail!("prctl failed: {}", std::io::Error::last_os_error()); }
    Ok(())
}

#[cfg(target_os = "linux")]
fn drop_capabilities() -> Result<()> {
    use caps::{CapSet, Capability};
    caps::clear(None, CapSet::Permitted)?;
    caps::clear(None, CapSet::Inheritable)?;
    caps::clear(None, CapSet::Effective)?;
    let remaining = caps::read(None, CapSet::Effective)?;
    if !remaining.is_empty() {
        anyhow::bail!("capabilities not fully dropped: {:?}", remaining);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_landlock(watch_paths: &[std::path::PathBuf]) -> Result<()> {
    use landlock::{
        Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr,
        RulesetCreatedAttr, ABI,
    };
    let abi = ABI::new_current();
    let ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?;

    let mut ruleset = ruleset
        // state, runtime: read+write
        .add_rule(PathBeneath::new(PathFd::new("/var/lib/mosaicfs")?,
            AccessFs::from_all(abi)))?
        .add_rule(PathBeneath::new(PathFd::new("/run/mosaicfs")?,
            AccessFs::from_all(abi)))?
        // config, certs, zoneinfo: read-only
        .add_rule(PathBeneath::new(PathFd::new("/etc/mosaicfs")?,
            AccessFs::from_read(abi)))?
        .add_rule(PathBeneath::new(PathFd::new("/etc/ssl/certs")?,
            AccessFs::from_read(abi)))?
        .add_rule(PathBeneath::new(PathFd::new("/usr/share/zoneinfo")?,
            AccessFs::from_read(abi)))?;

    // watch_paths: read-only
    for p in watch_paths {
        if let Ok(fd) = PathFd::new(p) {
            ruleset = ruleset.add_rule(
                PathBeneath::new(fd, AccessFs::from_read(abi))
            )?;
        } else {
            tracing::warn!(path = %p.display(),
                "Landlock: watch path missing at startup, not allow-listed");
        }
    }

    let status = ruleset.restrict_self()?;
    tracing::info!(?status, "Landlock applied");
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_seccomp() -> Result<()> {
    use seccompiler::{
        BpfProgram, SeccompAction, SeccompFilter, SeccompRule,
        TargetArch,
    };
    use std::collections::BTreeMap;

    // Default-allow with explicit denies. See design-notes §"Seccomp deny list"
    // for the rationale per syscall.
    let denied: &[i64] = &[
        libc::SYS_execve, libc::SYS_execveat,
        libc::SYS_ptrace,
        libc::SYS_mount, libc::SYS_umount2,
        libc::SYS_bpf,
        libc::SYS_kexec_load, libc::SYS_kexec_file_load,
        libc::SYS_init_module, libc::SYS_finit_module, libc::SYS_delete_module,
        libc::SYS_unshare, libc::SYS_setns,
        libc::SYS_keyctl, libc::SYS_add_key, libc::SYS_request_key,
        libc::SYS_pivot_root, libc::SYS_chroot,
        libc::SYS_perf_event_open,
    ];

    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    for s in denied { rules.insert(*s, vec![]); }

    let action = if std::env::var("MOSAICFS_SECCOMP_LOG").is_ok() {
        SeccompAction::Log
    } else {
        SeccompAction::Errno(libc::EPERM as u32)
    };

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,    // mismatch (allowed by default)
        action,                  // match (denied / logged)
        TargetArch::x86_64,      // adjust for aarch64 NAS at install time
    )?;
    let program: BpfProgram = filter.try_into()?;
    seccompiler::apply_filter(&program)?;
    tracing::info!("seccomp filter applied");
    Ok(())
}
```

Wired into `mosaicfs/src/main.rs` after config load and before the
`JoinSet` spawn loop:

```rust
// after `cfg` is loaded
sandbox::apply(&cfg.agent.as_ref().map(|a| a.watch_paths.clone()).unwrap_or_default())?;
```

(Exact field path TBD against current config struct; the design-notes
review pass will pin it.)

## Seccomp deny list — rationale per syscall

| Syscall | Why deny |
|---|---|
| `execve`, `execveat` | Block all program execution. After Phase 1 the agent has no `execve` callsite. |
| `ptrace` | Prevent debugging/injecting other processes. |
| `mount`, `umount2` | Prevent constructing arbitrary filesystem views. |
| `pivot_root`, `chroot` | Same. Closes the namespace-escape toolkit. |
| `bpf` | No legitimate use; blocks loading rootkit/eBPF programs. |
| `kexec_load`, `kexec_file_load` | Boot a new kernel — root-only but defense-in-depth. |
| `init_module`, `finit_module`, `delete_module` | Module loading — already blocked by `ProtectKernelModules`, but seccomp covers the syscall numbers directly. |
| `unshare`, `setns` | Namespace manipulation; container-style escape primitive. |
| `keyctl`, `add_key`, `request_key` | Kernel keyring access; not used; can stash secrets across processes. |
| `perf_event_open` | CPU side-channels and kernel introspection. |

**Not denied (intentional):**

- `clone`, `clone3`, `fork`, `vfork` — tokio's multi-threaded runtime needs
  these. Process isolation is handled by `RestrictNamespaces=yes` and the
  capability bound; a clone without `CAP_SYS_ADMIN` cannot create new user
  namespaces.
- `socket`, `connect`, `bind`, `accept` — needed; egress restriction is
  handled by `IPAddressDeny/Allow`.
- `openat`, `readat`, etc. — Landlock covers filesystem.

## Bring-up runbook

This is the procedure when first deploying to the NAS, regardless of which
phase is being verified.

1. **Build:** `cargo build --release` on a build host (or the NAS itself).
2. **Stage:** `scp target/release/mosaicfs deploy/systemd/* nas:/tmp/mfs/`.
3. **Install:** `ssh nas 'cd /tmp/mfs && sudo ./install.sh'`.
4. **Configure:** `sudo -e /etc/mosaicfs/mosaicfs.toml` — set watch paths,
   CouchDB URL `http://127.0.0.1:5984`, and inline secrets.
5. **First start:** `sudo systemctl enable --now mosaicfs`.
6. **Watch:** `journalctl -u mosaicfs -f` for ≥ 5 minutes — must see one
   heartbeat (30 s) and one storage-capacity check (300 s) without errors.
7. **Score:** `systemd-analyze security mosaicfs.service` — record the
   exposure number per phase. Expected: Phase 2 ≈ 9, Phase 3 ≈ 2.0,
   Phase 4 ≈ 1.5.
8. **Validate isolation:** run the smoke tests in arch §"Verification".

### Phase 6 seccomp-specific runbook

The seccomp phase has a non-obvious bring-up sequence:

1. Install with `Environment=MOSAICFS_SECCOMP_LOG=1` in the unit file (or
   via drop-in). This sets the action to `SeccompAction::Log`.
2. Run for at least one full health-check cycle — the inotify and
   storage-capacity checks fire every 300 s, so wait 600 s to see two cycles.
3. Harvest hit syscalls: `journalctl -u mosaicfs --since '10 minutes ago'
   | grep SECCOMP | awk '{...}' | sort -u`. This produces the actual
   syscall set the agent uses under load.
4. If any hit syscall is in our deny list, that's a real problem — file a
   bug, the design needs revisiting. (We don't expect any after Phase 1's
   `df` removal.)
5. Remove the `MOSAICFS_SECCOMP_LOG` env var; restart; verify zero seccomp
   denials over 24 h.

## Configuration changes for the agent-only NAS

The TOML on the NAS:

```toml
[features]
agent = true
web_ui = false
vfs = false

[agent]
state_dir = "/var/lib/mosaicfs"
watch_paths = ["/srv/storage/photos", "/srv/storage/documents"]

[secrets]
backend = "inline"
[secrets.inline]
couchdb_url = "http://127.0.0.1:5984"
couchdb_user = "mosaicfs"
couchdb_password = "..."
```

(Schema verified against `mosaicfs-common/src/config.rs` during design-notes
review pass.)

## Testing strategy

- **Phase 1 (statvfs):** unit-style — call the new function against a
  temp directory and assert the percentage matches `df --output=pcent`'s
  output ± 1%. Manual: tail journalctl, confirm storage warnings still
  trigger when a watch_path nears 90%.
- **Phase 2 (install):** Debian 12 VM clean install + `install.sh` →
  `systemctl status mosaicfs` is active. No automated test.
- **Phase 3 (sandbox):** the smoke-test list in arch §"Verification" runs
  as a shell script under `deploy/systemd/test-isolation.sh` (added in
  Phase 3).
- **Phase 4 (IP):** added to the same shell script. `curl 1.1.1.1`
  blocked, `curl 127.0.0.1:5984` reachable.
- **Phase 5 (Landlock+caps):** Linux-only `#[test]` in `mosaicfs/tests/`
  that, in a child process, calls `sandbox::apply` and then attempts to
  `open("/etc/shadow")` — must return EACCES.
- **Phase 6 (seccomp):** Linux-only test that applies the filter and
  attempts `Command::new("/bin/true").spawn()` — must fail.

## Resolved decisions

The five items raised during architecture review are resolved below. They
informed the final shape of this design and are recorded so the rationale
is preserved.

### 1. `sandbox` module lives in the `mosaicfs` crate

It's binary-specific init code — invoked once from `main()` before any
subsystem is spawned. Putting it in `mosaicfs-common` would imply reuse
across binaries, but there is only one binary today and the project's
unified-binary direction means there will continue to be one. Keeping it
next to `main.rs` makes the call ordering self-evident in code review.

### 2. Architecture detected at compile time

`seccompiler::TargetArch` is a build-time enum, so the filter is built
behind `cfg!(target_arch = ...)`:

```rust
let target = if cfg!(target_arch = "x86_64") {
    TargetArch::x86_64
} else if cfg!(target_arch = "aarch64") {
    TargetArch::aarch64
} else {
    anyhow::bail!("seccomp: unsupported target_arch");
};
```

x86_64 and aarch64 are the two we care about (Linux NAS hardware in 2026
is one or the other). The build itself is per-architecture, so the agent
binary on the NAS gets a filter native to its arch — no runtime detection
needed.

### 3. `linux-host.md` includes a CouchDB pointer

Upstream Apache CouchDB ships `.deb` packages for Debian 12 via
`https://couchdb.apache.org/`. The deployment doc will give the two-line
install (`curl | gpg --dearmor`, `apt install couchdb`) plus a note to
configure CouchDB to bind only `127.0.0.1:5984`. Out-of-scope for the unit
file itself but in-scope for the deployment runbook so the developer
doesn't have to assemble it from scratch.

### 4. Watch paths are static for the Linux agent — no Landlock workaround needed

Confirmed by reading `mosaicfs-agent/src/watch_path_provider.rs:14` and
`mosaicfs-agent/src/start.rs:59,150`: on Linux/headless deployments the
`BareWatchPathProvider` is used, which just returns the config-loaded
`agent.watch_paths` with no-op guards. The dynamic NSOpenPanel / scoped
bookmark flow added by change 014 is desktop-only (macOS).
`mosaicfs-server/src/handlers/nodes.rs:142` updates a CouchDB doc but does
not change what the running agent watches — that comes from local TOML.

Implication for this design: Landlock can pin the exact watch_paths list
at startup. Adding or changing a watch path requires `sudoedit
/etc/mosaicfs/mosaicfs.toml && systemctl restart mosaicfs` — which is
already the deployment model and surfaces in `linux-host.md`. No umbrella
allow needed.

### 5. `MemoryDenyWriteExecute=yes` is staying — verified at Phase 3 bring-up

`aws-lc-rs` (the rustls crypto provider in `mosaicfs/src/main.rs:26-28`)
is a FIPS-style C library with no JIT. It does call `mprotect`, but for
`PROT_READ|PROT_EXEC` (loading code) and `PROT_READ|PROT_WRITE` (data),
not the simultaneously-writable-and-executable mapping that
`MemoryDenyWriteExecute` rejects. We expect compatibility.

Risk mitigation: Phase 3 bring-up adds a single explicit step — start the
agent with the directive in place, run for one heartbeat cycle (30 s),
confirm no `EPERM` from `mprotect` in `journalctl`. If it fails, the
fallback is to drop the directive and rely on the rest of the layer
(seccomp blocks `bpf` and `kexec_*`; Landlock blocks unintended writes).
Phase 3 verification step:

```bash
sudo systemctl restart mosaicfs
sleep 35
journalctl -u mosaicfs --since "1 minute ago" | grep -iE 'mprotect|operation not permitted' \
  && echo "FAIL: MDWE incompatibility" \
  || echo "OK: MDWE compatible"
```
