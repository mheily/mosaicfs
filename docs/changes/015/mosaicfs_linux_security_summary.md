# MosaicFS Linux Agent Security Hardening Plan

## Overview

This document outlines a comprehensive, defense-in-depth security architecture for the MosaicFS agent running on Linux. The strategy layers multiple restriction mechanisms to prevent privilege escalation, filesystem breakout, code execution, and data exfiltration.

**Target:** v1 implementation on Linux (macOS FileProvider and redb deferred to v2)

---

## Threat Model & Attack Surface

### What We're Protecting Against

1. **Privilege escalation** — compromised agent gaining root or other user privileges
2. **Filesystem breakout** — escaping the intended data directories (/var/lib/mosaicfs, /mnt/data, etc.)
3. **Process spawning** — spawning arbitrary child processes or executing binaries
4. **Kernel access** — loading modules, modifying tunables, accessing dmesg
5. **Data exfiltration** — exfiltrating data to unauthorized network destinations
6. **IPC/socket exploitation** — leaking data via temp files, shared memory, or Unix sockets
7. **Side-channel attacks** — information leakage through timing, cache covert channels (mitigated at kernel level, not per-service)
8. **Code injection** — buffer overflows, heap exploits, ROP chains (mitigated by Rust memory safety + W^X)

### What's Already Protected (Rust Implementation)

- Memory safety (no buffer overflows, use-after-free by default)
- No execve of untrusted binaries (pure Rust, no external tool spawning)
- Type safety (no arbitrary memory access)

---

## Security Architecture: 7-Layer Defense

### Layer 1: Unprivileged User & Capability Isolation

**Mechanism:** Dedicated system user account with no capabilities

**Implementation:**
```bash
useradd --system \
  --shell /usr/sbin/nologin \
  --home-dir /nonexistent \
  mosaicfs
```

**systemd configuration:**
```ini
User=mosaicfs
Group=mosaicfs
NoNewPrivileges=yes
CapabilityBoundingSet=
```

**What it prevents:**
- Process runs with unprivileged UID/GID (mosaicfs:mosaicfs)
- Cannot gain capabilities via setuid/setgid or filesystem capabilities
- No CAP_SYS_ADMIN, CAP_NET_ADMIN, CAP_SYS_PTRACE, etc.
- Blast radius limited to mosaicfs user permissions

**Kernel dependency:** None (always available)

---

### Layer 2: Namespace Isolation (systemd)

**Mechanisms:** Filesystem, network, device, and process visibility isolation

**systemd configuration:**
```ini
# Filesystem namespace
ProtectSystem=strict                    # All except /dev, /proc, /sys are read-only
ProtectHome=yes                         # /home, /root, /run/user inaccessible
PrivateTmp=yes                          # Private /tmp and /var/tmp
PrivateDevices=yes                      # No access to physical devices

# Network namespace (replaced by IP allow-list below)
# DO NOT use PrivateNetwork=yes for this service (need CouchDB access)

# Process/kernel visibility
ProtectProc=invisible                   # Other users' processes hidden
ProcSubset=pid                          # Only /proc/self visible

# Kernel protection
ProtectKernelTunables=yes               # /proc/sys and /sys read-only
ProtectKernelModules=yes                # Cannot load kernel modules
ProtectControlGroups=yes                # /sys/fs/cgroup read-only
ProtectKernelLogs=yes                   # dmesg access blocked
ProtectHostname=yes                     # Cannot modify hostname
ProtectClock=yes                        # Cannot change system clock
```

**What it prevents:**
- Escaping to other directories (filesystem namespace)
- Accessing sensitive /proc and /sys interfaces
- Modifying kernel state
- Accessing other users' processes

**Kernel dependency:** Linux 4.6+ (namespaces stable, earlier features in 5.7+)

---

### Layer 3: Filesystem Path Whitelisting (Landlock)

**Mechanism:** Landlock LSM syscall interception (user-space sandbox)

**Implementation (Rust):**
```rust
use landlock::*;

fn apply_landlock() -> Result<()> {
    Landlock::new()
        .read_write("/var/lib/mosaicfs")           // State directory
        .read_only("/etc/mosaicfs.toml")           // Config
        .read_write("/run/mosaicfs")               // Runtime files
        .read_only("/usr/lib")                     // System libraries
        .read_only("/etc/ca-certificates")         // TLS certs (if needed)
        // Deny everything else implicitly
        .restrict_self()?;
    Ok(())
}
```

**What it prevents:**
- Accessing files outside the whitelist
- Reading sensitive files (/etc/passwd, /root/.ssh, etc.)
- Writing to system directories
- Escalating via setuid binaries (no execute permission granted)

**Kernel dependency:** Linux 5.13+ (stable in 5.15+)

**Advantage over ProtectSystem=strict:** Per-process, finer granularity, can be tuned per deployment via TOML flags

---

### Layer 4: Syscall Filtering (seccomp-bpf)

**Mechanism:** Block dangerous syscalls at the kernel boundary

**Implementation (Rust with `libseccomp` crate):**
```rust
use libseccomp::*;

fn apply_seccomp() -> Result<(), Box<dyn std::error::Error>> {
    let mut filter = ScmpFilterContext::new(ScmpAction::Allow)?;
    filter.add_arch(ScmpArch::X8664)?;
    
    // Block process spawning entirely
    for syscall_name in &["execve", "execveat", "fork", "clone", "clone3"] {
        let syscall = ScmpSyscall::from_name(syscall_name)?;
        filter.add_rule(ScmpAction::Errno(libc::EACCES), syscall)?;
    }
    
    // Block privilege escalation syscalls
    for syscall_name in &["prctl", "ptrace", "mount", "umount2"] {
        let syscall = ScmpSyscall::from_name(syscall_name)?;
        filter.add_rule(ScmpAction::Errno(libc::EPERM), syscall)?;
    }
    
    // Block BPF loading (prevent eBPF rootkits)
    let bpf_syscall = ScmpSyscall::from_name("bpf")?;
    filter.add_rule(ScmpAction::Errno(libc::EPERM), bpf_syscall)?;
    
    filter.load()?;
    Ok(())
}
```

**What it prevents:**
- Spawning new processes (fork, clone, execve all blocked)
- Escalating via prctl/CAP_SYS_ADMIN
- Mounting filesystems
- Loading eBPF programs (eBPF rootkits)
- Tracing other processes (ptrace)

**Kernel dependency:** Linux 4.8+ (comprehensive BPF support)

**Crate selection:** `libseccomp` is the recommended choice because:
- Official Rust bindings to libseccomp C library
- Actively maintained
- Full control over rules
- Simple API for your use case (unconditional allow-list)

---

### Layer 5: In-Code Privilege Dropping

**Mechanism:** Explicitly drop capabilities and set security flags at startup

**Implementation (Rust):**
```rust
use caps::{Capability, CapSet};

fn drop_capabilities() -> Result<()> {
    // Drop all capabilities for current user
    caps::clear(CapSet::Permitted, CapSet::Inheritable)?;
    
    // Verify no capabilities remain
    let caps = caps::read(None, CapSet::Effective)?;
    if !caps.is_empty() {
        return Err("Failed to drop all capabilities".into());
    }
    
    Ok(())
}

fn set_no_new_privs() -> Result<()> {
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err("Failed to set PR_SET_NO_NEW_PRIVS".into());
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    // 1. Set no-new-privs FIRST (before dropping caps)
    set_no_new_privs()?;
    
    // 2. Drop capabilities
    drop_capabilities()?;
    
    // 3. Apply Landlock
    apply_landlock()?;
    
    // 4. Apply seccomp
    apply_seccomp()?;
    
    // Now safe to start agent
    run_agent().await
}
```

**What it prevents:**
- Any subsequent privilege escalation (even if a library tries to setuid)
- Gaining new capabilities after startup

**Kernel dependency:** Linux 3.17+ (prctl PR_SET_NO_NEW_PRIVS)

---

### Layer 6: Network Egress Filtering (systemd + cgroup eBPF)

**Mechanism:** Allow-list outbound network connections by IP address

**Implementation:** systemd configuration with cgroup eBPF hooks (kernel 4.11+)

```ini
# Block all outbound except explicit allow-list
IPAddressDeny=any

# Allow loopback (127.0.0.0/8 for IPv4, ::1/128 for IPv6)
# Needed for: internal IPC, NFS serve on localhost, CouchDB if local
IPAddressAllow=127.0.0.0/8
IPAddressAllow=::1/128

# Allow CouchDB peer(s) ONLY
# Replace 192.168.1.100 with actual CouchDB IP(s)
IPAddressAllow=192.168.1.100/32    # Primary CouchDB
IPAddressAllow=192.168.1.101/32    # Secondary CouchDB (if multi-node)
```

**What it prevents:**
- Exfiltration to arbitrary network addresses
- DNS-based covert channels (hard-code CouchDB IP, don't resolve hostnames)
- Connecting to command-and-control servers
- Accidental exposure via misconfiguration

**Notes:**
- Works at **IP level**, not port level (systemd limitation)
- In-code, ensure your HTTP client connects only to port 5984 for CouchDB
- Both ingress and egress are filtered (source/destination IP checked)
- Uses kernel cgroup eBPF hooks, unrelated to iptables

**Kernel dependency:** Linux 4.11+ (cgroup eBPF hooks for cgroup_sock_addr)

---

### Layer 7: Audit & Observability

**Mechanism:** Log all denied operations for intrusion detection

**systemd configuration:**
```ini
AuditControl=yes                       # Enable kernel audit logging
```

**Monitoring:**
```bash
# View denied syscalls
sudo auditctl -w /var/lib/mosaicfs -p wa -k mosaicfs_audit
sudo ausearch -k mosaicfs_audit

# View network denials (cgroup eBPF blocks)
sudo journalctl -t kernel --grep="cgroup" -f

# Monitor service health
sudo systemctl status mosaicfs.service
sudo journalctl -u mosaicfs.service -f

# Validate security posture
systemd-analyze security mosaicfs.service
```

**What it provides:**
- Visibility into enforcement actions
- Early warning of compromise attempts
- Audit trail for compliance

---

## Complete systemd Unit File

```ini
[Unit]
Description=MosaicFS Virtual Filesystem Agent
After=network.target couchdb.service
Wants=network-online.target

[Service]
Type=notify
User=mosaicfs
Group=mosaicfs
WorkingDirectory=/var/lib/mosaicfs

ExecStart=/usr/local/bin/mosaicfs-agent --config=/etc/mosaicfs.toml

# Restart policy
Restart=on-failure
RestartSec=5s
StartLimitInterval=60s
StartLimitBurst=3

# === Layer 1: User & Privilege Restrictions ===
NoNewPrivileges=yes
CapabilityBoundingSet=

# === Layer 2: Namespace Isolation ===
# Filesystem
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/mosaicfs
ReadWritePaths=/run/mosaicfs
StateDirectory=mosaicfs
LogsDirectory=mosaicfs
RuntimeDirectory=mosaicfs
CacheDirectory=mosaicfs

# Device & Kernel
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectKernelLogs=yes

# Process & Clock
ProtectProc=invisible
ProcSubset=pid
ProtectHostname=yes
ProtectClock=yes

# === Layer 6: Network Egress Filtering (Allow-List) ===
IPAddressDeny=any
IPAddressAllow=127.0.0.0/8
IPAddressAllow=::1/128
IPAddressAllow=192.168.1.100/32    # CouchDB primary (adjust to your IP)
# IPAddressAllow=192.168.1.101/32  # CouchDB secondary (if multi-node)

# === Layer 3+4+5: Applied in Rust Code ===
# (Landlock, seccomp, capability dropping)

# === Additional Hardening ===
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
MemoryDenyWriteExecute=yes         # W^X enforcement
LockPersonality=yes
RemoveIPC=yes

# === Audit & Monitoring ===
AuditControl=yes

# === Resource Limits ===
MemoryMax=512M
CPUQuota=75%

[Install]
WantedBy=multi-user.target
```

---

## Optional Enhancements (v1.1+)

### Kernel Lockdown LSM

**What it does:** Eliminates user-space methods to escalate to kernel privileges

**Setup:** Add to `/etc/default/grub`:
```bash
GRUB_CMDLINE_LINUX="lockdown=confidentiality"
```

Then run `sudo update-grub && sudo reboot`

**What it prevents:** kexec, raw I/O, kernel module loading, debugfs access (system-wide)

**Verdict:** Recommended for production but requires kernel boot parameter change

---

### AppArmor Profile

**What it does:** Mandatory Access Control at the OS level (path-based, simpler than SELinux)

**Example profile** (`/etc/apparmor.d/usr.local.bin.mosaicfs-agent`):
```apparmor
#include <tunables/global>

/usr/local/bin/mosaicfs-agent {
  #include <abstractions/base>
  
  /etc/mosaicfs.toml r,
  /var/lib/mosaicfs/ rw,
  /var/log/mosaicfs/ w,
  /run/mosaicfs/ w,
  
  deny / rwx,  # Deny everything else
}
```

**Load it:**
```bash
sudo apparmor_parser -r /etc/apparmor.d/usr.local.bin.mosaicfs-agent
```

**Verdict:** Overkill if systemd sandboxing is tight; consider for high-security deployments

---

### BPF LSM (Advanced)

**What it does:** Custom kernel security policies via eBPF (Linux 5.7+)

**Use case:** Prevent mmap(PROT_EXEC | PROT_WRITE) or other sophisticated attacks

**Verdict:** Defer to v2; too complex for v1 unless you anticipate JIT-based attacks

---

## Deployment Checklist

### Pre-Deployment

- [ ] Create `mosaicfs` system user: `useradd --system --shell /usr/sbin/nologin --home-dir /nonexistent mosaicfs`
- [ ] Create state directories: `mkdir -p /var/lib/mosaicfs /var/log/mosaicfs /run/mosaicfs`
- [ ] Set ownership: `chown -R mosaicfs:mosaicfs /var/lib/mosaicfs /var/log/mosaicfs /run/mosaicfs && chmod 750 ...`
- [ ] Verify Landlock support: `cat /sys/kernel/config/bpf/bpf_enabled` or `bpftool feature probe kernel`
- [ ] Verify libseccomp: `dpkg -l | grep libseccomp` (or equivalent on your distro)
- [ ] Determine CouchDB IP address (hard-code in TOML config and systemd unit)

### Rust Implementation

- [ ] Add `libseccomp` crate to `Cargo.toml`: `libseccomp = "0.3"`
- [ ] Add `caps` crate: `caps = "0.5"`
- [ ] Add `landlock` crate: `landlock = "0.2"`
- [ ] Implement `drop_capabilities()`, `set_no_new_privs()`, `apply_landlock()`, `apply_seccomp()` in main startup
- [ ] Call functions in correct order: no_new_privs → drop_caps → landlock → seccomp → agent loop
- [ ] Compile as PIE binary (default in modern Rust, verify with `file` output)

### systemd Deployment

- [ ] Copy `mosaicfs.service` to `/etc/systemd/system/`
- [ ] Run `sudo systemctl daemon-reload`
- [ ] Test: `sudo systemctl start mosaicfs.service`
- [ ] Verify: `sudo systemctl status mosaicfs.service`
- [ ] Check security score: `systemd-analyze security mosaicfs.service` (aim for < 2.0 exposure)
- [ ] Monitor logs: `sudo journalctl -u mosaicfs.service -f`

### Validation

- [ ] Verify service runs as `mosaicfs` user: `ps aux | grep mosaicfs`
- [ ] Verify no capabilities: `grep Cap /proc/$(pgrep mosaicfs)/status` (should be 0000...)
- [ ] Verify filesystem isolation: `ls -la /var/lib/mosaicfs/` (owned by mosaicfs:mosaicfs)
- [ ] Verify network filtering: `sudo systemctl show mosaicfs.service -p IPAddressAllow --value`
- [ ] Test blocked connection: `sudo -u mosaicfs timeout 2 curl http://8.8.8.8/` (should fail)
- [ ] Test allowed connection: `sudo -u mosaicfs timeout 2 curl http://192.168.1.100:5984/` (should succeed, or fail with connection refused if CouchDB down — but not IP block)

---

## Known Limitations & Tradeoffs

| Limitation | Why | Mitigation |
|-----------|-----|-----------|
| **Landlock requires Linux 5.13+** | Relatively new LSM | Fall back to ProtectSystem=strict for older kernels (less granular) |
| **Network filtering at IP level only** | systemd limitation | Trust in-code to use only port 5984 for CouchDB; hard-code IP to prevent DNS exfil |
| **Rust memory safety doesn't prevent all attacks** | Doesn't cover unsafe blocks | Code review for unsafe sections; prefer safe alternatives |
| **No port-level filtering in systemd** | cgroup eBPF limitation | Use in-code configuration + Landlock to prevent executing other network tools |
| **Audit logging requires kernel audit subsystem** | May not be available in containers | Fall back to journalctl logging in application |
| **eBPF rootkits possible if CAP_BPF available** | Blocked by dropping caps, but older kernels might not have CAP_BPF | Update kernel; Cap_SYS_ADMIN covers this pre-5.8 |

---

## Testing & Verification Strategy

### Unit Tests (Rust)

```rust
#[cfg(test)]
mod security_tests {
    #[test]
    fn test_capabilities_dropped() {
        let caps = caps::read(None, CapSet::Effective).unwrap();
        assert!(caps.is_empty(), "Capabilities not fully dropped");
    }
    
    #[test]
    fn test_landlock_blocks_unauthorized_path() {
        // This requires running in actual restricted environment
        // Hard to test in unit test; prefer integration tests
    }
}
```

### Integration Tests

```bash
#!/bin/bash
# test_security.sh

# Test 1: User is unprivileged
PID=$(pgrep mosaicfs)
USER=$(ps -p $PID -o user=)
[ "$USER" = "mosaicfs" ] || { echo "FAIL: Not running as mosaicfs"; exit 1; }

# Test 2: No capabilities
CAPS=$(grep Cap /proc/$PID/status | grep -v Cap_Inh | head -1 | awk '{print $2}')
[ "$CAPS" = "0000000000000000" ] || { echo "FAIL: Capabilities not dropped"; exit 1; }

# Test 3: Cannot write to /etc
sudo -u mosaicfs touch /etc/test 2>/dev/null && { echo "FAIL: Can write to /etc"; exit 1; }

# Test 4: Cannot connect to 8.8.8.8
sudo -u mosaicfs timeout 1 curl http://8.8.8.8/ 2>/dev/null && { echo "FAIL: Can reach 8.8.8.8"; exit 1; }

# Test 5: Can connect to CouchDB
sudo -u mosaicfs timeout 2 curl http://192.168.1.100:5984/ >/dev/null 2>&1 || { echo "WARN: Cannot reach CouchDB"; }

echo "All tests passed"
```

### Manual Testing

```bash
# Start service
sudo systemctl start mosaicfs.service

# Check status and score
sudo systemctl status mosaicfs.service
systemd-analyze security mosaicfs.service

# Monitor logs in real-time
sudo journalctl -u mosaicfs.service -f &

# Try operations that should fail
sudo -u mosaicfs ls /root          # Should fail (ProtectHome)
sudo -u mosaicfs cat /proc/modules # Should fail (ProtectKernelModules)
sudo -u mosaicfs curl 8.8.8.8      # Should fail (IPAddressDeny=any)

# Try operations that should succeed
sudo -u mosaicfs curl 127.0.0.1    # Should timeout or succeed (loopback allowed)
sudo -u mosaicfs curl 192.168.1.100:5984  # Should connect or show connection refused (allowed IP)
```

---

## Multi-Node Considerations (Future: Mac Studio + M1 MacBook)

When adding secondary nodes via CouchDB peer coordination:

1. **Network filtering:** Add secondary node's CouchDB IP to `IPAddressAllow=`
2. **Landlock:** No changes needed (paths are per-node)
3. **seccomp:** No changes needed (process restrictions are per-node)
4. **Each node:** Has identical security profile via systemd unit

Example (multi-node):
```ini
# Primary CouchDB (Mac Studio)
IPAddressAllow=192.168.1.100/32

# Secondary CouchDB (M1 MacBook)
IPAddressAllow=192.168.1.101/32
```

---

## References & Crate Documentation

- **Landlock:** https://crates.io/crates/landlock
- **libseccomp:** https://crates.io/crates/libseccomp
- **caps:** https://crates.io/crates/caps
- **systemd.exec:** https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html
- **systemd.resource-control:** https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html

---

## Summary: What's Protected

| Attack Vector | Layer(s) | Status |
|---------------|----------|--------|
| Privilege escalation | 1, 5 | ✓ Blocked |
| Filesystem escape | 2, 3 | ✓ Blocked |
| Process spawning | 4 | ✓ Blocked |
| Kernel access | 2, 4 | ✓ Blocked |
| Data exfiltration | 6 | ✓ Blocked (IP allow-list) |
| Capability bypass | 1, 5 | ✓ Blocked |
| IPC exploitation | 2 | ✓ Blocked |
| Code injection (memory) | Rust + Layer 2 (W^X) | ✓ Protected |
| Side-channel timing | Kernel-level (out of scope) | ⚠ Partial |

---

## Next Steps for Implementation

1. **Rust implementation:** Integrate Landlock, seccomp, and capability dropping into agent startup
2. **systemd unit:** Deploy `mosaicfs.service` with all restrictions enabled
3. **Testing:** Validate each layer independently and in combination
4. **Documentation:** Add security architecture to project README and architecture docs
5. **Monitoring:** Set up auditd and journalctl log aggregation for production
6. **Future (v1.1+):** Consider kernel lockdown LSM and AppArmor profile for high-security deployments
7. **Future (v2):** Evaluate BPF LSM for sophisticated attacks; revisit redb integration and macOS FileProvider

