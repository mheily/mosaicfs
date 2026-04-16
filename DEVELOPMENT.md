# Development notes

# Building the image

Run `make mosaicfs-image` to build the `mosaicfs:latest` production image
(mosaicfs-server + mosaicfs-agent).

Run `podman kube play --replace deploy/mosaicfs.yaml` to start the pod on Linux.

Run `mosaicfs-server bootstrap` on first start (or click through the /admin
bootstrap page) to generate an initial admin user.

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

The admin UI is server-rendered from `mosaicfs-server` at `/admin`. Templates
live in `mosaicfs-server/templates/` (Tera), static assets (Pico CSS + HTMX)
are in `mosaicfs-server/assets/` and embedded at compile time via
`include_bytes!`. There is no separate build step — rebuild the server and
the admin UI is up-to-date.

Pages use HTMX for polling (every 10s) and regular HTML form POSTs for
writes (POST-redirect-GET with session flash). When `MOSAICFS_INSECURE_HTTP=1`
is set, the admin auth middleware is bypassed (dev only, binds to 127.0.0.1).
