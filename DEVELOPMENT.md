# Development notes

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