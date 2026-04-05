# Review Clarification 001

Responses to the feedback in `review-feedback.md`.

---

## 1. Sled — Agreed

Recommendation: replace with **`redb`**. It is pure-Rust, actively maintained, has a comparable API surface (typed tables, MVCC, ACID transactions), and is a direct drop-in for this use case. `heed` (LMDB) is also viable but adds a C dependency. The architecture and design-notes will be updated to reference `redb` instead of `sled`.

---

## 2. Avoiding UniFFI via REST API — Feasible, and Recommended

Your instinct is correct. UniFFI's value is sharing complex logic across the language boundary. For MosaicFS, the Rust engine already exposes that logic through the Loco REST API. Swift components can consume the API over the loopback interface instead of via FFI, which eliminates the UniFFI build pipeline, the Xcode static library integration, and the ARC/ownership issues with async callbacks.

**Proposed approach:**

| Need | Mechanism |
|---|---|
| FileProvider reads metadata | `GET /api/inodes/{id}` on localhost |
| FileProvider fetches file content | `GET /api/files/{id}/content` on localhost |
| Rust engine notifies FileProvider of changes | **Server-Sent Events** (`GET /api/events`) — the FileProvider holds an open SSE connection; the engine pushes change events, and Swift calls `NSFileProviderManager.signalEnumerator` in response |
| Menu Bar app starts/stops the engine | `launchd` plist + `NSTask` — no FFI needed |
| Menu Bar app opens Settings | `WKWebView` pointed at `http+unix://` (see Minor Note #2) |

The only piece that initially seemed to require callbacks (Rust→Swift) is handled cleanly by SSE, which Loco supports natively. This approach is simpler, more testable, and keeps the language boundary thin. The design-notes will be updated to reflect this.

**One caveat to validate early:** FileProvider has strict latency expectations for `enumerateItems` and `fetchContents`. REST over loopback is typically sub-millisecond when the engine is running locally, but this should be confirmed with a prototype before Phase 3 commits to it. If loopback REST proves too slow for synchronous Finder enumeration, a Unix domain socket transport (still HTTP, just via a socket file) shaves another few hundred microseconds.

---

## 3. CI/CD — Agreed

The CI/CD split is:

- **Container CI** (existing pipeline): builds the Rust workspace, runs unit and integration tests, produces the `mosaicfs-agent` container image. This remains the primary CI path and is not affected by macOS-native work.
- **macOS CI** (separate, later): validates that the Swift code compiles and the `.app` bundle can be assembled. Does not run logic tests — those are covered by the container pipeline.

The architecture will note this explicitly. Phases 1–2 do not touch the macOS CI path at all.

---

## 4. Front-Loading Difficult Work — Agreed, Revised Phase Order

Given no time pressure and the desire to surface showstoppers early, the phasing is revised:

**Phase 1 (was: Loco integration only) → Loco + redb prototype in parallel**
- Bootstrap Loco in `mosaicfs-agent` with a minimal status endpoint.
- Simultaneously prototype `redb` as the metadata store, validating read/write latency against the Sled benchmarks cited in the original design.
- Both can land in the same phase since they are independent of each other.

**Phase 2 (new): FileProvider proof-of-concept**
- Build a minimal macOS FileProvider extension that enumerates a hardcoded list of items fetched from the Loco REST API.
- Validate loopback REST latency for `enumerateItems`.
- Validate SSE-based change notification triggering `signalEnumerator`.
- **Gate on this phase:** if REST latency is insufficient, adjust transport before the full engine is built around it.

**Phase 3 (was Phase 2): Full redb cache integration**
- Complete dual-write (CouchDB + redb), full Loco UI with HTMX, real-time status views.

**Phase 4 (was Phase 3): Full FileProvider implementation**
- Replace the Phase 2 stub with full metadata and on-demand content fetching backed by the real redb cache.

**Phase 5 (was Phase 4): Menu Bar host**
- Thin SwiftUI/AppKit host, launchd management, WKWebView settings window.

**Phase 6 (was Phase 5): Cleanup, packaging, Keychain**
- Deprecate Tauri/React/FUSE (for macOS).
- macOS Keychain integration (see §5 below).
- `.app` bundle packaging.

---

## 5. Secrets Manager Design

**Variables containing secrets** (identified from `agent.toml.example`, `docs/architecture/03-security.md`, and `docs/architecture/08-authentication.md`):

| Config location | Key | Description |
|---|---|---|
| `agent.toml` | `secret_key` | Agent HMAC signing key — shown once at bootstrap, used for all control-plane and P2P requests |
| `agent.toml` | `access_key_id` | Technically public, but co-located with `secret_key`; included for completeness |
| `~/.config/mosaicfs/cli.toml` | `secret_key` | CLI credential secret |
| `~/.config/mosaicfs/cli.toml` | `access_key_id` | CLI credential ID |
| Storage backend config | OAuth tokens | Per-backend; currently stored as encrypted files on the hosting agent |

The `access_key_id` is a public identifier (safe to log), so it does not need Keychain storage. Only `secret_key` and OAuth tokens are sensitive.

**Proposed `secrets_manager` design:**

The Keychain item names are fixed and standardized — they are not configurable in the TOML file. When `secrets_manager = "keychain"`, secret fields (`secret_key`, OAuth tokens) must be absent from the config file entirely; the engine resolves them automatically by looking up the corresponding fixed Keychain item. If a secret field is present alongside `secrets_manager = "keychain"`, the config parser raises an error at startup rather than silently ignoring it.

```toml
# agent.toml — inline mode (default)
secrets_manager = "inline"
access_key_id = "MOSAICFS_7F3A9B2C1D4E5F6A"
secret_key = "mosaicfs_abc123..."

# agent.toml — keychain mode
# secret_key is absent; engine reads from Keychain automatically.
# If secret_key is present here, startup fails with an explicit error.
secrets_manager = "keychain"
access_key_id = "MOSAICFS_7F3A9B2C1D4E5F6A"
```

This means the presence of `secret_key` in the file is itself a visible signal that inline mode is active — useful for auditing.

The `keyring` crate provides the macOS Keychain implementation. On Linux/Windows, `secrets_manager = "keychain"` is unsupported and returns an error at startup until native equivalents are added.

**Standardized Keychain item names:**

| Item name | Maps to |
|---|---|
| `mosaicfs-agent-secret-key` | `agent.toml` → `secret_key` |
| `mosaicfs-cli-secret-key` | `cli.toml` → `secret_key` |
| `mosaicfs-backend-{backend_id}-oauth-token` | Storage backend OAuth token |

The `backend_id` is the unique identifier already present in each storage backend document, so names are stable across restarts.

**First-run UX:** The server already does the right thing. `POST /api/system/bootstrap` validates the token and calls `create_credential` immediately — no Settings → Credentials step is required. DEPLOYMENT.md is wrong on this point and will need to be corrected.

The only friction that remains is what happens after the `POST /api/system/bootstrap` response:

- On macOS (default: `secrets_manager = "keychain"`), the host app intercepts the response, stores `secret_key` directly in the Keychain, writes `agent.toml` with only `access_key_id`, and restarts the agent. The user never sees the secret key.
- On Linux/Windows (`secrets_manager = "inline"`), the web UI displays `access_key_id` and `secret_key` once with a copy button and instructions to write them to `agent.toml`, as today.

If a credential needs to be rotated later, that goes through Settings → Credentials as today.

---

## Minor Notes

**§6 Finder Sync Extension:** Architecture and design-notes will be updated to schedule FileProvider in Phase 4 and defer Finder Sync to a follow-on phase after the core experience is proven.

**WKWebView + localhost:** The agent will bind its Loco server to a Unix domain socket (e.g., `/var/run/mosaicfs/agent.sock`) on macOS. The Menu Bar host opens the socket path via `WKWebView` using a custom `WKURLSchemeHandler` that proxies requests to the Unix socket. This avoids App Transport Security restrictions entirely and removes the need for `NSAllowsLocalNetworking`. On Linux and Windows (where the UI is browser-based rather than embedded in a host app), the agent continues to bind to `localhost:8443`.

---

## Summary of Document Changes Needed

The following changes to `architecture.md` and `design-notes.md` should be made after this discussion is approved:

1. Replace all references to `sled` with `redb`.
2. Remove UniFFI. Replace with REST API (HTTP over Unix socket) + SSE for change notifications.
3. Add note clarifying container CI vs. macOS CI split.
4. Revise phase order (Phases 1–6 as described in §4 above).
5. Promote Keychain/secrets to a concrete deliverable in Phase 6; add `secrets_manager` design.
6. Resequence Finder Sync as a post-FileProvider follow-on.
7. Replace `http://localhost` WKWebView with Unix domain socket + custom URL scheme handler.

Please approve and I will edit `architecture.md` and `design-notes.md` accordingly.
