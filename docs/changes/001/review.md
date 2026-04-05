# Generated with Sonnet 4.6

---
  Review: Architecture Change 001

  Overall Assessment: The direction is sensible — FileProvider is the right call over FUSE on macOS, and replacing Tauri/React with a Loco + HTMX SSR approach
  reduces frontend complexity significantly. That said, there are several areas worth scrutinizing.

  ---
  Strengths

  FileProvider over FUSE — correct choice. FUSE on macOS is increasingly painful (requires kernel extension approval, third-party kext drama, Apple Silicon
  compatibility issues). FileProvider is the supported, sandboxed path and gives first-class Finder sidebar integration.

  Loco + HTMX — a reasonable tradeoff. Eliminating React/Tauri removes a large dependency surface and the complexity of the JS build pipeline. The config UI is
  CRUD-heavy, which is exactly where HTMX excels. Loco is young but actively maintained.

  Sled for local cache — good fit for metadata latency requirements. Its MVCC model handles the concurrent-reader scenario (Loco UI + Swift FileProvider via FFI)
  cleanly.

  ---
  Concerns

  1. Sled is effectively unmaintained.
  The sled crate (github.com/spacejam/sled) has had minimal activity since ~2023 and the sled 1.0 release has been stalled for years. The author has acknowledged
  burnout. Before committing to it as a core dependency, evaluate redb or heed (LMDB wrapper) as alternatives with better maintenance trajectories. redb in
  particular is pure-Rust, actively developed, and has similar performance characteristics.

  2. UniFFI complexity is underestimated.
  The design-notes describe UniFFI as if it's straightforward plumbing, but in practice:
  - Async callbacks from Rust back to Swift (the "wake up FileProvider" use case) are a known pain point in UniFFI. The signalEnumerator flow described in Future
  Question #2 requires uniffi::callback_interface!, which has subtle lifecycle/ownership issues with Swift ARC.
  - UniFFI generates a .xcframework or static lib + headers. That needs to be integrated into an Xcode project with proper codesigning for the FileProvider
  extension, which must be embedded in the .app bundle with its own entitlements. This is non-trivial CI/CD work.

  Consider prototyping the Rust→Swift callback path early in Phase 3 — it's the highest-risk technical unknown in the plan.

  3. The Loco integration model conflicts with the existing deployment model.
  The current CLAUDE.md says: rebuild and redeploy via make mosaicfs-image && podman kube play. The agent runs in a container. Loco embedded in the agent binary
  is fine for that. But Phase 4 embeds Loco into a native macOS .app bundle. These are two fundamentally different deployment targets for the same binary — one
  containerized/server-side, one local macOS process. The plan should explicitly state which phases break the current container-based deployment, and whether
  that's intentional.

  4. Phase ordering risk.
  The plan puts Sled cache (Phase 2) before FileProvider (Phase 3). This is correct since FileProvider depends on the cache. However, it also means Phase 1 (Loco
  integration) must land before any macOS-native work begins. If Loco integration hits friction (e.g., template rendering for a complex sync-status view), it
  could block the native work. Consider whether the Sled cache can be prototyped in parallel with Phase 1 rather than sequentially after it.

  5. Future Question #1 (Keychain) is not really optional.
  Storing CouchDB credentials in a plain .toml file inside a .app bundle is a sandbox violation waiting to happen — macOS App Sandbox restrictions and
  notarization requirements will surface this. If the app is ever submitted to the Mac App Store or requires Hardened Runtime, Keychain access is mandatory. The
  keyring crate supports this. This should be moved from "Future Questions" to a Phase 5 task.

  ---
  Minor Notes

  - Design-notes §6 (Finder Sync Extension) describes a separate Finder Sync extension alongside the FileProvider. These serve different purposes and should not
  both be implemented initially — FileProvider handles the core sync; Finder Sync adds contextual menus and badges. The plan should sequence these (FileProvider
  first, Finder Sync as a follow-on).
  - The WKWebView approach for the Settings window (§5) is fine but note that WKWebView in a sandboxed app can't load http://localhost by default — you'll need
  NSAllowsLocalNetworking in the App Transport Security config, or use a Unix domain socket instead of TCP.

  ---
  Answers to the Four Future Questions

  1. Keychain — yes, use keyring. Not optional for a shipped app.
  2. Wake-up strategy — UniFFI callback interface → signalEnumerator. Prototype early.
  3. VFS on Linux/Windows — keep fuser in the engine crate for now. Don't solve Windows/Linux native cloud sync until the macOS path is proven.
  4. Initial sync UX — Loco progress page that polls a Sled-backed /api/status endpoint via HTMX hx-trigger="every 2s". Activate FileProvider only after the
  initial enumeration is complete.

