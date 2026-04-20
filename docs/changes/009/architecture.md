# Change 009 — Rename `/admin` → `/ui`

## 1. Current State Summary

_Condensed from the auto-generated inventory (commit `9be6abe`), supplemented
by direct grep._

### The server-rendered UI today

Lives entirely under `/admin/*`:

- Module: `mosaicfs-server/src/admin/` (`mod.rs`, `views.rs`, `actions.rs`)
- Public entry: `admin::router()` called from `routes.rs::build_router`
- Session cookie: `mosaicfs_admin` (12-hour idle, `SameSite::Lax`)
- Assets: `/admin/assets/pico.min.css`, `/admin/assets/htmx.min.js` served by `serve_asset` from `admin/mod.rs:347`
- 45 routes on the admin router (public + protected branches)
- Auth bypass: `MOSAICFS_INSECURE_HTTP=1` in `admin::insecure_http`
- Templates: 20 files in `mosaicfs-server/templates/`, registered via `include_str!` in `admin::tera()`
- Root redirect: `routes.rs` redirects `/` → `/admin`; `admin::router` redirects `/admin` → `/admin/browse`

### Where `/admin` appears

- Rust code: `mosaicfs-server/src/admin/mod.rs` (54 occurrences), `admin/actions.rs` (46), `routes.rs` (1)
- Templates: 12 files with a combined 54 hardcoded `/admin/...` URLs (nav links, form actions, row `onclick` handlers, redirect hrefs)
- Dev scripts: `scripts/start-dev-environment`, `scripts/test-dev-environment`, `scripts/watch-dev-server`
- Documentation: `CLAUDE.md`, `DEPLOYMENT.md`, `DEVELOPMENT.md`
- Skill definition: `.claude/skills/admin-crud/SKILL.md` (34 occurrences — this skill documents the CRUD pattern the module uses)

Total: 207 `/admin` occurrences across 28 files outside `docs/changes/`.

### Why the current name is wrong

The `admin` module is the home of all server-rendered HTML, not just administrative pages. Change 010 will add a user-facing file browser to the same module. Leaving the name `admin` forces every new page into a label that misdescribes it, and every new URL starts with a prefix that will need to be renamed later — at higher cost.

---

## 2. Goal

Rename the server-rendered UI surface from `/admin` to `/ui` so it can host both administrative and end-user pages without a name mismatch. This is a mechanical rename — no behavior changes, no page changes, no auth changes.

---

## 3. Changes

### Change 3.1 — Module rename

**Today.** `mosaicfs-server/src/admin/` (`mod.rs`, `views.rs`, `actions.rs`). Public entry point `admin::router()`.

**Proposed.** Rename the directory to `mosaicfs-server/src/ui/`. Update `lib.rs` to `pub mod ui;` (or equivalent — need to check the current visibility; the module is declared with `pub mod admin` in `lib.rs`). Update the call site in `routes.rs:158` from `crate::admin::router()` to `crate::ui::router()`. Internal paths like `admin::actions::` and `admin::page_ctx` → `ui::actions::` and `ui::page_ctx`.

**Justification.** Directory name becomes the importable identifier throughout the crate. Renaming the directory without renaming the module identifier would leave a permanent mismatch between file path and import path.

### Change 3.2 — URL prefix rename

**Today.** 45 routes under `/admin/*` plus an asset subtree at `/admin/assets/*`. Root redirect at `/` → `/admin`. Intra-admin redirect at `/admin` → `/admin/browse`.

**Proposed.** Every `/admin` prefix becomes `/ui`:

- `GET /admin/login` → `GET /ui/login`
- `GET /admin/bootstrap` → `GET /ui/bootstrap`
- `GET /admin/browse` → `GET /ui/browse` (page content unchanged; change 010 will then repurpose this URL — flagged in §6)
- `GET /admin/assets/{*path}` → `GET /ui/assets/{*path}`
- `/admin/status`, `/admin/nodes`, `/admin/vfs`, `/admin/replication`, `/admin/storage-backends`, `/admin/notifications`, `/admin/settings/*` → `/ui/...` at the matching paths
- `/admin/logout` POST → `/ui/logout` POST
- Root redirect in `routes.rs`: `/` → `/ui` (was `/admin`)
- Self-redirect: `/ui` → `/ui/browse` (was `/admin` → `/admin/browse`)

All template internal URLs (`href`, `action`, `onclick='location.href=...'`) are mechanically search-and-replaced from `/admin/` to `/ui/`. The `<link rel="stylesheet">` and `<script src>` in `layout.html` point at the new asset paths.

**Justification.** URL prefix is user-visible and is the artifact the developer will type into launcher files, bookmarks, and reverse-proxy configs going forward. Consistency with the module name matters.

### Change 3.3 — Session cookie rename

**Today.** The admin session cookie is named `mosaicfs_admin` (set in `admin/mod.rs:146` via `.with_name("mosaicfs_admin")`).

**Proposed.** Rename to `mosaicfs_session`. This is the only session cookie the project uses; the name `mosaicfs_session` leaves room for no assumption that it is admin-scoped.

**Justification.** Keeps the cookie name consistent with the new, broader UI surface. No concept of multiple cookies needs to be introduced. Any logged-in user gets signed out on first deploy of this change; that is acceptable because nothing has shipped.

### Change 3.4 — Rename the `admin-crud` skill to `ui-crud`

**Today.** `.claude/skills/admin-crud/SKILL.md` (234 lines) documents the CRUD UX pattern with 34 `/admin/*` references in example URLs, route tables, and Rust snippets. The skill's `name:` frontmatter is `admin-crud`, and its description mentions "the MosaicFS admin UI".

**Proposed.** Move the directory to `.claude/skills/ui-crud/`. Update the frontmatter `name:` to `ui-crud`, update the description ("CRUD UX pattern for the MosaicFS server-rendered UI"), and replace every `/admin/` with `/ui/` in the body. Route table, URL conventions, example Rust code, checklist — all get the prefix swap.

**Justification.** The skill describes an internal pattern that downstream work (change 010 and onward) will follow. If the skill still says `/admin/` after the code says `/ui/`, future invocations will produce inconsistent code.

### Change 3.5 — Developer documentation sweep

**Today.** `CLAUDE.md`, `DEPLOYMENT.md`, `DEVELOPMENT.md`, and three scripts under `scripts/` reference `/admin` URLs (typically as the landing URL for `curl` tests or as the URL to open in a browser after starting the server).

**Proposed.** Replace every occurrence with `/ui`. Update any accompanying prose that says "admin UI" → "UI" or "web UI" where the text is about the surface at large, versus administrative functions specifically. Where the text really does mean "administrative", leave the word "admin" — this is a URL rename, not a concept rename.

**Justification.** Documentation and scripts are the first things a developer (human or AI) reads. Leaving them at `/admin` would create lasting confusion about which URL is correct.

---

## 4. Implementation Phases

Phases are topical; intermediate states need not compile. A single PR is acceptable.

### Phase 1 — Code rename

Deliverables:

- `git mv mosaicfs-server/src/admin mosaicfs-server/src/ui`
- `lib.rs`: `pub mod admin` → `pub mod ui`
- `routes.rs:158`: `crate::admin::router()` → `crate::ui::router()`
- Inside `mosaicfs-server/src/ui/*.rs`: rewrite every `"/admin"` string literal to `"/ui"`. This covers route definitions, redirect targets, form action URLs in flash messages, the `require_auth` path-prefix check, the `serve_asset` URL stripping, etc.
- Session cookie name literal: `"mosaicfs_admin"` → `"mosaicfs_session"`
- Asset path literal in `serve_asset`: `"/admin/assets/"` → `"/ui/assets/"`
- Root redirect in `routes.rs`: `Redirect::to("/admin")` → `Redirect::to("/ui")`
- Unit tests inside `ui/` and any integration tests under `mosaicfs-server/tests/` that hit `/admin/*` — update URLs.
- `cargo build` and `cargo test` pass.

### Phase 2 — Template + asset sweep

Deliverables:

- In `mosaicfs-server/templates/*.html`, replace every `/admin/` with `/ui/`. This is a literal string substitution across 12 files.
- `layout.html` stylesheet and script tags switch to `/ui/assets/...`.
- Manual verification: start the server (`make run-dev-server`), load `http://localhost:8443/ui`, confirm the full nav works, log in, click through every page.

### Phase 3 — Skill + docs + scripts sweep

Deliverables:

- `.claude/skills/admin-crud/` → `.claude/skills/ui-crud/` (directory rename).
- Inside the skill, rename frontmatter, description, all `/admin/` → `/ui/`, and any prose that referred to "admin UI" as the surface-at-large.
- `CLAUDE.md`, `DEPLOYMENT.md`, `DEVELOPMENT.md`: `/admin` → `/ui` in URLs, prose around it reviewed for sense.
- `scripts/start-dev-environment`, `scripts/test-dev-environment`, `scripts/watch-dev-server`: URL swap.
- Auto-memory index at `/Users/robot/.claude/projects/-Users-robot-mosaicfs/memory/project_change008.md` (references `/admin/vfs`) — update after Phase 1–3 land, or leave as a historical record; tag for follow-up rather than blocking this change.

---

## 5. What Does Not Change

- **Behavior.** Every page renders identically; every form submits to the same handler with the same semantics.
- **Authentication.** Same session-based auth, same `require_auth` middleware, same `MOSAICFS_INSECURE_HTTP` bypass, same JWT auth on `/api/*` (untouched).
- **The REST API at `/api/*`.** Not touched. The admin/server-rendered UI and the REST API are independent surfaces.
- **CouchDB schema, document ids, and session store (`MemoryStore`).**
- **Templates' internal content** — only URLs inside them change.
- **Nav structure, nav links' labels, page layouts, CSS.**
- **Pico and HTMX asset bytes** — served from a renamed path but unchanged.
- **Agent, VFS, common crates.**
- **Deploy manifest, Dockerfile, CI.** No build inputs change.

---

## 6. Deferred

- **`/ui/browse` → end-user file browser (change 010).** After this rename, `/ui/browse` is the old admin-oriented browse page (file list with source/mime/labels and edit-delete affordances). Change 010 replaces the content at `/ui/browse` with the new user-facing browser. The admin affordances currently on that page either migrate to `/ui/vfs` or disappear; change 010 will decide.
- **Nav redesign.** The current `layout.html` nav lists eight admin links. Once `/ui/browse` is the user-facing default, the nav likely wants a visible split ("Browse" up front vs. "Admin: Status/Nodes/…" further in) or a collapsible section. Not in scope for a pure rename.
- **Unifying auth with the JWT API.** Two auth systems (session cookie, JWT) coexist. Merging them is a separate design question not driven by this rename.
- **Module rename for `mosaicfs-server` itself.** The server-rendered UI and the REST API live in the same crate. Splitting them is a larger architectural question not driven here.
- **Renaming `browse.html` / `browse_page` handler.** Keeping the template name keeps this change mechanical. The file can be renamed or replaced when change 010 lands.
- **Backup memory update.** The developer's auto-memory file that mentions `/admin/vfs` URLs (from change 008) will become slightly stale after this change; acceptable because it's a historical record of prior work.
