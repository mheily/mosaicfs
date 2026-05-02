# Review: docs/changes/015 — Linux agent host-OS deployment + hardening

## Context

The intent doc (`docs/changes/015/intent.md`) proposes deploying the MosaicFS
agent to a Linux NAS as a systemd service running directly on the host (rather
than in a container), hardened with a 7-layer plan written by Haiku
(`mosaicfs_linux_security_summary.md`). The user is asking, before
implementing, whether the approach is sound and whether the plan is accurate.

This is a review / Q&A task, not an implementation. No code changes.

## Answers to the three questions

### 1. Is host-OS + systemd hardening actually a good idea, or is a container better?

**Both are reasonable; host-OS is a fine choice given the stated goals.** The
common belief that "container = more secure" overstates the difference:

- A rootless Podman container on Linux gets its isolation from the *same*
  kernel primitives the plan already uses directly: user namespaces, seccomp,
  cgroups, capabilities. Default container profiles are often *weaker* than a
  carefully written systemd unit (Docker's default seccomp profile allows ~300
  syscalls; a tight systemd+Landlock+seccomp policy allows far fewer paths and
  syscalls).
- Containers' real wins are **portability, reproducibility, and operational
  simplicity** — not raw security. A NAS that already runs systemd doesn't
  benefit much from the portability angle.
- The user's stated goals — personal education, learning Linux security
  primitives, ability to fall back to containers later — all support the
  host-OS path.

Caveat: the operational cost (writing/maintaining the unit, handling kernel
version variation, debugging seccomp denials) is real and higher than `podman
kube play`. Worth it only if learning the primitives is itself a goal, which
the user has said it is.

**Verdict: yes, proceed with host-OS.**

### 2. Is the proposed solution accurate?

**Mostly directionally right, but it has several concrete bugs that would
break things on first deploy.** Treat the doc as a starting outline, not a
spec to follow verbatim.

Concrete issues found:

1. **`AuditControl=yes` is not a systemd directive.** It does not exist.
   Audit is configured via `auditd` / `auditctl` rules, not the unit file.
   The unit will either ignore it or refuse to load. Remove it; if audit is
   wanted, add an `auditctl -w /var/lib/mosaicfs -p wa -k mosaicfs` rule
   outside the unit.

2. **Blocking `clone` and `fork` in seccomp will break tokio.** This is the
   biggest functional bug. `tokio = { features = ["full"] }` in `Cargo.toml`
   means a multi-threaded runtime, which calls `clone` (or `clone3`) to spawn
   worker threads. The proposed filter denies `execve`, `execveat`, `fork`,
   `clone`, `clone3` outright with `EACCES` — the agent will fail to start
   threads, or crash mid-startup. The correct pattern is:
   - Allow `clone`/`clone3` but use seccomp argument filtering to forbid
     flags that would create a *process* (no-`CLONE_THREAD`, no
     `CLONE_NEWNS`, etc.), or
   - Just block `execve`/`execveat` (which is what actually prevents
     spawning new programs) and let thread creation through.
   The simpler, well-trodden option is: block `execve`/`execveat`,
   `ptrace`, `mount`, `umount2`, `bpf`, `kexec_load`, `init_module`,
   `finit_module`, `delete_module`, `unshare`, `setns`, and the keyring
   syscalls — leave `clone`/`clone3` alone.

3. **Landlock crate API in the example is invented.** The real `landlock`
   crate uses a `Ruleset::default().handle_access(AccessFs::from_all(...))?
   .create()?.add_rules(path_beneath_rules!(...))?.restrict_self()?` style.
   The pseudocode in the doc won't compile. Not a design flaw, just don't
   copy-paste.

4. **`cat /sys/kernel/config/bpf/bpf_enabled` is not a real check.** That
   path doesn't exist. To verify Landlock: `cat /sys/kernel/security/lsm |
   grep landlock`, or call `landlock::ABI::new_current()` from Rust.

5. **`PrivateDevices=yes` blocks `/dev/fuse`.** Today the agent does not
   depend on FUSE (no fuser/fuse-mt crate in `Cargo.toml`; the Linux serving
   path is NFS on loopback). So this is fine *for now*. But: if a future
   change adds a Linux FUSE adapter, `PrivateDevices=yes` plus
   `NoNewPrivileges=yes` will block it (FUSE typically wants `/dev/fuse` and
   either `CAP_SYS_ADMIN` or the `fusermount` setuid helper). Worth noting in
   the change doc so it isn't a surprise later.

6. **Kernel version claim is slightly off.** Landlock landed in 5.13, ABI v2
   in 5.19, network-bind support in 6.7. "Stable in 5.15" is fine, but the
   minimum kernel for the design as written is **5.13**, and **6.1+** is what
   you actually want (Debian 12 ships 6.1; Debian 11 / older Ubuntu LTS won't
   work without backports). State this minimum explicitly.

7. **Hard-coding CouchDB IPs** in the unit is workable on a static home LAN
   but should be called out as a deliberate tradeoff against
   `IPAddressAllow=` not supporting hostnames. If the NAS or peers ever get
   DHCP leases, the unit needs editing on every IP change.

8. **`MemoryDenyWriteExecute=yes`** is fine for a pure-Rust async binary,
   but be aware it is incompatible with any future dep that does JIT (some
   regex engines, V8/wasm runtimes, etc.). Low risk today.

What the doc gets *right*: the overall layering (user/caps → namespaces →
Landlock → seccomp → in-code drop → IPAddress filter → audit) is the
canonical pattern; `ProtectSystem=strict` + `ReadWritePaths=` is correct;
`CapabilityBoundingSet=` (empty) is correct syntax to drop all; ordering of
`PR_SET_NO_NEW_PRIVS` → drop caps → Landlock → seccomp is right; the
`systemd-analyze security` validation step is a good practice.

### 3. What are the risks of failure / known issues?

- **High likelihood of seccomp false positives during bring-up.** Even after
  fixing the `clone`/`fork` mistake, real Rust + tokio + your HTTP/CouchDB
  stack will hit syscalls you didn't anticipate (e.g. `getrandom`, `statx`,
  `prlimit64`, `rseq`, `io_uring_*`). Plan to start in *log-only* mode
  (`SCMP_ACT_LOG`) for a day, harvest the actual syscall set, then flip to
  enforcing. Skipping this step is the most common reason these projects
  stall at "90%."
- **Landlock can't restrict things it doesn't model.** Notably, network
  sockets (until 6.7), some TTY/ioctl operations, and process signaling.
  Don't treat Landlock as a complete sandbox — it complements seccomp, it
  doesn't replace it.
- **Distro/kernel skew.** A NAS distro (e.g. older Debian-based, OpenMediaVault,
  Synology DSM, TrueNAS SCALE) may not have a recent enough kernel or may
  ship a non-systemd init. Confirm the actual NAS environment before
  committing to this design — this is the single biggest unknown.
- **systemd-analyze chasing.** It's tempting to optimize the score; don't.
  Optimize for the actual threat model. A score of 1.5 with `PrivateDevices`
  blocking FUSE is worse than 2.5 with FUSE working.
- **People do successfully run hardened systemd services this way.** The
  pattern (NoNewPrivileges + ProtectSystem=strict + Landlock + seccomp + IP
  allow-list) is used by mainstream projects (e.g. systemd-resolved, parts of
  the systemd suite itself, some Mastodon/Matrix deployments). It is not
  experimental. The bugs above are in *this specific draft*, not the approach.

## Overall recommendation

**Yes, proceed — but revise the security summary doc before implementing.**

Specifically, before opening change 015 for implementation:

1. Confirm the target NAS distro and kernel version (must be ≥ 5.13, prefer
   ≥ 6.1).
2. Fix the seccomp policy: do not block `clone`/`clone3`; block `execve`/
   `execveat` and the privilege/kernel syscalls instead.
3. Drop `AuditControl=yes`; if audit is desired, configure auditd separately.
4. Replace the Landlock pseudocode with the real `landlock` crate idioms.
5. Note `PrivateDevices=yes` as a future blocker for any Linux FUSE adapter.
6. Plan a seccomp log-only bring-up phase — do not jump straight to
   enforcing.

If the NAS turns out to be running an old kernel or non-systemd init,
fall back to a rootless Podman container — the abandon-and-container exit
the user named in the intent.

## Files reviewed

- `docs/changes/015/intent.md`
- `docs/changes/015/mosaicfs_linux_security_summary.md`
- `Cargo.toml` (confirmed tokio multi-threaded; no FUSE dep today)
- `deploy/mosaicfs.yaml` (current container-based deploy, for contrast)

## Verification

This is a review, not an implementation, so no end-to-end test. The
recommendation should be verified by:

- The user confirming the actual NAS distro and `uname -r` output.
- Re-running the security plan through a deeper model (Sonnet/Opus) to
  cross-check the systemd directive list against the current
  `systemd.exec(5)` man page on the target distro.
