Design Notes 001: Technical Implementation Details

### 1. Unified Control UI: Loco + HTMX
To replace the React-based Tauri UI, we will move to a server-side MVC architecture using [Loco](https://loco.rs/) and [HTMX](https://htmx.org/).

*   **Architecture:** MVC (Model, View, Controller) in Rust.
*   **Templates:** Use Tera (similar to Django/Jinja2) for server-side HTML rendering.
*   **Interactivity:** HTMX will handle dynamic updates (e.g., sync progress bars, status badges) without custom JavaScript.
*   **Integration:** Loco will be integrated directly into the `mosaicfs-agent` process, allowing it to share state with the Crawler and Sled cache.

### 2. Sled Key-Value Cache
The [Sled](https://github.com/spacejam/sled) lock-free KV store provides the sub-millisecond responsiveness required for Finder and the Control UI.

*   **Data Layout:**
    *   `inodes/`: Namespace for file and directory metadata.
        *   Key: `inodes/{id}`
        *   Value: Serialized `Inode` struct (using `bincode`).
    *   `status/`: Namespace for sync state.
        *   Key: `status/{file_id}`
        *   Value: Serialized `SyncStatus` struct.
*   **Shared Access:** The Loco web server (Control UI) and the Swift FileProvider extension (via FFI) will both query this cache concurrently. Sled's MVCC ensures no "database is locked" errors.

### 3. FFI Bridge (Rust -> Swift)
We will use [UniFFI](https://github.com/mozilla/uniffi-rs) to bridge the "Mosaic Engine" to macOS-native components.

*   **FileProvider Bridge:** The Swift extension will call `get_file_metadata(id)` and `fetch_file_content(id)` via the UniFFI bridge.
*   **Menu Bar App Bridge:** The macOS host will use the bridge to control the lifecycle of the Rust agent and to retrieve local configuration paths.
*   **Lib Generation:** The Rust library will be compiled as a static library (`.a`) with generated Swift headers for Xcode consumption.

### 4. macOS FileProvider Architecture
*   **Native Finder Locations:** MosaicFS will appear in the Finder sidebar as a first-class Location.
*   **On-Demand Fetching:** Files in Sled marked with `remote_only` will show with a cloud icon in Finder. When double-clicked, the FileProvider will call the Rust Engine to fetch the file bits from a remote node.
*   **Signal Enumerator:** The Rust Engine will emit change signals to the FileProvider extension (via FFI) when Sled updates, triggering Finder to refresh its view.

### 5. Thin macOS Menu Bar Host
*   **Role:** Background manager for the `mosaicfs-agent` (Loco server).
*   **Configuration Access:** Provides a "Settings" menu item that opens a native `WKWebView` pointing to the locally-running Loco server (e.g., `http://localhost:8443`). This ensures the user experience remains contained within a "Native-feeling" window.

### 6. Finder Sync Extension (macOS UI)
To provide a first-class macOS experience, we will implement a [Finder Sync](https://developer.apple.com/documentation/findersync) extension.

*   **Contextual Menus:** Adds a "MosaicFS Settings..." option to the Finder right-click menu for any file or directory within the MosaicFS location.
*   **Status Badging:** Displays sync status icons (e.g., green checkmarks for "Synced", blue circles for "Syncing") directly on files in Finder.
*   **Action Flow:** Clicking "MosaicFS Settings..." from the Finder menu will signal the Menu Bar Host to open the Loco-based web UI, pre-navigated to the selected item's configuration page.
*   **Integration:** Communicates with the Rust engine via the same UniFFI bridge used by the FileProvider extension.

### Future Questions
1. **Secret Management:** Should we migrate sensitive credentials (API keys, CouchDB tokens) from `agent.toml` to the native **macOS Keychain** using the `keyring` Rust crate?
2. **Wake-up Strategy:** How will the Rust Engine signal the Swift FileProvider to refresh Finder (e.g., via UniFFI callbacks to `NSFileProviderManager.signalEnumerator`) after a background sync completes?
3. **Cross-Platform VFS:** Will Windows and Linux continue to use the existing FUSE (`fuser`) implementation, and how will we maintain that alongside the macOS FileProvider in the unified "Mosaic Engine" crate?
4. **Initial Sync UX:** How will the Loco "First Run" experience manage user expectations and progress indicators during the initial metadata crawl before the FileProvider is fully active?

  1. How will we handle "Secret Management" natively?
  Currently, your project uses agent.toml for configuration. On macOS, users expect first-class security.
   * The Question: Should we move API keys and CouchDB credentials from a plain-text .toml file into the macOS Keychain?
   * Why it matters: It’s more secure and is the "native way" to handle secrets. Our Rust engine will need a platform-specific abstraction (e.g., using the
     keyring crate) to handle this.

  2. How does the Rust Engine "Wake Up" the Swift FileProvider?
  FileProvider extensions are often suspended by the system to save battery.
   * The Question: When the Rust crawler finishes a background download or sync, how does it tell macOS to "refresh" the cloud icon in Finder?
   * Why it matters: We need to ensure our UniFFI bridge supports Callbacks. The Rust engine must be able to call a Swift function that triggers
     NSFileProviderManager.signalEnumerator() to keep the UI in sync.

  3. What is the VFS strategy for Windows and Linux?
  We’ve solved the macOS experience with FileProvider.
   * The Question: Will Windows and Linux users still use FUSE, or should we plan for their native "Cloud Sync" equivalents later?
   * Why it matters: If we keep FUSE for Linux/Windows, our "Mosaic Engine" must maintain a dual-backend (FileProvider for Mac, FUSE for others). This is fine,
     but we should explicitly decide if we are keeping the fuser code in the "Engine" crate.

  4. How do we handle "Initial Sync" performance?
  Sled is incredibly fast, but moving from a "cold" install to a "populated" Finder view requires an initial sync from CouchDB.
   * The Question: How do we prevent the macOS "Host App" from looking broken or empty while the initial crawl and Sled population are happening?
   * Why it matters: We likely need a "First Run Experience" in the Loco web UI that shows a clear progress bar for the initial metadata download before the
     FileProvider is "activated" in Finder.
