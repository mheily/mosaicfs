# Scope Change 1: Drop WebDAV; Use Security-Scoped Bookmarks for Tauri File Access

## What changed and why

Two decisions from the original architecture.md are superseded by this document.

---

## Decision 1: WebDAV is unnecessary

The original architecture proposes serving the MosaicFS virtual filesystem over WebDAV so
the Tauri app can open files from a stable mountpoint (`/Volumes/MosaicFS`). This is
unnecessary.

The NAS is already mounted on the user's laptop via an out-of-band mechanism, and the local
mountpoint is already registered in the database as `network_mounts` on the node document.
The server's existing `open_file_by_id` (in `mosaicfs-server/src/ui/open.rs`) already
performs the full resolution:

```
file_id → source.node_id + source.export_path → network_mounts lookup → local path
```

The only problem with the current code is that the **server** calls `open` on the resolved
path, which is the security boundary violation being fixed. The fix does not require WebDAV.

**New open-file flow:**

```
Server resolves file_id → local path (via network_mounts)
Server returns local path in response (no subprocess spawn)
Tauri app validates path is under an authorized mountpoint
Tauri app calls NSWorkspace.openURL(URL(fileURLWithPath:))
```

The server never calls `open` or `xdg-open`. The Tauri app does the open, but only after
validating the path against the set of mountpoints the user has authorized (see Decision 2).

**What this removes from the original architecture.md:**

- Change 2 (WebDAV routes on Axum) — dropped entirely
- Change 4 (Mount LaunchAgent) — dropped entirely
- Change 5 (UI reconnect action) — dropped (no mount to reconnect)
- The `dav_password` config field — dropped
- Keychain credential seeding — dropped
- `mount_webdav` / `/Volumes/MosaicFS` — dropped

**What remains:**

- Change 1 (server stops spawning `open`, returns path instead) — unchanged
- Change 3 (Tauri plugin for `open_file`) — simplified; receives a local path rather than
  a virtual path, and validates against security-scoped bookmark mountpoints (see below)
  rather than a fixed WebDAV mountpoint prefix

---

## Decision 2: Security-Scoped Bookmarks for Tauri sandbox authorization

The Tauri app is sandboxed. To read from or validate paths on the NAS mount, it needs
explicit OS authorization. The original architecture used
`com.apple.security.temporary-exception.files.absolute-path.read-only` with a hardcoded
path — but the NAS mountpoint is user-configured at runtime and cannot be known at
code-signing time.

### Options considered

| Approach | Problem |
|---|---|
| Hardcoded path entitlement | Mountpoint is user-configured; unknown at signing time |
| `sandbox_init()` self-sandboxing with Seatbelt | Deprecated API (`USE OF THIS FUNCTION IS UNSUPPORTED`); not suitable for `.app` bundles launched via Finder/Dock |
| nullfs bind mount to a known path | Requires a root LaunchDaemon; App Sandbox likely resolves through nullfs to the underlying vnode, making the entitlement ineffective |
| Symlink to known path | App Sandbox resolves symlinks before checking entitlements — same problem as nullfs |

### Chosen approach: Security-Scoped Bookmarks (Powerbox)

On first launch, the Tauri app presents `NSOpenPanel` asking the user to select the NAS
mountpoint directory. The OS grants access via the Powerbox daemon (a separate privileged
process — the app never sees arbitrary filesystem contents). The app then creates a
**security-scoped bookmark** from the selected URL and persists it.

On subsequent launches, the app resolves the bookmark to re-obtain access without showing
the picker again. File access is wrapped in
`url.startAccessingSecurityScopedResource()` / `stopAccessingSecurityScopedResource()`.

If the bookmark goes stale (volume renamed, reformatted, or user revokes access in System
Settings → Privacy & Security), the API returns `isStale: true` and the app re-presents the
picker.

**Bookmarks survive app updates** as long as the bundle identifier (`CFBundleIdentifier`)
stays the same. They are keyed to the bundle ID, not the binary or version number.

**Entitlements required** (replaces the temporary-exception entitlement):

```xml
<key>com.apple.security.files.user-selected.read-only</key>
<true/>
<key>com.apple.security.files.bookmarks.app-scope</key>
<true/>
```

### How this integrates with the open-file flow

The Tauri `open_file` plugin:

1. Resolves the persisted security-scoped bookmark to obtain the authorized mountpoint URL
2. Calls `startAccessingSecurityScopedResource()` on the mountpoint URL
3. Verifies the local path returned by the server is prefixed by the authorized mountpoint
   (path traversal check)
4. Calls `NSWorkspace.openURL(URL(fileURLWithPath: localPath))`
5. Calls `stopAccessingSecurityScopedResource()`

If no bookmark exists yet (first run before setup is complete), `open_file` returns
`MountNotAuthorized` and the UI prompts the user to complete setup.

### Multiple NAS mountpoints

If a user has multiple NAS nodes each mounted at different paths, the app collects one
security-scoped bookmark per mountpoint. The path validation in step 3 above checks against
all authorized mountpoints and uses the matching one.

