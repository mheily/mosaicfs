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

# 1. system user — home-dir becomes pw_dir, used by the XDG_DATA_HOME
#    fallback in the sandbox code to locate the state directory.
#    Admins can point the data at any path by adjusting pw_dir in /etc/passwd.
id mosaicfs &>/dev/null || \
    useradd --system --shell /usr/sbin/nologin --home-dir /var/lib/mosaicfs mosaicfs

# 2. directories (StateDirectory= will also create /var/lib/mosaicfs, but
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
