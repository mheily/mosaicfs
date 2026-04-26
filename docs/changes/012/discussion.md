# Change 012 — Desktop node identity via hostname lookup

## Problem

The Tauri desktop app embeds the `mosaicfs` web_ui in-process (see the in-process server
refactor that landed alongside this discussion). When the user clicks a file to open it,
the server-side handler `mosaicfs-server/src/ui/open.rs::open_file_by_id` needs
`state.node_id` to resolve the file's source path:

- If `source.node_id == state.node_id` → `resolve_same_node` finds the local mount in
  the node doc's `storage[]` and returns a real path.
- Otherwise → `resolve_cross_node` uses `network_mounts[]` on **this** node's doc.
- If `state.node_id` is `None` → `OpenError::NoNodeId` is returned, which is what the
  user currently sees ("Cannot open remote file: this server's node ID is not configured.").

Today the desktop builds its `MosaicfsConfig` with `NodeConfig::default()`, so
`state.node_id` is `None`. Every "open file" attempt fails.

## Proposed approach

1. **Get the host's name via `gethostname(3)`.** A new helper in
   `mosaicfs-common` (or reuse `mosaicfs-agent/src/node.rs::hostname()`, which already
   exists) returns the OS hostname.
2. **Persist hostname on the node doc.** When the agent calls `register_node`, write
   the hostname to a new `hostname` field on the `node::<id>` doc (alongside `name`,
   `platform`, etc.).
3. **Desktop startup looks up its node by hostname.** Before building the in-process
   router, the desktop:
   - Computes its own hostname.
   - Queries CouchDB for `node::*` docs and finds the one whose `hostname` field
     matches.
   - If exactly one match, adopts that `node_id` (sets it on the in-memory
     `MosaicfsConfig`).
   - If zero or multiple matches, surfaces an error to the user — no router is built,
     the "Connecting…" page stays up, and the user is told to register the host.
     A future Change will add a registration form; for now the error message points
     at that gap.

## What needs to change

- `mosaicfs-common` (or `mosaicfs-agent`): expose `hostname()` so both crates use the
  same logic. The agent's existing implementation reads `/etc/hostname` on Linux and
  presumably falls back to `gethostname()` elsewhere — confirm the macOS path works as
  expected.
- `mosaicfs-agent/src/node.rs::register_node`: write a `hostname` field onto the doc.
  Backfill on existing docs by setting it during the next heartbeat as well, so we
  don't need a separate migration step.
- `desktop/src/server.rs::build_router`: add a "resolve node identity" step that runs
  before `build_app_router`. On success, set `cfg.node.node_id`. On failure, return a
  structured error the lib.rs setup path can convert into a visible UI message.
- `desktop/src/lib.rs`: when router build fails for the "no matching node" reason,
  show a clear modal/setup-window message instead of just leaving "Connecting…" up.

## Pushback / risks

The agent code already considered hostname-as-identity and rejected it for one
specific case. Quoting `mosaicfs-agent/src/node.rs:17-18`:

> Inside a container the OS hostname is the pod name, not the physical host, so we
> don't use hostname() here.

That comment is about deriving `node_id` from hostname, which we are *not* proposing
here — `node_id` stays a UUID. We're only adding hostname as a *lookup key* for the
desktop. Still, the same caveat tells us:

1. **Containerized agents.** A `mosaicfs` agent running in podman/docker on the same
   physical host as the desktop will report the container's hostname, not the host's.
   The desktop's `gethostname()` returns the host's hostname. Result: the desktop will
   not find a matching node doc even though the dev environment looks superficially
   the same. The dev script (`scripts/start-dev-environment`) starts CouchDB in a
   container but runs the `mosaicfs` binary directly on the host — so for the *current*
   dev flow this works. For the production flow where the agent runs in a container,
   it does not.
2. **Hostname collisions.** Two laptops both named `MacBook-Air.local` (Apple's default)
   would resolve to the same node doc. Probably fine for a single-user deployment;
   a multi-user catalog needs UUID identity (which is what we already have on
   `_id`).
3. **Hostname instability on macOS.** macOS exposes three names: `gethostname()` (BSD),
   `scutil --get HostName`, and `scutil --get LocalHostName`. They drift, especially
   on laptops on DHCP. We should pick one and document it. The agent's existing
   `hostname()` helper is the right place to centralize this.
4. **Multiple node docs per hostname.** A user who reinstalls the agent without
   cleaning up the DB ends up with two `node::*` docs at the same hostname. The plan
   says "raise an error" in that case — fine for now, but the future registration
   UI should also offer a "claim this existing node" path so the user can resolve
   ambiguity rather than create a third doc.
5. **Privilege/security.** The hostname is not a secret, but it does leak machine
   identity to anyone with read access to the catalog. That's already true for
   agent-registered nodes, so no new exposure for the agent path; the desktop just
   stops being anonymous, which seems fine.

None of these are blockers. The container case (#1) is the only one that breaks the
proposal's promise outright — for users running a containerized agent on the same host
as the desktop, the lookup will fail and they'll have to use the future registration
UI from day one.

## Open questions for the next session

- Pick a single hostname source and document it. Recommendation: `gethostname()`
  (libc) on all platforms, with a one-line override env var (`MOSAICFS_HOSTNAME`) for
  the container case. The agent uses the override when it's set.
- Should the desktop also write a `desktop_last_seen` timestamp on the node doc when
  it adopts identity? Useful for "is this desktop still running?" diagnostics; trivial
  to add.
- What's the user-visible failure UX when no matching node is found? Probably a small
  modal with "We couldn't find a registered node for `<hostname>`. Run the agent
  first, or register this host manually (coming soon)." Tied to the future
  registration form.
- Does the desktop need to *write* anything to its node doc on adoption, or is read-only
  lookup enough? Read-only is simpler and safer; the agent owns the doc.

## Next steps (for next session)

1. Centralize `hostname()` in `mosaicfs-common`. Add the `MOSAICFS_HOSTNAME` env
   override.
2. Add `hostname` field writes in `register_node` and `heartbeat` (backfill via
   heartbeat handles existing docs).
3. Add a "find node by hostname" helper in `mosaicfs-common::couchdb` (or just a
   `_find` query in the desktop).
4. Rewire `desktop/src/server.rs::build_router` to look up the node_id, fail loudly
   if missing.
5. Add the user-facing error UX (probably reuse the setup window pattern).
6. (Future) Registration UI for hosts that don't yet have a node doc.
