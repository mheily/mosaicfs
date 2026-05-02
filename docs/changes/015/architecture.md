# Change 015 — Architecture: Hardened host-OS Linux deployment

## Context

The intent doc (`./intent.md`) and the Haiku-generated security summary in
this directory propose moving the Linux deployment off the existing Podman
pod and onto bare metal as a hardened systemd service. The motivation is
partly hardening (defense in depth on a personal NAS) and partly educational
— the developer wants to learn Landlock, seccomp, and systemd sandboxing
first-hand.

This document is the authoritative design and supersedes
`./mosaicfs_linux_security_summary.md`, which had real value as a starting
point but contained several codebase-incorrect claims (separate
`mosaicfs-agent` binary, FUSE/NFS serving on Linux, blocking `clone`/`fork`
in seccomp, `AuditControl=yes` directive).

## Open questions resolved with the developer

- **CouchDB location:** Same NAS, separate systemd unit. Agent reaches it on
  `127.0.0.1:5984`. The IP allow-list only needs loopback.
- **Role on this NAS:** Agent only — `features.agent=true`, `web_ui=false`,
  `vfs=false`. No port 8443 exposed; no LAN ingress at all.
- **Target kernel:** Debian 12 / Ubuntu 24.04 (kernel 6.1+). All sandbox
  primitives (Landlock ABI v2+, cgroup-eBPF `IPAddress*`, full systemd
  directive set) are available.

## 1. Current State Summary

This is the inventory the design rests on. Anything in here that turns out to
be wrong invalidates the design — flag it before implementing.

### Workspace and binaries

- The workspace produces **one binary**: `mosaicfs` (`mosaicfs/src/main.rs`,
  242 lines). Library crates: `mosaicfs-common`, `mosaicfs-agent`,
  `mosaicfs-vfs`, `mosaicfs-server`.
- "Agent" is **not a separate binary** — it is a feature flag inside the
  unified binary. `mosaicfs/src/main.rs` reads `[features]` from
  `/etc/mosaicfs/mosaicfs.toml` and conditionally calls `start_agent()`,
  `start_vfs()`, `start_web_ui()` as separate tokio tasks
  (`mosaicfs/src/main.rs:66-88`).
- This matches the project-decisions direction of consolidating around one
  binary. The systemd unit must therefore invoke `/usr/local/bin/mosaicfs
  --config ...`, not a fictional `mosaicfs-agent` binary.

### Agent runtime profile (Linux)

- Entry: `start_agent()` in `mosaicfs-agent/src/start.rs:31`.
- Runtime: `#[tokio::main]` multi-threaded (`tokio = { features = ["full"] }`
  in `Cargo.toml:22`). **Spawns worker threads via `clone`/`clone3`.**
- Filesystem footprint:
  - Reads: `/etc/mosaicfs/mosaicfs.toml` (default, overridable;
    `mosaicfs/src/main.rs:22`), the configured `watch_paths` (read-only).
  - Writes: `/var/lib/mosaicfs` (state dir, default
    `mosaicfs-agent/src/start.rs:21`); contains `node_id` and the replication
    SQLite DB.
- Network footprint: HTTP to CouchDB only (URL from secrets;
  `mosaicfs-agent/src/start.rs:51-56`). No other outbound traffic.
- Child processes: **one** — `tokio::process::Command::new("df")` in the
  storage-capacity health check (`mosaicfs-agent/src/start.rs:230`), called
  every 300 s. This is the only `execve` in the agent path.
- FUSE/NFS/SMB: **none on Linux today.** `mosaicfs-vfs` is feature-gated and
  stubbed; the agent does not serve any filesystem protocol. No `/dev/fuse`
  dependency.
- Existing security code: **none.** No `seccomp`, `landlock`, `prctl`, or
  capability calls anywhere in the workspace. The macOS App Sandbox is
  applied to the desktop app only (`desktop/Entitlements.plist`), not to the
  agent process. Greenfield for hardening.
- macOS portability of the startup path: shared except for one `cfg(linux)`
  block in `mosaicfs-agent/src/start.rs:201-224` (inotify check) and one
  `cfg(target_os = "macos")` block in `mosaicfs-vfs/src/tiered_access.rs:454-476`.
  New hardening code added by this change must follow the same `cfg`-gating
  pattern so macOS is unaffected.

### Current deployment

- `deploy/mosaicfs.yaml`: a Podman/Kubernetes pod with two containers
  (`couchdb`, `mosaicfs`). The `mosaicfs` container runs as root, mounts
  `/var/lib/mosaicfs` from a PVC, and reads `/etc/mosaicfs` from the host.
- `Dockerfile.mosaicfs`: Debian-bookworm-slim base, `libfuse2` runtime dep
  (carried for future VFS work, not actively used), entrypoint
  `/usr/local/bin/mosaicfs`.
- `scripts/start-dev-environment`: macOS+Linux dev harness that runs the
  same Podman pod. There is **no host-native Linux dev path today**.

## 2. Goal

Run the unified `mosaicfs` binary on a personal Linux NAS as a hardened
systemd service, with all features except `agent` disabled, talking to a
co-located CouchDB instance over loopback. The agent's blast radius if
compromised is reduced to: read-only access to the configured watch paths
plus read/write access to `/var/lib/mosaicfs`. Net new outbound: nothing
besides loopback.

This is one moving part: introduce a host-OS deployment option for Linux
with sandbox layers added incrementally. The existing Podman deployment is
not removed and remains the dev/CI path.

## 3. Changes (deltas)

### 3.1 Replace the agent's only child-process spawn

- **Today:** Storage capacity health check shells out to `df` via
  `tokio::process::Command::new("df")` at `mosaicfs-agent/src/start.rs:230`.
- **Proposed:** Replace with a direct `statvfs()` syscall (`nix::sys::statvfs`
  or `libc::statvfs`) over each `watch_path`. Same outputs (free bytes,
  total bytes, percent used) without forking a process.
- **Justification:** This is the only `execve` in the agent path. Removing it
  lets seccomp deny `execve`/`execveat` outright with no allow-list needed,
  which is much simpler and less error-prone than argument-filtering. Also a
  small win independent of hardening: avoids parsing `df`'s human-formatted
  output.

### 3.2 Add a Linux host-OS install footprint

- **Today:** No host-native install. The `mosaicfs` binary lives inside a
  container image; there is no system user, no `/usr/local/bin` install
  path, no systemd unit, no host-side state dir owner.
- **Proposed:** Add to the repo:
  - `deploy/systemd/mosaicfs.service` — the hardened unit file.
  - `deploy/systemd/install.sh` (or `make install-linux-host` target) — a
    repeatable script that creates the `mosaicfs` system user, lays down
    `/var/lib/mosaicfs`, `/etc/mosaicfs/`, `/run/mosaicfs/` with correct
    ownership, copies the binary, installs the unit, and runs the kernel
    precheck.
  - A short `docs/deployment/linux-host.md` explaining the install and the
    things `systemd-analyze security` and `journalctl` will report.
- **Justification:** The intent is to deploy this. The repo currently has
  no path that produces a working host install, so deployment is undefined.
  Tracking the unit in the repo also makes it reviewable and diff-able as
  the hardening tightens.

### 3.3 Add systemd sandbox layer (no code changes)

- **Today:** Container is the only isolation; agent runs as root inside it.
- **Proposed:** The unit file applies the standard systemd hardening
  directive set: `NoNewPrivileges`, `CapabilityBoundingSet=` (empty),
  `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp`, `PrivateDevices`,
  `ProtectProc=invisible`, `ProcSubset=pid`, `ProtectKernel{Tunables,Modules,
  Logs}`, `ProtectControlGroups`, `ProtectClock`, `ProtectHostname`,
  `RestrictNamespaces`, `RestrictRealtime`, `RestrictSUIDSGID`,
  `MemoryDenyWriteExecute`, `LockPersonality`, `RemoveIPC`, plus
  `ReadWritePaths=/var/lib/mosaicfs /run/mosaicfs` and resource caps
  (`MemoryMax`, `CPUQuota`).
- **Justification:** Free hardening — pure unit-file directives, no Rust
  changes. Each maps to a documented kernel primitive. Failure mode is
  visible (`systemd-analyze security mosaicfs.service`) and reversible (edit
  the unit).
- **Drop from the Haiku draft:** `AuditControl=yes` (not a real systemd
  directive — would be silently ignored or fail to load).

### 3.4 Add network egress allow-list

- **Today:** Agent has unrestricted egress.
- **Proposed:** `IPAddressDeny=any` plus `IPAddressAllow=127.0.0.0/8` and
  `IPAddressAllow=::1/128`. With CouchDB co-located, that's the entire allow
  list. No LAN ingress is needed (no web UI on this NAS).
- **Justification:** Eliminates the data-exfil and C2 paths in one cgroup
  eBPF rule. Cheap because of the co-located-CouchDB decision.

### 3.5 Add in-code privilege drop and Landlock filesystem sandbox

- **Today:** No in-process sandboxing. The unified binary loads config,
  spawns subsystems, and runs.
- **Proposed:** Early in `mosaicfs/src/main.rs` (after config load, before
  spawning subsystems), on Linux only, call:
  1. `prctl(PR_SET_NO_NEW_PRIVS)` — belt-and-braces with the systemd flag.
  2. Drop all capabilities from permitted/inheritable sets.
  3. Apply Landlock with: `/var/lib/mosaicfs` rw, `/run/mosaicfs` rw,
     `/etc/mosaicfs` ro, configured watch paths ro, plus the minimum needed
     for `ca-certificates`/`/usr/lib` reads.
  Implemented in a new `mosaicfs/src/sandbox.rs` (or a `sandbox` module in
  `mosaicfs-common`) with Linux-cfg-gated functions and a `cfg(target_os =
  "linux")` import in `main.rs`. Not in `mosaicfs-agent` because it must run
  before any subsystem starts.
- **Justification:** Layered defense — if a CVE in a Rust dep gives an
  attacker code-exec, Landlock still bounds the filesystem reach to the
  config-declared paths. Per-process and tunable from config; cheaper than
  AppArmor.

### 3.6 Add seccomp syscall allow-list

- **Today:** No syscall filtering.
- **Proposed:** A default-allow seccomp filter that explicitly denies
  process-creation and kernel-modification syscalls: `execve`, `execveat`,
  `ptrace`, `mount`, `umount2`, `bpf`, `kexec_load`, `kexec_file_load`,
  `init_module`, `finit_module`, `delete_module`, `unshare`, `setns`,
  `keyctl`, `add_key`, `request_key`. **Critically, `clone`, `clone3`, and
  `fork` are NOT blocked** — tokio needs them.
- **Bring-up policy:** First deployment uses `SCMP_ACT_LOG` (log without
  blocking) for at least one full health-check cycle; harvest the syscall
  set from `journalctl`; then flip to `SCMP_ACT_ERRNO`.
- **Justification:** The Haiku doc's blanket fork/clone block was a
  show-stopper bug — tokio's multi-threaded runtime would not start. The
  log-first bring-up is the standard way to avoid the same class of bug
  for syscalls we haven't anticipated (`getrandom`, `statx`, `prlimit64`,
  `rseq`, etc.).

## 4. Implementation Phases

Phases are organized by topical concern, in the order that prep precedes
hardening. Per project rules, intermediate states need not be deployable;
only the final state must work.

### Phase 1 — Replace `df` with `statvfs`

- Touch: `mosaicfs-agent/src/start.rs` (storage capacity check), workspace
  `Cargo.toml` (add `nix` if not already present).
- Verify: existing health-check log lines still appear, with the same
  fields. No process-spawn path remains in the agent; grep for `Command::new`
  in `mosaicfs-agent/` returns nothing.

### Phase 2 — Linux host install footprint

- Add: `deploy/systemd/mosaicfs.service` (minimal — User/Group/ExecStart/
  Restart/StateDirectory only, no hardening yet), `deploy/systemd/install.sh`,
  `docs/deployment/linux-host.md`.
- Verify: on a fresh Debian 12 VM, run the installer; agent registers in the
  co-located CouchDB; `systemctl status mosaicfs` shows running as user
  `mosaicfs`.

### Phase 3 — systemd sandbox directives

- Edit: `deploy/systemd/mosaicfs.service` adding the directive set listed in
  3.3.
- Verify: agent still runs and registers; `systemd-analyze security
  mosaicfs.service` exposure score drops from baseline (~9) to under ~3.

### Phase 4 — Network egress allow-list

- Edit: `deploy/systemd/mosaicfs.service` adding `IPAddressDeny=any` and the
  loopback allows.
- Verify: agent still reaches CouchDB on 127.0.0.1; `sudo -u mosaicfs curl
  http://1.1.1.1` blocked; journalctl shows cgroup denials for unintended
  egress attempts.

### Phase 5 — In-code privilege drop + Landlock

- Add: `mosaicfs/src/sandbox.rs` with `set_no_new_privs`, `drop_capabilities`,
  `apply_landlock`. Wire into `mosaicfs/src/main.rs` after config load,
  before subsystem spawn. Add `caps`, `landlock` deps. `cfg(target_os =
  "linux")` gating.
- Verify: agent starts on Linux; `cat /proc/$(pgrep mosaicfs)/status | grep
  Cap` shows zero caps; attempts to read `/root/.ssh/id_rsa` (or any
  non-allowlisted path) fail with EACCES; macOS unaffected (`cfg`-gated
  out).

### Phase 6 — seccomp filter

- Add: seccomp filter setup in `sandbox.rs`, called last in the init
  sequence. Add `seccompiler` (pure-Rust, preferred over `libseccomp` to
  avoid C dep) or `libseccomp` to deps.
- Bring-up: deploy first with `SCMP_ACT_LOG`; observe one full
  health-check cycle (≥ 5 minutes); enumerate hit syscalls from journalctl;
  add any missing ones to the allow-list; redeploy with `SCMP_ACT_ERRNO`.
- Verify: agent runs steady-state for 24 h with no seccomp denials in
  journal. Manual smoke test of a forbidden syscall (e.g. inject `execve`
  via test harness) is denied.

### Cross-phase dependencies

- Phase 6 depends on Phase 1 (no `execve` in the agent path).
- Phases 5 and 6 depend on Phase 2 (something to deploy onto).
- Phases 3 and 4 are independent of 5 and 6 and could in principle run in
  parallel, but ordering simplifies bisecting if something breaks.

## 5. What Does Not Change

- **The Podman deployment.** `deploy/mosaicfs.yaml` and
  `Dockerfile.mosaicfs` are kept as the dev/test path and as a fallback.
  This change adds a deployment option, it does not remove one.
- **The macOS path.** All new Rust code is `cfg(target_os = "linux")`-gated
  or no-op on macOS. The desktop App Sandbox, Keychain backend, and
  `start-dev-environment` script are untouched.
- **The unified binary structure.** No new binary, no new crate. Sandbox
  code lives in a new module inside the existing `mosaicfs` crate.
- **The agent's logic.** Crawler, replication, heartbeat, node-registration,
  inotify check — all unchanged. Only the storage-capacity check (Phase 1)
  and the startup wrapper (Phase 5) touch agent-adjacent code.
- **CouchDB.** The CouchDB unit is out of scope for this change — the
  developer will install it separately from upstream packages.
- **The REST API and `mosaicfs-server` crate.** Not deployed on this NAS.
- **The web UI.** Disabled by config on this NAS; no port exposed.

## 6. Deferred

Each item is named so it doesn't get lost, with a one-line reason it isn't
needed now.

- **AppArmor profile.** systemd + Landlock + seccomp already cover the same
  ground; AppArmor adds a second policy file to maintain.
- **Kernel Lockdown LSM (boot param).** Requires NAS reboot and a
  system-wide policy decision; revisit once v1 is operational.
- **BPF LSM.** Only worthwhile if we anticipate JIT/mmap-based attacks; we
  don't.
- **auditd integration.** `journalctl` capture of LSM and seccomp denials is
  enough for a personal NAS; auditd is heavyweight.
- **Multi-CouchDB peers.** With CouchDB co-located, the allow-list is just
  loopback. Adding remote peers is one extra `IPAddressAllow=` line — defer
  until a second peer actually exists.
- **Linux Web UI exposure.** This NAS is headless. If a future NAS deployment
  wants the UI, it will need LAN-ingress allow-list rules and TLS choices —
  out of scope.
- **Linux FUSE adapter.** None today; if added later, `PrivateDevices=yes`
  and `NoNewPrivileges=yes` will need rework (FUSE wants `/dev/fuse` and
  often `fusermount` setuid).
- **macOS hardening parity.** Different primitives (sandbox-exec / App
  Sandbox profile for the agent process) — separate change.
- **Distroless / AppImage / `.deb` packaging.** Hand-installed `make
  install-linux-host` is fine for one personal NAS; revisit if distribution
  becomes a goal.
- **Per-deployment Landlock tuning via TOML.** Hard-coded paths are fine
  for v1; configurable allow-list is YAGNI until a second deployment exists
  with different paths.
- **Kernel-version fallback path** (Landlock-less older kernels). The target
  is 6.1+; if a future user hits an older kernel, they fall back to the
  Podman deployment.

## Verification of the final state

Once all phases are merged, on the NAS:

1. `systemctl status mosaicfs` — active (running) under user `mosaicfs`.
2. `systemd-analyze security mosaicfs.service` — exposure ≤ 1.5.
3. `cat /proc/$(pgrep mosaicfs)/status | grep ^Cap` — all zeros.
4. `cat /proc/$(pgrep mosaicfs)/status | grep ^Seccomp` — `2` (filter mode).
5. `journalctl -u mosaicfs -f` over 24 h — no LSM/seccomp denials, agent
   reports normal heartbeats and crawls.
6. Smoke tests: agent can read watch paths, write `/var/lib/mosaicfs`,
   reach `127.0.0.1:5984`; cannot read `/etc/shadow`, write outside its
   state dir, or reach `1.1.1.1`.
7. Co-located CouchDB shows the new node registered in the federation.

## Files to be created or modified

Created:

- `deploy/systemd/mosaicfs.service`
- `deploy/systemd/install.sh` (or equivalent makefile target)
- `docs/deployment/linux-host.md`
- `mosaicfs/src/sandbox.rs`

Modified:

- `mosaicfs/src/main.rs` — call sandbox init early on Linux.
- `mosaicfs/Cargo.toml` — add `caps`, `landlock`, `seccompiler` deps under
  Linux `[target.'cfg(target_os = "linux")'.dependencies]`.
- `mosaicfs-agent/src/start.rs` — replace `df` spawn with `statvfs`.
- Workspace `Cargo.toml` — possibly version-pin the new deps in
  `[workspace.dependencies]`.

Unmodified (called out for the reader): `mosaicfs-server/`, `mosaicfs-vfs/`,
`mosaicfs-common/` (except possibly hosting `sandbox.rs` if we decide it
belongs there in design-notes), `desktop/`, `deploy/mosaicfs.yaml`,
`Dockerfile.mosaicfs`, `scripts/start-dev-environment`.

## Next step

Produce `./design-notes.md` covering: exact systemd unit content, exact
crate version pins, the seccomp syscall list with rationale per syscall,
the Landlock ABI version handling, the bring-up runbook (log-only →
enforce flip), and the install-script contents.
