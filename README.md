
## Desktop App (macOS)

The `desktop/` directory contains a minimal Tauri v2 app that opens the MosaicFS browser in a native window. It is sandboxed — it has no access to your files, though outgoing network connections are unrestricted.

### Prerequisites

- Rust toolchain (`rustup`)
- Tauri CLI v2: `cargo install tauri-cli --version "^2"`

### Build and install

```sh
cd desktop
cargo tauri build --bundles app
cp -r target/release/bundle/macos/MosaicFS.app /Applications/
```

The build step ad-hoc signs the app with the sandbox entitlements (`Entitlements.plist`). No Apple Developer account is required.

### Open

```sh
open /Applications/MosaicFS.app
```

Or click the icon in Launchpad. The app connects to `http://localhost:8443/ui/browse` — start the MosaicFS server first.
