# Development notes

# Building the image

Run `make mosaicfs-image` to build the `mosaicfs:latest` production image
(the unified `mosaicfs` binary — agent + web UI + VFS selected at runtime
via `[features]` in `mosaicfs.toml`).

Run `podman kube play --replace deploy/mosaicfs.yaml` to start the pod on Linux.

On first start, either click through the `/admin/bootstrap` page or run
`mosaicfs bootstrap --config /etc/mosaicfs/mosaicfs.toml` inside the
container to generate an initial admin credential.

# Automated code generation

Inside a devcontainer terminal, run:

```
env IS_SANDBOX=1 claude --dangerously-skip-permissions
```

# Running podman as a different user

podman depends on dbus, so you have to switch to mosaicfs UID/GID like this:
```
sudo machinectl shell mosaicfs@
```

# Building

Build all Rust crates in the workspace:

```
cargo build
```

Build in release mode:

```
cargo build --release
```

Build a specific crate:

```
cargo build -p mosaicfs           # unified host binary
cargo build -p mosaicfs-common
cargo build -p mosaicfs-agent
cargo build -p mosaicfs-server
cargo build -p mosaicfs-vfs
```

Run all tests:

```
cargo test
```

Check compilation without producing binaries:

```
cargo check
```

# Admin UI

The admin UI is server-rendered by the `mosaicfs-server` crate at `/admin`,
loaded into the unified binary when `features.web_ui = true`. Templates
live in `mosaicfs-server/templates/` (Tera), static assets (Pico CSS + HTMX)
are in `mosaicfs-server/assets/` and embedded at compile time via
`include_bytes!`. There is no separate build step — rebuild the `mosaicfs`
binary and the admin UI is up-to-date.

Pages use HTMX for polling (every 10s) and regular HTML form POSTs for
writes (POST-redirect-GET with session flash). When `insecure_http = true`
is set in `[web_ui]` (or `MOSAICFS_INSECURE_HTTP=1` in the env), the admin
auth middleware is bypassed and the bind host is forced to 127.0.0.1
(dev only).
