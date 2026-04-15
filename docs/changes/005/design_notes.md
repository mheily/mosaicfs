# Change 005 — Design Notes

Companion to `architecture.md`. Captures the concrete decisions agreed before
implementation so reviewers can check alignment before code lands.

## Scope for this implementation round

- Implement **Phases 1 and 2** from `architecture.md`.
- **Phases 3 and 4 are deferred** to a follow-up session (next day). The
  React app and Tauri shell keep working throughout this change; users can
  compare the two UIs side-by-side.
- One git commit per phase (two commits this session).

## Decisions

### Loco integration

- Adopt `loco-rs` **minimally**: use it for controllers, Tera view rendering,
  and router glue only. Skip SeaORM/migration/worker subsystems — MosaicFS is
  CouchDB-backed, and the existing axum `AppState` (with the shared CouchDB
  client from change 004) stays the single source of truth.
- Loco's router is mounted **alongside** the existing axum router in
  `mosaicfs-server::routes::build_router`. No REST route is moved, renamed,
  or altered.
- If `loco-rs` cannot be embedded as a library without dragging in the
  ORM/worker stack, fall back to plain `axum` + `tera` + `tower-sessions` and
  note the deviation here before proceeding. Rationale: the architecture doc
  names Loco as a means, not an end; the user-visible contract is "SSR admin
  UI with HTMX."

### URL layout

- All new admin routes live under `/admin/*`.
- `/admin` (no suffix) redirects to `/admin/status` (the Phase 2 landing
  page).
- HTMX partials served from the same controller that renders the full page,
  selected by the `HX-Request` header — one route, two render paths.
- Static assets (Pico CSS, HTMX, any admin CSS) served from
  `/admin/assets/*`, sourced from `mosaicfs-server/assets/` embedded via
  `include_bytes!` (no filesystem dependency at runtime, no separate
  container layer).

### Auth

- By default, `/admin/*` requires a logged-in session (cookie-based,
  server-side session store in `AppState`, validates against the existing
  credential store using `credentials::verify_secret`).
- **Bypass:** when the environment variable `MOSAICFS_INSECURE_HTTP=1` is
  set at server start, the admin auth middleware is disabled entirely and
  `/admin/*` is reachable with no login. Intended for local development
  only. Logged with a `WARN` on startup so it cannot be missed.
- Login page: `/admin/login` (Tera form, posts access key + secret, sets
  session cookie on success, redirects to `/admin/status`).
- Logout: `POST /admin/logout` clears the cookie and redirects to
  `/admin/login`.
- The existing `/api/auth/*` JWT flow and the `/api/agent/*` HMAC flow are
  untouched. Admin sessions are orthogonal to API tokens.

### UI stack

- **CSS:** Pico.css v2 (classless), vendored at
  `mosaicfs-server/assets/pico.min.css`. One default theme; no dark mode
  toggle.
- **HTMX:** v2.0.x, vendored at `mosaicfs-server/assets/htmx.min.js`.
- No other JS. No bundler. No `package.json`.
- Templates live in `mosaicfs-server/templates/` and are embedded at compile
  time (via `rust-embed` or Tera's `include_str!` pattern) so the binary has
  no runtime template-directory dependency.

### Feature parity philosophy

- The admin UI ships the **minimum** forms and read views needed to cover
  each settings category. It does **not** reproduce every React affordance
  (toasts, animated modals, filtered tables, drag-and-drop). A plain form +
  a success/error banner on re-render is the baseline.
- React pages that are explicitly **out of scope** (file browsing,
  per-directory VFS navigation, search UI, DB console, label management UI)
  are NOT ported. The underlying `/api/*` routes remain available for the
  agent / external consumers; only the admin HTML surface is scoped to
  settings and admin.

### Scope map — React page → Loco admin page

| React page              | Loco admin page                 | Phase |
| ----------------------- | ------------------------------- | ----- |
| BootstrapPage           | `/admin/bootstrap`              | 3     |
| LoginPage               | `/admin/login`                  | 1/3   |
| DashboardPage (status)  | `/admin/status`                 | 2     |
| NodesPage / NodeDetail  | `/admin/nodes`, `/admin/nodes/{id}` | 2/3 |
| StoragePage             | `/admin/storage-backends`       | 3     |
| SettingsPage            | `/admin/settings` (credentials, node config, backup/restore) | 3 |
| (replication status)    | `/admin/replication`            | 2/3   |
| (notifications)         | `/admin/notifications`          | 2     |
| FileBrowserPage         | **not ported** (FUSE mount)     | —     |
| SearchPage              | **not ported**                  | —     |
| LabelsPage              | **not ported**                  | —     |
| VfsPage                 | **not ported**                  | —     |
| DbConsolePage           | **not ported**                  | —     |
| ServerConnectPage       | **not ported** (Tauri only)     | —     |

### Phase breakdown

**Phase 1 — framework wiring (one commit).**

- Add `tera`, `tower-sessions`, `rust-embed` (or equivalent) as deps. Add
  `loco-rs` if it embeds cleanly; otherwise record the fallback and
  proceed.
- Create `mosaicfs-server/src/admin/mod.rs` with router, session
  middleware, and a `/admin/status` placeholder page.
- Vendor Pico + HTMX under `mosaicfs-server/assets/`.
- Add `/admin/login`, `/admin/logout` (session-based). Honor
  `MOSAICFS_INSECURE_HTTP=1`.
- Mount `/admin` router under the existing `build_router`. The React
  fallback (`ServeDir` on `web/dist`) stays.
- **Acceptance:** `cargo build` succeeds; `cargo run -p mosaicfs-server`
  serves the existing UI at `/` AND a new HTML page at `/admin/status`;
  login flow works; existing REST tests pass.

**Phase 2 — read-only admin pages (one commit).**

- `/admin/status` — system info, bootstrap status, filesystem availability
  map (from change 003), heartbeat freshness per node.
- `/admin/nodes` — list nodes + heartbeat.
- `/admin/notifications` — list recent notifications (no ack yet;
  ack is a write, so it lands in Phase 3).
- `/admin/replication` — current rules + replicas + status (read).
- HTMX `hx-trigger="every 5s"` on the status / replication panels for
  live refresh.
- **Acceptance:** pages render with real data on a running dev server;
  read paths for all listed categories work; React UI still works.

**Phase 3 — write/admin flows (one commit).**

- `/admin/bootstrap` — initial admin credential creation.
- `/admin/settings` — node config (watch paths, identity), credential CRUD,
  backup/restore trigger.
- `/admin/nodes/{id}` — edit node, manage mounts.
- `/admin/storage-backends` — CRUD for S3/B2/directory targets.
- `/admin/replication` — create/edit/delete rules, initiate restore,
  cancel restore.
- `/admin/notifications` — ack individual + ack-all.
- OAuth callback route (for replication backends that use OAuth) moves to
  `/admin/oauth/callback`. No external registered callback URLs to
  preserve (confirmed).
- Forms post to Loco controllers; controllers call into the existing
  `credentials::*`, `handlers::*` logic where practical (refactor to
  extract pure functions from axum handlers where a handler currently
  bakes in `Json(...)` + status codes).
- **Acceptance:** every category listed in architecture's "Goal" has a
  working write path from the admin UI; sessions hold across requests;
  React UI still works in parallel.

## What is explicitly NOT in this round

- Phase 4 cleanup: `web/` and `web/src-tauri/` remain. `npm`/`node` stays
  in the Makefile/Dockerfile. `ts-rs` derives stay. The React static-asset
  fallback stays.
- No changes to the REST API surface, document model, FUSE, VFS, replication
  behavior, or agent.
- No dark mode, mobile layout, i18n, WebSocket/SSE, or admin UI for feature
  toggles.

## Open risks to flag during review

1. **Loco-as-library viability — resolved: fallback taken.** During Phase 1
   implementation, `loco-rs` was confirmed to be an app framework (owns
   `main`, ties in SeaORM + migrations + workers + its own config system)
   rather than a library embeddable alongside existing axum infrastructure.
   Adopting it would have meant restructuring `mosaicfs-server` around
   Loco's skeleton — out of proportion to the goal. Fallback adopted: plain
   `axum` (already a dep) + `tera` + `tower-sessions`. User-visible result
   is identical to architecture.md's intent: SSR admin UI, HTMX partials,
   Pico styling, cookie sessions.
2. **Handler reuse.** Current axum handlers return `(StatusCode, Json(...))`
   tuples. The Loco controllers will need to call the same underlying
   CouchDB/business logic. Expect light refactors extracting pure
   functions from `handlers/*.rs` into small helpers; the REST handlers
   become thin wrappers. This keeps the JSON wire shape stable.
3. **Session store.** In-memory `HashMap<SessionId, AccessKeyId>` inside
   `AppState` is the default (simplest, survives restarts by forcing
   re-login, no CouchDB schema change). If durable sessions are wanted
   later, add a `session::*` document type in a follow-up.
