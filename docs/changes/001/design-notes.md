Design Notes 001: Technical Implementation Details

### 1. Unified Control UI: Loco + HTMX

To replace the React-based Tauri UI, we will move to a server-side MVC architecture using [Loco](https://loco.rs/) and [HTMX](https://htmx.org/).

*   **Architecture:** MVC (Model, View, Controller) in Rust.
*   **Templates:** Use Tera (similar to Django/Jinja2) for server-side HTML rendering.
*   **Interactivity:** HTMX will handle dynamic updates (e.g., sync progress bars, status badges) without custom JavaScript.
*   **Integration:** Loco will be integrated directly into the `mosaicfs-agent` process, sharing state with the Crawler and redb cache.
*   **Transport:** On macOS the agent binds to a Unix domain socket (`/var/run/mosaicfs/agent.sock`). On Linux and Windows it binds to `localhost:8443` as today.

### 2. redb Key-Value Cache

The [redb](https://github.com/cberner/redb) embedded key-value store provides the sub-millisecond responsiveness required for Finder and the Control UI. redb is pure-Rust, actively maintained, and provides ACID transactions with MVCC.

*   **Data Layout:**
    *   `inodes` table: key `u64` inode ID → value serialized `Inode` struct (using `bincode`).
    *   `status` table: key `&str` file ID → value serialized `SyncStatus` struct.
*   **Dual-write:** The crawler writes to both CouchDB (federation) and redb (local performance). redb is the authoritative source for the Loco UI and FileProvider; CouchDB remains the authoritative source for federation and replication.
*   **Concurrent Access:** The Loco web server and the Swift FileProvider extension both query redb via the Loco REST API — there is no direct redb access from Swift.

### 3. Swift–Rust Integration: REST API + SSE

UniFFI is not used. Swift components communicate with the Rust engine exclusively through its Loco HTTP API, eliminating the FFI build pipeline and Xcode static library integration.

| Need | Mechanism |
|---|---|
| FileProvider reads metadata | `GET /api/inodes/{id}` |
| FileProvider fetches file content | `GET /api/files/{id}/content` |
| Rust engine notifies FileProvider of changes | Server-Sent Events: `GET /api/events` — FileProvider holds an open SSE connection; on receiving a change event it calls `NSFileProviderManager.signalEnumerator` |
| Menu Bar app starts/stops the engine | `launchd` plist + `NSTask` |
| Menu Bar app opens Settings | `WKWebView` via custom `WKURLSchemeHandler` proxying to the Unix domain socket |

All HTTP traffic between Swift and Rust travels over the Unix domain socket, avoiding TCP overhead and App Transport Security restrictions entirely.

**Latency validation (Phase 2 gate):** FileProvider has strict latency requirements for `enumerateItems` and `fetchContents`. REST over a Unix socket is expected to be sub-millisecond, but this must be confirmed with the Phase 2 proof-of-concept before the full implementation commits to this transport.

### 4. macOS FileProvider Architecture

*   **Native Finder Locations:** MosaicFS will appear in the Finder sidebar as a first-class Location.
*   **On-Demand Fetching:** Files in redb marked with `remote_only` will show a cloud icon in Finder. When double-clicked, the FileProvider calls `GET /api/files/{id}/content` on the Rust engine to fetch the file from a remote node.
*   **Change Notifications:** The Rust engine pushes change events to the FileProvider via SSE (`GET /api/events`). On receiving an event, the FileProvider calls `NSFileProviderManager.signalEnumerator` to refresh Finder.
*   **Finder Sync Extension** (contextual menus, per-file sync status badges) is deferred to a follow-on phase after the core FileProvider experience is proven.

### 5. Thin macOS Menu Bar Host

*   **Role:** Background manager for the `mosaicfs-agent` process.
*   **Process management:** The host uses a `launchd` plist to start and stop the agent. `NSTask` is used for on-demand restarts.
*   **Settings window:** A `WKWebView` with a custom `WKURLSchemeHandler` (e.g., scheme `mosaicfs://`) that proxies HTTP requests to the agent's Unix domain socket. This provides a native-feeling window without App Transport Security issues and without binding a TCP port.

### 6. Secrets Manager

*   **`secrets_manager` config key:** Controls where secret values are read from.
    *   `"inline"` (default): secret values are stored as literal strings in the config file. This is the existing behavior and the default on all platforms.
    *   `"keychain"`: secret fields must be absent from the config file; the engine resolves them by looking up fixed Keychain item names. Raises a startup error if a secret field is present alongside this setting. macOS only in Phase 6; Linux/Windows support deferred.

*   **Config examples:**

    ```toml
    # inline mode (default, all platforms)
    secrets_manager = "inline"
    access_key_id = "MOSAICFS_7F3A9B2C1D4E5F6A"
    secret_key = "mosaicfs_abc123..."

    # keychain mode (macOS)
    # secret_key must be absent — engine reads from Keychain automatically.
    secrets_manager = "keychain"
    access_key_id = "MOSAICFS_7F3A9B2C1D4E5F6A"
    ```

*   **Standardized Keychain item names** (not user-configurable):

    | Item name | Maps to |
    |---|---|
    | `mosaicfs-agent-secret-key` | `agent.toml` → `secret_key` |
    | `mosaicfs-cli-secret-key` | `cli.toml` → `secret_key` |
    | `mosaicfs-backend-{backend_id}-oauth-token` | Storage backend OAuth token |

*   **Bootstrap flow on macOS:** The existing `POST /api/system/bootstrap` endpoint already creates the initial credential and returns `access_key_id` + `secret_key` in a single step. On macOS, the Menu Bar host app intercepts this response, stores `secret_key` directly in the Keychain, and writes `agent.toml` with `secrets_manager = "keychain"` and `access_key_id` set. The user never sees or copies the secret key. On Linux/Windows the web UI displays the credentials once for manual entry into `agent.toml`, as today.

### 7. CI/CD

*   **Container CI** (existing pipeline): builds the Rust workspace, runs unit and integration tests, produces the `mosaicfs-agent` container image. This is the primary CI path and is unaffected by macOS-native work. Active from Phase 1.
*   **macOS CI** (separate pipeline): validates that the Swift code compiles and the `.app` bundle can be assembled. Does not run logic tests. Added in Phase 5 when the Menu Bar host is introduced.
