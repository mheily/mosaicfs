# MosaicFS Desktop App (Tauri)

The MosaicFS desktop app wraps the existing React frontend in a [Tauri v2](https://v2.tauri.app/) shell, providing a native Finder-like file browsing experience. It connects to a **remote** MosaicFS server — no embedded server is included.

## Prerequisites

- **Rust toolchain** — install via [rustup](https://rustup.rs/)
- **Node.js ≥ 18** and **npm**
- **Tauri v2 system dependencies** — see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Linux**: `build-essential`, `libwebkit2gtk-4.1-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev` (package names vary by distro)
  - **Windows**: Microsoft Visual Studio C++ Build Tools, WebView2 (pre-installed on Windows 10+)

## Setup

From the repository root:

```sh
cd web

# Install standard web dependencies
npm install

# Install Tauri-specific packages (not in the lockfile — only needed for desktop builds)
npm install @tauri-apps/api @tauri-apps/plugin-store @tauri-apps/plugin-shell @tauri-apps/plugin-fs
npm install -D @tauri-apps/cli
```

> **Note:** The Tauri npm packages are intentionally excluded from `package.json` and the lockfile so that `npm ci` in the server Docker image build is unaffected. They are installed locally for desktop development only.

## Development

Start the Tauri dev server with hot-reload:

```sh
# From repository root
make tauri-dev

# Or from web/
cd web && npm run tauri:dev
```

This will:
1. Start the Vite dev server on `http://localhost:5173`
2. Compile the Rust Tauri shell (first build takes a few minutes for dependency compilation)
3. Open a native window loading the React frontend

The Vite dev server is pinned to port 5173 (`strictPort: true`) since the Tauri config expects it at that address.

### First Launch

On first launch, the app shows a **Server Connect** page. Enter the URL of your running MosaicFS server (e.g. `https://localhost:8443`) and click Connect. The app validates connectivity by hitting the `/api/system/bootstrap-status` endpoint. On success, the URL is persisted locally and the app redirects to the login page.

After login, navigating to `/files` loads the Finder-like compact file browser layout.

### Hot Reload

- **Frontend changes** — Vite hot-reloads the React app instantly. The Tauri window reflects changes without restart.
- **Rust changes** — Tauri CLI watches `src-tauri/src/` and recompiles automatically. The window restarts when the Rust binary changes.

## Production Build

Build a distributable application bundle:

```sh
# From repository root
make tauri-build

# Or from web/
cd web && npm run tauri:build
```

Output location depends on platform:
- **macOS**: `web/src-tauri/target/release/bundle/dmg/MosaicFS_0.1.0_aarch64.dmg` (or `x64`)
- **Linux**: `web/src-tauri/target/release/bundle/deb/`, `appimage/`, or `rpm/`
- **Windows**: `web/src-tauri/target/release/bundle/msi/` or `nsis/`

## Architecture

```
web/src-tauri/           # Tauri Rust crate (NOT part of the Cargo workspace)
├── Cargo.toml           # tauri 2, tauri-plugin-store, tauri-plugin-shell, tauri-plugin-fs
├── build.rs             # tauri_build::build()
├── tauri.conf.json      # Window config, build commands, app metadata
├── capabilities/
│   └── default.json     # Permissions: core, store, shell, fs
└── src/
    ├── main.rs          # Entry point
    ├── lib.rs           # Tauri builder with plugins
    └── menu.rs          # Native menu bar (App, File, Edit, Go, Window)
```

### How It Connects to the Server

The desktop app does **not** embed a MosaicFS server. Instead:

1. On first launch, the user provides a server URL via the **Server Connect** page
2. The URL is persisted using `@tauri-apps/plugin-store` (saved to `settings.json` in the app's data directory)
3. On subsequent launches, the stored URL is loaded and applied via `setBaseUrl()` in `web/src/lib/api.ts`
4. All API calls (`api()` function) prepend this base URL, turning relative paths like `/api/vfs` into absolute URLs like `https://your-server:8443/api/vfs`
5. PouchDB replication also uses the base URL to connect to the remote CouchDB

### CORS

The MosaicFS server includes CORS headers for Tauri origins:
- `tauri://localhost` (macOS and Linux)
- `https://tauri.localhost` (Windows)
- `http://localhost:*` (development)

### Platform Detection

`web/src/lib/platform.ts` exports `isTauri()` which checks for `'__TAURI__' in window`. This is used throughout the frontend to conditionally enable desktop-specific behavior while keeping the standard web UI completely unaffected.

### What Changes in Tauri Mode

| Feature | Web | Tauri Desktop |
|---------|-----|---------------|
| Layout for `/files` | Standard sidebar + top bar (`Layout`) | Compact Finder layout (`FinderLayout`) |
| File list density | Normal (`text-sm`, `py-2`) | Compact (`text-xs`, `py-1`) |
| Node column | Visible | Hidden |
| Click behavior | Single click opens/navigates | Single click selects, double click opens/navigates |
| Enter key on file | Opens detail drawer | Downloads to temp dir and opens in native app |
| Directory tree sidebar | `w-64` with heading | `w-56` with subtle background, no heading |
| Row styling | Default | Alternating background (`even:bg-muted/30`), blue selection highlight |
| Breadcrumb | In file browser page | In FinderLayout toolbar |
| Titlebar | Browser chrome | macOS overlay with traffic lights and drag region |

### Keyboard Navigation

Active in both web and Tauri modes (selection highlight is more prominent in Tauri):

| Key | Action |
|-----|--------|
| `↓` / `↑` | Select next / previous item |
| `Enter` | Open selected — directory: navigate in, file: native open (Tauri) or drawer (web) |
| `Space` | Open detail drawer |
| `Escape` | Close drawer or deselect |
| `→` | Navigate into selected directory |
| `←` | Navigate to parent directory |
| `Cmd+Backspace` | Navigate to parent directory |

### Native Menu Bar

The Go menu provides navigation shortcuts:
- **Back** (`Cmd+[`) — browser-style back navigation
- **Forward** (`Cmd+]`) — browser-style forward navigation
- **Enclosing Folder** (`Cmd+Up`) — navigate to parent directory

Menu events are emitted to the frontend via `window.emit("menu-action", id)` and handled by `web/src/hooks/useMenuEvents.ts`.

## Relationship to the Server Build

The Tauri desktop app and the server container image are **independent build targets**:

- `make mosaicfs-image` — builds the server Docker image with the web UI baked in. The Tauri packages are not required and are excluded from this build.
- `make tauri-build` — builds the desktop application. Requires the Tauri npm packages installed locally.

Both use the same React frontend source code. The `isTauri()` check ensures all desktop-specific behavior is isolated and the web UI works identically whether served from the server or opened in a browser.

## Deferred Features (Not in v1)

- Drag-and-drop to/from local filesystem
- System tray icon
- Auto-update
- Multiple windows
- Grid / icon view (list view only)
- File upload from desktop
- Offline mode
- Custom protocol handler (`mosaicfs://`)
