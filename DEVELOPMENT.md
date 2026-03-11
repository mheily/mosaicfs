# Development notes

# Building the image

Run `make` to build the mosaicfs-dev:latest image. This is used by docker-compose.

Run `podman-compose up -d` to start the containers.

Run `podman-compose exec web /workspace/target/debug/mosaicfs-server bootstrap` to generate
an initial admin user and display the credentials.

# Automated code generation

Inside a devcontainer terminal, run:

```
env IS_SANDBOX=1 claude --dangerously-skip-permissions
```

# Running podman as a different user

podman depends on dbus, so you have to switch to mosaicfs UID/GID like this:
```
sudo machinectl shell mosaicfs@
```

# Building

Build all Rust crates in the workspace:

```
cargo build
```

Build in release mode:

```
cargo build --release
```

Build a specific crate:

```
cargo build -p mosaicfs-common
cargo build -p mosaicfs-agent
cargo build -p mosaicfs-server
cargo build -p mosaicfs-vfs
```

Run all tests:

```
cargo test
```

Check compilation without producing binaries:

```
cargo check
```

# Installing Node.js without root

The web frontend requires Node.js 22+ and npm. If your system doesn't have them (or has an older version), you can install a standalone copy into your home directory using the official prebuilt binaries. No root permissions are needed.

```sh
# Download and extract Node.js 22 to ~/.local
mkdir -p ~/.local
curl -fsSL https://nodejs.org/dist/v22.16.0/node-v22.16.0-linux-x64.tar.xz \
  | tar -xJ --strip-components=1 -C ~/.local

# Add to PATH (add this line to your ~/.bashrc or ~/.profile to persist)
export PATH="$HOME/.local/bin:$PATH"

# Verify
node --version   # v22.16.0
npm --version    # 10.x
```

For Apple Silicon Macs, replace `linux-x64` with `darwin-arm64`. For Intel Macs, use `darwin-x64`. Browse https://nodejs.org/dist/ for other platforms and versions.

# Web UI

The React frontend lives in `web/`. It uses Vite, TypeScript, Tailwind CSS v4, and shadcn/ui.

## Install dependencies

```
cd web && npm ci
```

## Dev server

Starts a Vite dev server with API proxy to the Rust backend:

```
cd web && npm run dev
```

## Production build

Outputs to `web/dist/`, served by the Rust server as a fallback:

```
cd web && npx vite build
```

## TypeScript check

```
cd web && npx tsc --noEmit
```

## Unit / component tests (Vitest)

Runs 32 tests across 6 files using Vitest + React Testing Library + jsdom:

```
cd web && npx vitest run
```

Test files are in `web/tests/`:

- `auth.test.tsx` — AuthProvider login/logout/token flow
- `pouchdb-hooks.test.ts` — useLiveQuery and useLiveDoc hooks
- `step-editor.test.tsx` — StepEditor component (all 10 op types)
- `label-chip.test.tsx` — LabelChip direct vs inherited rendering
- `file-detail.test.tsx` — FileDetailDrawer metadata/preview/download
- `search.test.tsx` — SearchPage debounce and label filtering

## Tauri desktop app

The Tauri desktop shell lives in `web/src-tauri/`. It wraps the same React frontend in a native window. See `docs/tauri-desktop-app.md` for full details.

### System dependencies (requires root)

Tauri on Linux needs WebKitGTK and related system libraries. On Debian/Ubuntu:

```
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
```

On Fedora:

```
sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel libsoup3-devel
```

On macOS, only Xcode Command Line Tools are needed (`xcode-select --install`).

### Install Tauri npm packages

The Tauri npm packages are **not** listed in `web/package.json` because they are not in the lockfile and would break `npm ci` in the Docker image build. Install them locally before working on the desktop app:

```
cd web
npm install @tauri-apps/api @tauri-apps/plugin-store @tauri-apps/plugin-shell @tauri-apps/plugin-fs
npm install -D @tauri-apps/cli
```

### Run

Start the Tauri dev server (first build compiles ~470 Rust crates and takes several minutes):

```
make tauri-dev
```

Build a release bundle:

```
make tauri-build
```

## E2E tests (Playwright)

Runs against a real Axum server + CouchDB. Requires the backend and database to be running.

Install browsers (first time only):

```
cd web && npx playwright install --with-deps chromium
```

Run tests:

```
cd web && npx playwright test
```

Test files are in `web/e2e/`:

- `login.spec.ts` — Login/logout flow
- `file-browser.spec.ts` — Directory tree navigation, file detail drawer
- `search.spec.ts` — Search input, results, drawer
- `labels.spec.ts` — Assignments and Rules tabs
- `vfs-editor.spec.ts` — VFS directory tree, mount editor
- `credentials.spec.ts` — Credential CRUD in Settings
- `live-sync.spec.ts` — PouchDB live sync of node badges
- `responsive.spec.ts` — Mobile viewport with bottom tab navigation
