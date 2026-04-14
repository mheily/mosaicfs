# Architecture Change 005: Loco + HTMX Settings UI

This change replaces the React/TypeScript single-page application under
`web/` with a server-side rendered settings and administration UI built on
the Loco framework with Tera templates and HTMX for interactivity. File
browsing is explicitly out of scope — users browse files through the FUSE
mount, not the web UI.

Depends on change 004 (code consolidation must land first so the new web
layer talks to a single CouchDB client and a single readdir/notification
publisher rather than re-importing per-binary copies).

## Current State Summary

_Verified against the tree at the head of `master` (post change 003)._

**Frontend:** `web/` is a Vite + React 18 + TypeScript SPA.
`web/src` contains `App.tsx`, `pages/`, `components/`, `contexts/`,
`hooks/`, `lib/`, `types/`. It consumes the ~92 REST routes the server
exposes under `/api/*`. Shared types between the Rust server and the
TypeScript frontend are generated via `ts-rs` from
`mosaicfs-common::documents` and friends.

**Tauri shell:** `web/src-tauri/` wraps the SPA into a desktop app; it has
its own `Cargo.toml`, `tauri.conf.json`, `capabilities/`, `icons/`. The
shell is currently the only consumer of a few platform-specific bridges
(open-in-finder, mount-management UI affordances).

**Tests:** `web/tests` and `web/e2e` (Playwright) cover the React app.
`web/vitest.config.ts` and `web/playwright.config.ts` configure these.

**Server:** `mosaicfs-server` exposes REST routes from
`src/handlers/*.rs` and serves the built React assets from a static-file
route. Auth flows (login, OAuth callbacks for replication targets) bounce
through the SPA.

**Build pipeline:** `Makefile` invokes `npm` to build the SPA and bundles
the output into `mosaicfs-image`. The container image carries both the
Rust binaries and the static React bundle.

## Goal

Replace the React SPA with server-side rendered HTML so that the entire
settings/admin surface ships in the Rust binary, with no Node.js build
step, no TypeScript type generation, and no client-side routing. The
result is a single Rust binary that serves both the REST API (for the
agent and for any external consumers) and the admin HTML pages, with HTMX
handling the small amount of in-page interactivity the UI needs.

The scope is **settings and administration only**:

- Node configuration (watch paths, exclusions, identity).
- Credential management (access keys, OAuth tokens for backends).
- Replication setup (rules, targets, schedules, status).
- Storage backend configuration (S3/B2/directory targets).
- System status (heartbeats, filesystem availability map from change 003,
  notifications).
- Backup/restore (the in-progress Phase 8 surface).

File browsing remains the FUSE mount's job. The web UI does not enumerate
the namespace.

## Changes

### Change A — Introduce Loco as the web framework

**Today:** `mosaicfs-server` uses `axum` directly. Routes are mounted in
`src/routes.rs`; handlers live in `src/handlers/*.rs`. There is no view
layer; HTML responses are static React assets served from a single route.

**Proposed:** Add Loco as the framework for the new HTML routes. The
existing `axum`-based REST handlers continue to work — Loco builds on
`axum`, so the migration is additive: Loco mounts its router alongside the
existing one. New routes under `/admin/*` (or similar — settle the prefix
in phase 1) render Tera templates. Existing `/api/*` routes are
unchanged.

**Justification:** Loco gives a coherent SSR framework — controllers,
templates, model layer, background-job hooks — that fits the
"server-side admin UI" shape better than building it ad-hoc on raw `axum`.
Adopting it as a framework (rather than just adding `tera` and rolling our
own) means less bespoke glue.

### Change B — Tera templates + HTMX, no JavaScript build pipeline

**Today:** All UI logic runs in the browser. State is managed in React
contexts. Forms post via `fetch` to `/api/*` and re-render on response.

**Proposed:** Each admin page is a Tera template. Forms post to admin
routes that render either a full page or an HTMX partial (for in-page
swaps). HTMX is included as a single `<script src="…">` from a vendored
asset — no bundler, no `package.json`, no `npm install` in the build.

Anywhere the React UI used a client-side modal, list filter, or live
update, the equivalent Loco view either:

- renders the new state on form submit and swaps the affected fragment
  via HTMX, or
- polls a small JSON endpoint via HTMX `hx-trigger="every Ns"` for
  status that genuinely needs to refresh (heartbeat freshness,
  replication job progress).

No SPA-style client-side routing. Browser back/forward works because each
URL renders a real page.

**Justification:** The settings UI is form-heavy and read-mostly. SSR +
HTMX handles forms-and-fragments natively without the cost of a JS build
pipeline, type generation, or a parallel state model. Removing `npm` from
the build also shrinks the container image and removes a class of CVE
(transitive npm dependencies).

### Change C — Remove the React app and the Tauri shell

**Today:** `web/` and `web/src-tauri/` are the entire frontend. The
`Makefile` invokes `npm`. `ts-rs` generates TypeScript from Rust types.

**Proposed:** Once the Loco UI covers the equivalent settings surface,
delete `web/` and `web/src-tauri/` entirely. Remove `npm`/`node` from
the build. Remove `ts-rs` derives from `mosaicfs-common` (the JSON shape
on the wire stays the same; only the type-export side disappears). The
desktop experience (previously Tauri) becomes "open the admin URL in a
browser." If a desktop wrapper is wanted later, it can come back as a
thin native window pointing at the local URL — but it is not part of
this change.

**Justification:** Two parallel UI stacks is the worst of both worlds.
Once Loco is sufficient, the React app is dead weight: extra build
complexity, extra test surface (Vitest + Playwright), extra distribution
artifact (Tauri shell). Removing it is the point of the migration.

## Implementation Phases

Phases land in deployable increments. The old React UI keeps working until
Phase 4; users see the Loco UI under a parallel URL prefix during Phases 2
and 3 and can compare.

**Phase 1 — Wire up Loco.**
Add Loco to `mosaicfs-server`. Mount its router alongside the existing
`axum` routes. Add a single placeholder admin page rendering a Tera
template. Vendor HTMX as a static asset. Confirm the existing REST API
and the existing React app are unaffected. The container image now
contains both UIs — that is fine.

**Phase 2 — Port read-only admin pages.**
Settings pages that only display state (system status, filesystem
availability map, heartbeats, notifications, current replication
configuration). Each page is Tera + HTMX-poll for live values. The React
versions remain accessible.

**Phase 3 — Port write/admin flows.**
Forms for node config, credentials, replication rules, storage backends,
backup/restore. Each form posts to a Loco controller which writes to
CouchDB (via the shared client from change 004) and re-renders the
relevant fragment. OAuth callbacks for backend authentication move from
the React app's callback page to a Loco route.

**Phase 4 — Remove the React app and Tauri shell.**
Delete `web/` and `web/src-tauri/`. Drop `npm`/`node` from the
`Makefile` and `Dockerfile`. Drop the static-asset route that served the
React bundle. Remove `ts-rs` derives from `mosaicfs-common` and the
generated TypeScript outputs. Update `DEPLOYMENT.md` and `DEVELOPMENT.md`
to point at the new admin URL.

**Phase dependencies:**

- Phase 2 requires Phase 1.
- Phase 3 requires Phase 2 (or at least the framework setup from 1 — they
  could overlap).
- Phase 4 requires Phase 3 to be complete enough that no user flow still
  needs the React app.

## What Does Not Change

- **REST API surface.** The ~92 routes under `/api/*` keep their shapes
  and remain available for the agent and any external consumers. The
  Loco UI does its own thing under `/admin/*` (or chosen prefix).
- **Document model and CouchDB schema.** No new doc types, no field
  changes.
- **VFS / FUSE.** File browsing is via the mount, not the UI. No FUSE
  changes.
- **Replication targets and behavior.** S3/B2/directory replication
  continues to work the same way; only the UI for configuring it
  changes.
- **Authentication for the API.** API auth (access key + HMAC) is
  unchanged. The admin UI gets its own session-based auth via Loco —
  settle the integration with the existing credential store in phase 1.
- **Deployment.** Still one container image, still one pod. The image
  shrinks (no npm bundle); the container layout does not change.
- **Agent.** `mosaicfs-agent` is untouched. (Merging into one binary is
  change 006.)
- **Per-filesystem availability map (change 003), code consolidation
  (change 004).** Already settled; this change consumes the consolidated
  modules and renders the availability map but does not modify either.

## Deferred

- **File browsing in the web UI.** Out of scope. The FUSE mount is the
  file-browsing surface. If a web file browser ever comes back, it is a
  separate change.
- **Desktop wrapper.** Removing Tauri leaves "open the admin URL in a
  browser" as the desktop experience. A native wrapper can return later
  as a thin shell around the local URL; not part of this change.
- **Mobile/responsive polish.** The admin UI targets desktop browsers
  first. Mobile layout work is deferred.
- **Real-time push (WebSockets/SSE).** HTMX polling covers the live
  values that matter (heartbeats, job progress). Push is deferred unless
  a use case appears that polling cannot handle.
- **Dark mode and theming.** Not in scope. Ship one default theme.
- **i18n / localization.** English only for v1.
- **macFUSE FileProvider replacement and redb.** Both deferred to v2 per
  project decisions; this change does not touch the VFS backend.
- **Admin UI for unified-binary feature toggles.** The TOML `[features]`
  surface from change 006 is server-side only at first; an admin page
  for it can come later.
