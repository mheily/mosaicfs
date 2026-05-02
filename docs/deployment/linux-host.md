# Linux host deployment (NAS / headless agent)

This doc covers running `mosaicfs` as a hardened systemd service on a bare-metal
Linux host (Debian 12 / Ubuntu 24.04, kernel ≥ 6.1). The Podman deployment
(`deploy/mosaicfs.yaml`) remains the dev/CI path and is unaffected.

## Prerequisites

### Kernel requirements

- Kernel ≥ 6.1 (Landlock ABI v2).
- Landlock LSM enabled: `grep landlock /sys/kernel/security/lsm` must match.
  If absent, add `lsm=landlock,...` to your kernel command line and reboot.
- The install script checks both and exits early if they are not met.

### CouchDB

Install from the Apache CouchDB upstream packages (Debian 12 example):

```bash
curl -fsSL https://couchdb.apache.org/repo/keys.asc \
  | gpg --dearmor -o /etc/apt/trusted.gpg.d/couchdb.gpg
echo "deb https://apache.jfrog.io/artifactory/couchdb-deb/ bookworm main" \
  > /etc/apt/sources.list.d/couchdb.list
apt update && apt install -y couchdb
```

Bind CouchDB to loopback only — edit `/opt/couchdb/etc/local.ini`:

```ini
[chttpd]
bind_address = 127.0.0.1
port = 5984
```

Then `systemctl restart couchdb`.

## Build

On a build host (or the NAS itself, if it has a Rust toolchain):

```bash
cargo build --release
```

Cross-compilation from x86_64 to aarch64:

```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

## Install

From the repo root, with the release binary already built:

```bash
sudo bash deploy/systemd/install.sh
```

The script:
1. Checks kernel ≥ 6.1 and Landlock LSM.
2. Creates the `mosaicfs` system user if absent.
3. Creates `/etc/mosaicfs` and `/var/lib/mosaicfs` with correct ownership.
4. Installs the binary to `/usr/local/bin/mosaicfs`.
5. Copies `deploy/systemd/mosaicfs.example.toml` to `/etc/mosaicfs/mosaicfs.toml`
   **only if the file does not already exist** (safe to re-run).
6. Installs the unit file and runs `systemctl daemon-reload`.

## Configure

Edit `/etc/mosaicfs/mosaicfs.toml`:

```bash
sudo -e /etc/mosaicfs/mosaicfs.toml
```

Minimum changes required:
- Set `watch_paths` to the directories you want indexed.
- Set `[couchdb].password` to the CouchDB admin password.

For the NAS (agent-only) role, keep `web_ui = false` and `vfs = false`.

## First start

```bash
sudo systemctl enable --now mosaicfs
```

Watch the log for the first heartbeat (30 s) and storage-capacity check (300 s):

```bash
journalctl -u mosaicfs -f
```

Expected: structured JSON log lines with `"level":"INFO"` for `mosaicfs starting`,
`Landlock applied`, `seccomp filter applied`, heartbeat, and crawl events. No
`"level":"ERROR"` lines.

## Verification

### Sandbox score

```bash
systemd-analyze security mosaicfs.service
```

Expected exposure: ≤ 1.5. A score above 3.0 indicates a directive was not
loaded (check `journalctl -u mosaicfs` for unit parse errors).

### Capability check

```bash
cat /proc/$(pgrep mosaicfs)/status | grep ^Cap
```

All four lines (`CapInh`, `CapPrm`, `CapEff`, `CapBnd`, `CapAmb`) should be
`0000000000000000`.

### Seccomp mode

```bash
cat /proc/$(pgrep mosaicfs)/status | grep ^Seccomp
```

Should show `Seccomp: 2` (BPF filter mode).

### Network isolation smoke tests

```bash
# Blocked — external IP
sudo -u mosaicfs curl -sS --max-time 2 http://1.1.1.1/
# Should succeed
sudo -u mosaicfs curl -sS --max-time 2 http://127.0.0.1:5984/
```

### Filesystem isolation smoke tests

```bash
# Should fail with EACCES
sudo -u mosaicfs cat /etc/shadow
# Should succeed
sudo -u mosaicfs ls /var/lib/mosaicfs
```

## Seccomp bring-up (log-first workflow)

On the first deploy, verify no unexpected syscalls are being denied before
switching to enforce mode:

1. Add a drop-in to switch to log mode:
   ```bash
   sudo systemctl edit mosaicfs
   # Add:
   # [Service]
   # Environment=MOSAICFS_SECCOMP_LOG=1
   ```
2. Restart and wait for two full health-check cycles (≥ 600 s):
   ```bash
   sudo systemctl restart mosaicfs
   sleep 620
   ```
3. Check for SECCOMP hits:
   ```bash
   journalctl -u mosaicfs --since "10 minutes ago" | grep -i seccomp
   ```
   No hits expected. If any appear, the syscall is not in the deny list and
   is being allowed — no action needed. If a denied syscall shows up in the
   log, file a bug.
4. Remove the drop-in and restart to enforce mode:
   ```bash
   sudo systemctl edit mosaicfs   # delete the Environment= line
   sudo systemctl restart mosaicfs
   ```

## Updating

```bash
cargo build --release
sudo install -m 0755 target/release/mosaicfs /usr/local/bin/mosaicfs
sudo systemctl restart mosaicfs
```

Watch `journalctl -u mosaicfs -f` for clean restart before walking away.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| `Failed to open /etc/mosaicfs/mosaicfs.toml` | Config missing or wrong path |
| `Landlock: watch path missing at startup` | `watch_paths` entry doesn't exist yet; create the directory first |
| `prctl failed` | Kernel too old or `NoNewPrivileges` conflict |
| `EPERM` from CouchDB requests | `IPAddressDeny` blocking; verify CouchDB is on `127.0.0.1:5984` |
| Agent starts but no files indexed | Check `watch_paths` ownership — must be readable by the `mosaicfs` user |
| `MemoryDenyWriteExecute` failure | A dep is mapping RWX pages; see design-notes §5 for the mitigation |
