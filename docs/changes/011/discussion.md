# Changes 011+012: Secure File Access from the Tauri App

## The unified question

How does a user click a file in the sandboxed Tauri app and have it open in a native macOS
app, given that:

- The Tauri app is sandboxed (App Sandbox, no filesystem entitlements) running as the main user
- The MosaicFS server is unsandboxed, running as a separate build account
- The server must not be able to trigger `open` on arbitrary paths on the main user's account
- Files live on a remote NAS; the server exposes them via its virtual filesystem

This is one architecture question with two moving parts: how to expose the files as a
locally-accessible path (011), and how to open them safely from the sandbox (012). The
choice made in 011 directly determines what 012 needs to be, so they are designed together
here.

---

## Constraints

- **Read-only permanently.** MosaicFS is a consumption layer, not a write path.
- **Target NAS OS:** Debian.
- **Architecture matters more than delivery speed.**
- **The server returns a virtual path.** It no longer calls `open` directly — that change
  is agreed regardless of which option is chosen here.

---

## The option space

### Option 1: WebDAV mount + NSWorkspace in Tauri

**011 part:** MosaicFS server gains WebDAV routes (`PROPFIND`, `GET`, `HEAD`). The Tauri
app mounts the share at startup:

```sh
mount_webdav -o nobrowse http://server:8443/dav /Volumes/MosaicFS
```

**012 part:** No XPC helper. The Tauri app constructs a `file://` URL from the virtual path
and the known mountpoint, validates the path is under the mountpoint (prevents traversal),
then calls `NSWorkspace.openURL()` via a thin Tauri plugin.

```
Server → virtual path → Tauri validates prefix → NSWorkspace.open(file:///Volumes/MosaicFS/...) → native app
```

`NSWorkspace.openURL()` with a `file://` URL is permitted from sandboxed apps — this is
standard Mac App Store behaviour. The sandbox prevents the app from *reading* the file; it
does not prevent the app from asking the system to open it with another application.

**Security properties:**
- Server cannot trigger `open` on arbitrary paths (it only returns a virtual path string)
- Path validation is in the Tauri plugin (in-process, not a separate auditable binary)
- The sandboxed Tauri app cannot read or exfiltrate file contents
- A compromised Tauri app could in principle open arbitrary `file://` URLs — but a
  compromised Tauri app is already inside the main user account, so this is not a
  meaningful regression

**Simplicity:** No XPC helper binary, no Launch Agent, no IPC protocol. The 012 work
reduces to ~50 lines of Rust in a Tauri plugin plus the mount lifecycle management.

---

### Option 2: WebDAV mount + XPC helper

Same WebDAV server as Option 1. The Tauri app communicates with a small out-of-process
Swift helper (Launch Agent) over XPC rather than calling NSWorkspace directly.

```
Server → virtual path → Tauri → XPC → helper validates prefix → NSWorkspace.open() → native app
```

The helper is the only binary that calls `open`. It enforces the mountpoint constraint
independently of the Tauri app.

**Additional security over Option 1:** path validation lives in a separate process that is
small, auditable, and cannot be influenced by the sandboxed Tauri app's memory. This matters
if the threat model includes a compromised Tauri app (e.g. a malicious Tauri plugin). Against
that threat, the XPC helper is a meaningful boundary — the helper validates the path
regardless of what the Tauri app sends.

**Cost:** Swift XPC helper binary, Launch Agent plist, install UX, Tauri plugin that wraps
XPC, mach-lookup entitlement exception.

---

### Option 3: No mount — stream to temp file

No network share at all. The Tauri app downloads the file via `GET /api/files/{id}/content`
to its sandbox temp directory, then calls `NSWorkspace.openURL()` on the temp file.

**Advantages:** No mount lifecycle to manage. Works even if the NAS share is offline.

**Disadvantages:**
- Large files are slow to open (full download before native app launches)
- Temp files accumulate unless cleaned up explicitly
- The native app opens a *copy*, not the file in place — edits don't go anywhere
- For a read-only system this is tolerable, but it feels wrong to the user

**Verdict:** Viable fallback for when the mount is unavailable, but not the primary path.

---

### Option 4: Samba VFS container + XPC helper

The original 012 design. A `mosaicfs-smb` OCI container runs a custom Samba VFS module
that proxies filesystem calls to the MosaicFS HTTP API. The Tauri app mounts via SMB; the
XPC helper validates paths under the SMB mountpoint.

**Advantage over WebDAV:** SMB is a more capable protocol with better macOS integration and
Spotlight-over-SMB support.

**No longer recommended.** The Samba VFS module was always going to call the MosaicFS HTTP
API — it was a C shim around the same HTTP calls WebDAV serves directly. Adding a container,
C code, and Samba version management for a protocol that has the same per-operation HTTP cost
as WebDAV is not justified by the requirements.

Documented here as the fallback if macOS SMB-specific features (Spotlight, richer ACLs)
become hard requirements.

---

## Recommendation

**Option 1 (WebDAV + NSWorkspace in Tauri)** to start.

The threat it doesn't defend against — a compromised Tauri app opening arbitrary paths — is
a threat where the attacker is already inside the main user account. The meaningful threat
(the untrusted MosaicFS server triggering arbitrary opens) is fully addressed by the server
returning a virtual path and the Tauri app doing prefix validation.

**Option 2 (+ XPC helper)** is the right next step if:
- The project matures to the point where the Tauri plugin dependency chain itself becomes a
  trust concern
- A security review identifies the in-process validation as insufficient
- The install experience can absorb the additional setup complexity

Design the Tauri plugin interface to be compatible with Option 2 from the start — the
difference between calling NSWorkspace directly and calling it via XPC is one abstraction
boundary in the plugin. Option 2 can be slotted in without changing the server or the rest
of the Tauri app.

---

## WebDAV server design (011 implementation notes)

Implemented as routes on the existing Axum server. No new process, no new port.

**Required verbs (read-only):**

| Verb | Purpose |
|---|---|
| `OPTIONS` | Announce WebDAV support |
| `PROPFIND` | Directory listing + file metadata |
| `GET` | File content |
| `HEAD` | Metadata without body |

Write verbs (`PUT`, `DELETE`, `MKCOL`, etc.) return `405 Method Not Allowed`.

**Spotlight exclusion (both defenses):**
- Synthesize `.metadata_never_index` as a zero-byte virtual file in the WebDAV root —
  Spotlight respects this as a signal to skip the volume
- Tauri always mounts with `-o nobrowse` — hides the volume from Finder and excludes it
  from Spotlight regardless of file contents

**Implementation:** Direct Axum handlers (~400 lines of Rust) rather than the `dav-server`
crate. The read-only surface is small enough that owning the implementation is preferable to
a dependency on a niche crate.

---

## Tauri plugin design (012 implementation notes)

Single Rust function behind a platform trait:

```rust
fn open_file(virtual_path: &str) -> Result<(), OpenError>
```

**macOS (Option 1):** Construct `file:///Volumes/MosaicFS/<virtual_path>`, validate prefix,
call `NSWorkspace.openURL()` via objc2 bindings.

**macOS (Option 2 upgrade path):** Same interface, XPC call instead of direct NSWorkspace.

**Linux:** `xdg-open` directly (no sandbox enforcement on Linux by default).

**Windows:** `ShellExecuteW` with UNC path or mapped drive letter.

**Mount lifecycle:** The Tauri app manages `mount_webdav` at startup and unmount at quit.
If the mount fails or becomes unavailable, `open_file` returns `MountNotAvailable` and the
UI offers Option 3 (stream to temp) as a fallback.

---

## Open questions

1. **Auto-mount at login vs at app launch?** A Login Item that mounts the WebDAV share before
   the Tauri app opens would make the volume available to other apps too (though `nobrowse`
   limits visibility). App-launch mount is simpler and scopes the volume lifetime to the app.

2. **Credential for the WebDAV mount.** The mount needs credentials separate from the main
   API JWT (which is session-scoped). A dedicated long-lived `dav_password` config field,
   distinct from the API key system, is the cleanest approach.

3. **Multiple NAS / multiple mountpoints.** Deferred to a future change. For v1, one
   configured server URL and one mountpoint.
