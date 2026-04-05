Architecture Change 001: Hybrid Native macOS & Cross-Platform Loco Integration

Goal: 
Transition to a high-performance native macOS experience (FileProvider) while unifying the cross-platform configuration UI using a Rust-native, Rails-like framework (Loco + HTMX).

Core Components:
1. Loco-based Control UI (Rust): A server-side rendered (MVC) web interface for configuration, status, and rules. Replaces React/Tauri.
2. Native FileProvider (Swift): Deep Finder integration for macOS. Replaces FUSE on macOS.
3. Thin macOS Host (Swift): A native Menu Bar utility that manages the lifecycle of the Rust engine and hosts the FileProvider. Communicates with the engine via its REST API — no FFI.
4. Rust Engine (REST API): Core logic (crawling, sync, replication) exposed via the Loco HTTP server. Swift components consume this API over a Unix domain socket on macOS.
5. Local redb Cache: Embedded key-value store for sub-millisecond metadata access.

Implementation Plan:

Phase 1: Loco Bootstrap & redb Prototype
- Bootstrap the Loco web framework within `mosaicfs-agent` with a minimal status endpoint.
- Prototype the redb metadata store in parallel, validating read/write latency.
- These are independent and can land together.

Phase 2: FileProvider Proof-of-Concept (macOS)
- Build a minimal Swift FileProvider extension that enumerates items fetched from the Loco REST API over loopback.
- Validate REST latency for `enumerateItems` and `fetchContents`.
- Validate SSE-based change notifications triggering `signalEnumerator`.
- Gate: if REST latency is insufficient, adjust transport before proceeding.

Phase 3: Full redb Cache & Loco UI
- Refactor `mosaicfs-agent` and `mosaicfs-vfs` logic into a unified "Mosaic Engine" library crate.
- Complete dual-write (CouchDB for federation, redb for local performance).
- Implement the full "Control Center" views using Tera templates and HTMX.
- Expose redb cache status via the Loco UI for real-time monitoring.

Phase 4: Full FileProvider Implementation (macOS)
- Replace the Phase 2 stub with full metadata and on-demand content fetching backed by the real redb cache.
- Implement native Finder sidebar integration and on-demand downloading.
- Finder Sync extension (contextual menus, status badges) is deferred to a follow-on phase.

Phase 5: Thin macOS Menu Bar Host
- Develop a lightweight SwiftUI/AppKit Menu Bar app.
- Role: manage the Rust agent process via launchd, provide "Open Settings" via WKWebView over a Unix domain socket, and manage the FileProvider lifecycle.

Phase 6: Cleanup, Keychain & Packaging
- Implement `secrets_manager` config key with "inline" (default) and "keychain" (macOS) backends.
- On macOS, the bootstrap flow stores the agent secret key directly in the Keychain — the user never copies it manually.
- Deprecate Tauri, React, and FUSE-related code (macOS only; Linux/Windows retain FUSE).
- Package the macOS app as a native `.app` bundle.
- Package the Linux/Windows agent as a single Loco-powered binary.

CI/CD:
- Container CI (existing): builds the Rust workspace, runs all tests, produces the container image. Unaffected by macOS-native work.
- macOS CI (added in Phase 5): validates Swift compilation and `.app` bundle assembly only. Logic tests remain in container CI.

Verification:
- Verify metadata latency (redb) vs existing FUSE benchmarks.
- Validate cross-platform UI consistency (Loco) on macOS, Linux, and Windows.
- Confirm "Double Click to Open" and Finder sidebar functionality on macOS.
- Confirm bootstrap-to-Keychain flow requires zero manual credential handling on macOS.
