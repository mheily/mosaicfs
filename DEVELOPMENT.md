# Development notes

# Automated code generation

Inside a devcontainer terminal, run:

```
env IS_SANDBOX=1 claude --dangerously-skip-permissions
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