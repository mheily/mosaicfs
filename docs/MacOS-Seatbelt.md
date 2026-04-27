# macOS Sandbox Policy

MosaicFS Desktop is sandboxed via Apple's App Sandbox, enforced by the kernel
after codesigning.  The entitlements are declared in `desktop/Entitlements.plist`
and embedded into the binary by Tauri's build process.

## Enforcement: App Sandbox entitlements (the right approach)

```xml
<key>com.apple.security.app-sandbox</key><true/>
<key>com.apple.security.network.client</key><true/>
<key>com.apple.security.network.server</key><true/>
<key>com.apple.security.files.user-selected.read-only</key><true/>
<key>com.apple.security.files.bookmarks.app-scope</key><true/>
```

| Entitlement | What it covers |
|---|---|
| `app-sandbox` | Activates the kernel sandbox; grants implicit access to the app's own container and basic process operations |
| `network.client` | Outbound TCP — CouchDB connections (`connection.rs:47`) |
| `network.server` | Inbound loopback — embedded Axum HTTP server for WKWebView (`server.rs:55`) |
| `files.user-selected.read-only` | Directories the user picks via NSOpenPanel (`macos.rs:53`) |
| `files.bookmarks.app-scope` | Persisting and resolving security-scoped bookmarks across launches (`bookmarks.rs`) |

The App Sandbox automatically grants the process access to its own container
(`~/Library/Application Support/com.mosaicfs.desktop/`), anonymous memory
mapping, Mach task ports, and other basic process operations — none of which
need to be enumerated manually.

To test the sandbox on a development build, codesign the binary with the
entitlements plist:

```sh
codesign -s - --entitlements desktop/Entitlements.plist --force \
  target/debug/mosaicfs-desktop
./target/debug/mosaicfs-desktop
```

## Reference: what the app accesses

`desktop/mosaicfs-desktop.sb` documents the full set of resources the app uses.
It is **not used for runtime enforcement** — `sandbox-exec` is deprecated on
modern macOS and has silent failure modes that make it unreliable for Tauri/
Rust binaries (anonymous `mmap` for thread stacks fails without logging a
denial, regardless of profile rules).  The `.sb` file exists as a structured
record of the app's resource footprint.

### File system

| Path | Access | Reason |
|------|--------|--------|
| `~/Library/Application Support/com.mosaicfs.desktop/` | read/write | Tauri data root: `settings.json`, `bookmarks.json`, `server-data/` (TLS certs, JWT secret) |
| `~/.mosaicfs/` | read/write | Fallback `node-id.toml` written when `kern.uuid` is unavailable (`machine_id.rs:109`) |
| `$TMPDIR` | read/write | Atomic rename target for `bookmarks.json.tmp` (`bookmarks.rs:81`) |
| `~/Library/WebKit/com.mosaicfs.desktop/` | read/write | WKWebView website data store |
| `~/Library/Caches/com.mosaicfs.desktop/` | read/write | URL and image caches |
| `~/Library/Preferences/com.mosaicfs.desktop.plist` | read/write | NSUserDefaults |
| `~/Library/Saved Application State/com.mosaicfs.desktop.savedState/` | read/write | Cocoa window auto-save |
| `~` (home, read) | read | NSOpenPanel — resolved via security-scoped bookmarks at runtime |

### Sysctl

| Key | Reason |
|-----|--------|
| `kern.uuid` | IOPlatformUUID read by `machine_id.rs:31` for stable node identity — no child process needed |
| `kern.ostype`, `kern.osrelease`, `kern.version` | Runtime and framework initialisation |
| `hw.machine`, `hw.ncpu`, `hw.physicalcpu`, `hw.logicalcpu`, `hw.memsize` | Tokio runtime thread sizing |

### Network

| Rule | Reason |
|------|--------|
| Loopback TCP inbound (`127.0.0.1:*`) | Axum HTTP/1.1 server on `127.0.0.1:0` that serves the WKWebView UI (`server.rs:55`) |
| Loopback TCP outbound (`localhost:*`) | WKWebView connecting back to the embedded Axum server |
| Outbound TCP `:5984` | CouchDB default port; user-configurable in `settings.json` (`connection.rs:47`) |
| Outbound TCP `:80` / `:443` | HTTP and HTTPS for user-configured CouchDB URLs |
| Outbound UDP/TCP `:53` | DNS resolution |

If your CouchDB instance runs on a non-standard port, add `network.client` is
already declared; no entitlement change is needed, just ensure the URL in
`settings.json` is correct.

### Mach services

| Service | Reason |
|---------|--------|
| `com.apple.windowserver.active` | Required by every macOS GUI application |
| `com.apple.ViewBridgeAuxiliary` | WKWebView host/client XPC split |
| `com.apple.cfprefsd.{agent,daemon}` | CFPreferences / NSUserDefaults daemon |
| `com.apple.distributed_notifications40` | NSDistributedNotificationCenter |
| `com.apple.lsd.mapdb`, `com.apple.lsd.modifydb` | LaunchServices — `NSWorkspace.openURL()` (`macos.rs:118`) |
| `com.apple.dock.server` | NSStatusItem (system-tray icon, `lib.rs:189`) |
| `com.apple.tccd`, `com.apple.tccd.system` | TCC daemon — permission consent dialogs |
| `com.apple.gpsd`, `com.apple.MTLCompilerService` | Metal GPU compositor for WKWebView rendering |
| `com.apple.WebKit.GPU`, `com.apple.WebKit.Networking` | WebKit helper processes |
| `com.apple.fonts` | Font server |
| `com.apple.SystemConfiguration.configd` | Network reachability (SystemConfiguration framework) |
| `com.apple.pbs` | Pasteboard (clipboard) |
| `com.apple.usernotifications.usernotificationservice` | System notification banners |
| `com.apple.CoreServices.coreservicesd` | URL handling, file metadata |

### IOKit

| User client class | Reason |
|-------------------|--------|
| `IOSurfaceRootUserClient` | Shared GPU compositor surfaces (WKWebView rendering pipeline) |
| `AGXDeviceUserClient`, `IOAccelerator`, `IOAccelerationUserClient` | Metal device access (Apple Silicon + Intel) |
| `IOHIDLibUserClient` | Keyboard and mouse events |
