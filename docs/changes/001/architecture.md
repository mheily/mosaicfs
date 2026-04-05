Architecture Change 001: Hybrid Native macOS & Cross-Platform Loco Integration

Goal: 
Transition to a high-performance native macOS experience (FileProvider) while unifying the cross-platform configuration UI using a Rust-native, Rails-like framework (Loco + HTMX).

Core Components:
1. Loco-based Control UI (Rust): A server-side rendered (MVC) web interface for configuration, status, and rules. Replaces React/Tauri.
2. Native FileProvider (Swift): Deep Finder integration for macOS. Replaces FUSE.
3. Thin macOS Host (Swift): A native Menu Bar utility to manage the lifecycle of the Rust engine and host the FileProvider.
4. Embedded Rust Engine (FFI): Core logic (crawling, sync, replication) compiled as a shared library.
5. Local Sled Cache: Lock-free sidecar for sub-millisecond metadata access.

Implementation Plan:

Phase 1: Rust Engine & Loco Integration
- Refactor `mosaicfs-agent` and `mosaicfs-vfs` logic into a unified "Mosaic Engine" library crate.
- Bootstrap the Loco web framework within the agent to provide the HTML/MVC control interface.
- Implement the "Control Center" views using Tera templates and HTMX for minimal-JS interactivity.

Phase 2: Local Sled Cache Integration
- Implement the Sled-based metadata cache within the Rust engine.
- Update the crawler to perform dual-writes: CouchDB (federation) and Sled (local UI/Finder performance).
- Expose the Sled cache via the Loco UI for real-time status monitoring.

Phase 3: Native FileProvider Development (macOS)
- Create a FileProvider Extension in Swift/Xcode.
- Use UniFFI to bridge the Swift extension to the Rust engine's Sled-backed metadata and file-fetching logic.
- Implement on-demand downloading and native Finder sidebar integration.

Phase 4: Thin macOS Menu Bar Host
- Develop a lightweight SwiftUI/AppKit Menu Bar app.
- Role: Launch/monitor the Rust agent (Loco server), provide "Open Settings" (pointing to the Loco web UI), and manage the FileProvider lifecycle.

Phase 5: Cleanup & Cross-Platform Delivery
- Deprecate Tauri, React, and FUSE-related code.
- Package the Windows/Linux agent as a single "Loco-powered" binary.
- Package the macOS app as a native `.app` bundle containing the engine, Loco UI, and FileProvider extension.

Verification:
- Verify metadata latency (Sled) vs existing FUSE benchmarks.
- Validate cross-platform UI consistency (Loco) on macOS, Linux, and Windows.
- Confirm "Double Click to Open" and Finder sidebar functionality on macOS.
