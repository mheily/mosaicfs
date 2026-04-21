# Change 011: Secure File Access via WebDAV

## Current State Summary

_Condensed from the inventory auto-generated at ba451f9. Supplemented with targeted reads of the files this change touches._

**Workspace crates relevant to this change:**

- `mosaicfs-server` (9,225 lines) — Axum server, hosts `/api/*` and `/ui/*` routes. Currently on HTTP port 8443.
- `mosaicfs-common` — shared config, CouchDB client, documents.
- `mosaicfs-vfs` — contains a partial FUSE implementation (`fuse_fs.rs`, 561 lines with real `lookup`/`getattr`/`readdir`/`open`/`read`) that is **not wired into runtime** (`start_vfs` parks forever). Not used by this change.
- `desktop/` — Tauri shell. Current content: a single `WebviewWindowBuilder` pointed at `http://localhost:8443/ui/browse`. No plugins, no custom Rust beyond `lib.rs`. Entitlements: `app-sandbox` + `network.client` only (no filesystem, no process execution).

**Files directly touched by this change:**

- `mosaicfs-server/src/ui/open.rs` — current `open_file_by_id` resolves a file_id to a local path via `node_doc.network_mounts`, then spawns `open` (macOS) or `xdg-open` (Linux). This is the behavior being removed.
- `mosaicfs-server/src/ui/browse.rs:226` and `mosaicfs-server/src/ui/actions.rs:22` — the two callers of `open_file_by_id`.
- `mosaicfs-server/src/handlers/files.rs` — `get_file_content` already supports `Range` headers and streams responses. Reusable for the WebDAV `GET` path.
- `mosaicfs-server/src/routes.rs` — 91 API routes + 47 UI routes; WebDAV verbs are new.
- `desktop/Entitlements.plist` — needs a temporary-exception for `NSWorkspace.openURL` (file URLs are permitted; the exception is only needed if we invoke an external helper, which we no longer do).
- `desktop/src/lib.rs` — gains a Tauri plugin invoked by the browse UI's open-file action.

**Deployment today:** two-container pod (`couchdb` + `mosaicfs`) built from `Dockerfile.mosaicfs`. This change does not alter the pod composition.

**External services:** CouchDB (unchanged), S3 replication (unchanged).

---

## Goal

Replace the MosaicFS server's direct invocation of `open`/`xdg-open` with an indirection where the Tauri desktop app performs the open. Make the MosaicFS filesystem tree available on the user's Mac at a stable local path (`/Volumes/MosaicFS`) via a WebDAV share served by the existing Axum process, mounted at login by a LaunchAgent.

The server no longer triggers native-app launches. The browse UI returns a virtual path; the Tauri app validates it against the mountpoint and calls `NSWorkspace.openURL`.

---

## Changes

### Change 1: Server `/ui/browse/open` stops spawning `open`

**Today:** `mosaicfs-server/src/ui/open.rs::open_file_by_id` resolves the file_id to a local path (via `network_mounts` in the node document), then calls `std::process::Command::new("open").arg(path)`. Any `file_id` the caller can route to this handler becomes a path the server process opens.

**Proposed:** The handler resolves the file_id to a **virtual path** (the MosaicFS-internal path — the same string the VFS tree uses) and returns it to the caller. No subprocess spawn. The import of `open_file_by_id` in `ui/actions.rs:22` has no call sites (compiler warns as unused) and is removed in the same phase. `open.rs` shrinks to virtual-path resolution; the `network_mounts`-based local-path resolution, the `Command::new("open")` call, and `summarize_open_error` all go away.

**Justification:** The server currently runs as a separate build account specifically so it has limited blast radius. That containment is undermined when the server process calls `open` on attacker-controllable paths. Returning a string moves the open decision to a principal (the Tauri app + user session) that is already trusted with the user's files.

### Change 2: WebDAV routes on the Axum server

**Today:** No WebDAV support. File content is served by `GET /api/files/{id}/content` with JWT auth and `Range` support.

**Proposed:** Add a read-only WebDAV surface at `/dav/*`:

| Verb | Purpose |
|---|---|
| `OPTIONS` | Advertise `DAV: 1` class, `Allow: OPTIONS, HEAD, GET, PROPFIND` |
| `PROPFIND` | Directory listing (Depth 0 and 1), file metadata as multistatus XML |
| `GET` | File content (reuses the Range-capable streaming from `get_file_content`) |
| `HEAD` | File metadata without body |

Write verbs (`PUT`, `DELETE`, `MKCOL`, `LOCK`, `UNLOCK`, `PROPPATCH`, `COPY`, `MOVE`) return `405 Method Not Allowed` with a stable `Allow` header.

Authentication: HTTP Basic against a new `dav_password` config field, distinct from the JWT session scheme. The password is seeded into the user's Keychain at first run (see Change 4).

Implementation: Hand-rolled handlers on Axum. The read-only surface is four verbs and a single XML response shape (`<D:multistatus>` with `<D:response>` children); owning ~400–800 lines of code avoids a dependency on a niche crate for a protocol that rarely changes. Budget revisits if `PROPFIND` XML edge cases on real macOS clients prove more costly than expected — at which point the `dav-server` crate becomes the fallback, not the default.

**Justification:** WebDAV is the smallest standard protocol that Finder, Quick Look, and native apps understand as a read-only network volume. It reuses the existing Axum runtime, HTTP port, TLS story, and file-content serving code. No new binary, no new container.

### Change 3: Tauri plugin for `open_file`

**Today:** `desktop/src/lib.rs` is a bare webview. No plugins. No Rust logic beyond window creation.

**Proposed:** Add a Tauri command `open_file(virtual_path: String) -> Result<(), OpenError>` behind a platform trait:

```rust
fn open_file(virtual_path: &str) -> Result<(), OpenError>

enum OpenError {
    MountNotAvailable,
    PathTraversal,
    FileNotFound,
    OpenFailed(String),
}
```

**macOS implementation:**

1. Join `/Volumes/MosaicFS` with the virtual path.
2. Canonicalize the result (`std::fs::canonicalize` — resolves `..`, symlinks, and the `/private/var/...` shadow path macOS sometimes exposes for `/Volumes/*`).
3. Verify the canonical path still starts with the canonical form of the mountpoint. If not, return `PathTraversal`.
4. Verify the file exists. If the mountpoint directory does not exist, return `MountNotAvailable`.
5. Call `NSWorkspace.openURL(URL(fileURLWithPath:))` via `objc2` bindings.

**Linux/Windows:** Trait stubs that return `OpenFailed("not implemented on this platform")` for v1. Mount mechanism and open command documented for future implementation but not built.

**Browse UI wiring:** The current `POST /ui/browse/open` response is a flash message. It becomes a JSON response `{ "virtual_path": "..." }` consumed by JS in the browse template, which invokes the Tauri command. When the Tauri command returns `MountNotAvailable`, the UI shows a flash with a "Reconnect" button that triggers Change 5's reconnect action.

**Justification:** The Tauri app is the right place for this — it's the process the user's session trusts to operate on their behalf, it's sandboxed so it can't be misused to read file contents, and `NSWorkspace.openURL` with a `file://` URL is permitted for sandboxed apps without additional entitlements.

### Change 4: Mount LaunchAgent

**Today:** No LaunchAgent. The Tauri app ships as a single `.app` bundle.

**Proposed:** A per-user LaunchAgent that mounts `/Volumes/MosaicFS` at session login:

- Plist path (modern): registered via `SMAppService.agent(plistName:)` from within the Tauri app bundle at `Contents/Library/LaunchAgents/com.mosaicfs.mount.plist`. User approves in System Settings → Login Items on first launch.
- Agent program: a small shell script (or tiny Swift/Rust binary, ~30 lines) that:
  1. Checks if `/Volumes/MosaicFS` is already mounted (idempotent re-launch).
  2. Runs `mount_webdav -o nobrowse http://localhost:8443/dav /Volumes/MosaicFS`.
     - Note: credentials are pulled from Keychain by host+realm; the first-run flow (below) seeds them.
- `KeepAlive`: `{ SuccessfulExit: false, NetworkState: true }` — relaunches on mount failure and on network-up transitions. `ThrottleInterval: 30` to avoid hammering.

**First-run credential seeding:** On first launch of the Tauri app, if no `mosaicfs_dav` Keychain item exists, prompt the user for the WebDAV password (the same password configured server-side in `dav_password`), write it to Keychain via `SecItemAdd` with attributes `{ server: "localhost", port: 8443, protocol: kSecProtocolTypeHTTP, authentication: kSecAuthenticationTypeHTTPBasic, path: "/dav" }`. Then trigger the LaunchAgent via `launchctl kickstart`.

**Spotlight exclusion:** Two defenses:
1. The WebDAV server synthesizes a zero-byte `.metadata_never_index` at the share root.
2. The `-o nobrowse` mount flag hides the volume from Finder and excludes it from Spotlight.

Both should be verified in practice on the target macOS version before accepting the implementation.

**Justification:** The Tauri app is sandboxed and cannot invoke `mount_webdav`. Moving the mount to a LaunchAgent puts it in the user's session but outside the sandbox. A LaunchAgent is also the right home for auto-restart behavior (sleep/wake, network transitions).

### Change 5: UI reconnect action

**Today:** N/A (no mount to reconnect).

**Proposed:** When the Tauri plugin returns `MountNotAvailable`, the browse UI shows a flash message "Share not mounted" with a "Reconnect" button. The button invokes a second Tauri command that kicks the mount agent via the `SMAppService` Objective-C API (`SMAppService.agent(plistName:).unregister()` followed by `.register()`, or equivalent — the sandbox permits this because `SMAppService` is designed for sandboxed apps to manage their own bundled helpers). It then retries the open. Spawning `launchctl` as a subprocess is **not** an option — the Tauri sandbox blocks process execution, and adding an exception would widen the attack surface for the sake of a convenience the framework API already provides.

**Justification:** With no temp-file fallback (deferred), auto-restart via `KeepAlive` plus a manual reconnect covers the realistic recovery scenarios without adding a degraded-open-a-copy code path.

---

## Implementation Phases

Phases are organized by topical concern, not deployability. Tree may not be usable between phases; final state is what matters.

### Phase 1 — WebDAV protocol surface

Add `/dav/*` routes to `mosaicfs-server`. Integrate the `dav-server` crate if compatible; otherwise hand-roll `OPTIONS`/`PROPFIND`/`GET`/`HEAD`. Reuse `files.rs::serve_file_content` for the `GET` body. Map MosaicFS virtual-path hierarchy (directory documents + file documents in CouchDB) to the WebDAV tree. Synthesize `.metadata_never_index` at root. Add `dav_password` config field.

**Acceptance:** `mount_webdav` from a macOS Terminal against `http://localhost:8443/dav` produces a working mount; Finder can browse; `ls`, `cat`, Quick Look, and native-app `open` all work against files under the mountpoint.

### Phase 2 — Mount infrastructure

Author the LaunchAgent plist. Write the agent program (shell script or tiny binary). Implement the Tauri app's first-run Keychain-seeding flow. Register the agent via `SMAppService`. Implement the reconnect command.

**Acceptance:** Logging into the desktop session produces a mounted `/Volumes/MosaicFS`. Putting the laptop to sleep and waking it produces a remount within 30 seconds (or on next network-up). The reconnect command works.

### Phase 3 — Open-file flow rewrite

Change `open_file_by_id` to return a virtual path. Remove the `std::process::Command::new("open")` call. Update `ui/browse.rs` and `ui/actions.rs` to the new return type. Update the browse HTML/HTMX to invoke the Tauri plugin on open clicks rather than relying on the server. Implement the Tauri `open_file` plugin (canonicalize, prefix-check, `NSWorkspace.openURL`). Surface `MountNotAvailable` in the UI with the reconnect button.

**Acceptance:** Clicking a file in the browse UI opens it in its native app via the mount. The server process never calls `open` or `xdg-open`. A crafted `file_id` whose virtual path resolves outside `/Volumes/MosaicFS` is rejected.

**Cross-phase dependency:** Phase 3 needs Phase 1's WebDAV endpoint (otherwise the mount has no content) and Phase 2's mount (otherwise the NSWorkspace call fails). Phase 2 needs Phase 1 so the agent has something to mount.

---

## What Does Not Change

- **`mosaicfs-vfs` crate.** The partial FUSE implementation remains unwired. Not used by this change.
- **Pod composition.** Two containers (`couchdb`, `mosaicfs`). No `mosaicfs-smb` image, no third container.
- **REST API surface** — `/api/*` routes are unchanged. `GET /api/files/{id}/content` keeps its JWT auth.
- **JWT session auth.** WebDAV uses a separate `dav_password`; this is intentional so a WebDAV credential leak doesn't escalate to full API access.
- **Replication, CouchDB federation, agent crawlers.** Out of scope.
- **Linux and Windows clients.** Tauri plugin trait has non-macOS implementations stubbed to return `OpenFailed`; actual implementation deferred.
- **Port.** Server stays on 8443. WebDAV is a path prefix (`/dav`), not a separate port.
- **TLS posture.** Server is currently HTTP-only (per `desktop/src/lib.rs:8` pointing at `http://localhost:8443`). This change does not introduce TLS. WebDAV Basic Auth over localhost HTTP is acceptable because traffic doesn't leave the machine; if the server moves to a NAS in a later change, TLS becomes a prerequisite at that point.

---

## Deferred

- **XPC helper (was planned as change 012).** Extra isolation against a compromised Tauri plugin chain. Not justified by the current threat model (untrusted server, trusted Tauri app). The plugin's `open_file` trait is designed so an XPC-backed implementation can be slotted in without touching the server or the browse UI. Delete `docs/changes/012/` as part of this change.
- **Temp-file fallback (open-a-copy when mount unavailable).** Degraded UX (opens a copy, edits lost). Auto-restart via `KeepAlive` + the manual reconnect action cover the realistic scenarios. Add later if real-world use reveals cases they don't cover.
- **Linux WebDAV consumption (`davfs2`, `gvfs`).** Tauri plugin stubbed. Add when Linux desktop support is prioritized.
- **Windows WebDAV consumption (WebClient service, mapped drive).** Same as Linux.
- **Multiple NAS / multiple mountpoints.** v1 assumes one server, one mountpoint.
- **WebDAV TLS.** See "TLS posture" above. Prerequisite for any deployment where the Tauri app and server are not on the same machine.
- **Sleep/wake stale-mount detection.** `KeepAlive { NetworkState: true }` handles the common case. A reachability-check-and-remount path in the Tauri app is a belt-and-suspenders addition if stale mounts turn out to be common in practice.
- **Full FUSE implementation in `mosaicfs-vfs`.** Orthogonal to this change. Useful for non-Mac clients (or Mac clients that don't want WebDAV) in the future.
- **Server-side `dav_password` rotation UX.** Manual edit of config + Keychain re-seed for v1.

---

## Open items to resolve during implementation

1. **`dav-server` crate compatibility.** Confirm the crate integrates with the workspace's Axum 0.7 and tokio versions before committing to it. If incompatible, hand-rolled WebDAV becomes a larger piece of phase 1.

2. **Virtual-path → WebDAV URL encoding.** MosaicFS virtual paths may contain spaces, Unicode, and characters that need percent-encoding. Unicode normalization (NFC vs NFD) on lookup needs to be settled — macOS Finder sends NFD; CouchDB documents likely hold NFC.

3. **Canonicalization of `/Volumes/MosaicFS` prefix-check.** Verify both the naive path and the `/private/var/folders/.../Volumes/...` shadow form are handled. Canonicalize the mountpoint once at plugin init and compare canonicalized file paths against it.

4. **`SMAppService` on the minimum supported macOS.** The project targets modern macOS. `SMAppService` is macOS 13+. Older-macOS support requires the legacy `SMLoginItemSetEnabled` path, which is more complex. Settle the minimum target before phase 2.

5. **First-run DAV password provisioning.** The server's `dav_password` must exist before the Tauri app's first-run Keychain seed is meaningful — otherwise the user is typing a password into a Keychain dialog that doesn't match anything server-side. The existing bootstrap-token pattern (`actions.rs:41+`, writes `state.data_dir.join("bootstrap_token")` at first start) is the natural template: have the server generate `dav_password` on first launch and write it to a file (e.g. `dav_password`) that an admin reads once to configure the Tauri app, or that the Tauri app reads directly if it runs on the same host as the server. Avoid a design that requires a manual config-file edit between `mosaicfs` starting and the Tauri app launching.
