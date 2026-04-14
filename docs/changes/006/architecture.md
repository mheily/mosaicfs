# Architecture Change 006: Unified Binary + TOML Feature Config

This change merges `mosaicfs-server` and `mosaicfs-agent` into a single
`mosaicfs` binary whose enabled components are selected at runtime by a
`[features]` block in the node's TOML config. Each node is self-sufficient
— a NAS runs `agent + web_ui`, a laptop runs `agent + vfs`, a headless
indexer runs `agent` alone.

Depends on change 005 (Loco must be the web layer before the
server/agent split disappears — otherwise the unified binary still has to
serve a React static bundle that change 005 is trying to delete).

## Current State Summary

_Verified against the tree at the head of `master` (post change 003)._

**Crates:** `mosaicfs-common`, `mosaicfs-agent`, `mosaicfs-server`,
`mosaicfs-vfs`. Two binaries: `mosaicfs-server` (admin REST + UI) and
`mosaicfs-agent` (filesystem watcher, replication runner, heartbeat
publisher). Both are built from the same workspace and packaged into one
container image.

**Configuration:** `agent.toml.example` documents the agent's config
(`control_plane_url`, `watch_paths`, `excluded_paths`, `access_key_id`,
`secret_key`, optional `node_id`). The server reads its config via env
vars (`COUCHDB_URL`, `COUCHDB_USER`, `COUCHDB_PASSWORD`) and a few
command-line flags. There is no single config file for "this node."

**Deployment:** `deploy/mosaicfs.yaml` runs both binaries as separate
containers in one pod, plus CouchDB. The agent talks to the server via
`control_plane_url`. The server talks to CouchDB directly.

**Process model assumption:** The codebase assumes "server" and "agent"
are different processes that communicate over HTTP. After change 003
removed the inter-node transport, the server↔agent HTTP surface is small
(bootstrap, credential issuance, notification delivery) but still
present.

## Goal

Collapse the two binaries into one `mosaicfs` binary that reads a single
TOML config and starts only the components named in `[features]`. The
node becomes the unit of deployment — there is no "server node" vs.
"agent node" distinction in the binary, only in the config. This matches
the project direction (peers, not control-plane/worker) and removes the
internal HTTP hop between server and agent on nodes that run both.

## Changes

### Change A — Single binary, feature-gated subsystems

**Today:** `mosaicfs-server/src/main.rs` and
`mosaicfs-agent/src/main.rs` each start their own subsystem set. Cargo
builds two separate binaries.

**Proposed:** One `mosaicfs` binary (decide in phase 1 whether to
introduce a new `mosaicfs` crate or grow one of the existing crates into
the binary host). The binary loads `/etc/mosaicfs/mosaicfs.toml` (or a
path from `--config`), inspects `[features]`, and starts the subsystems
named there:

```toml
[node]
node_id = "node-abc123"           # optional, persisted on first run

[features]
agent  = true                     # filesystem watcher, replication runner, heartbeat
vfs    = true                     # FUSE mount
web_ui = false                    # Loco admin UI + REST API

[agent]
watch_paths    = ["/data"]
excluded_paths = []

[vfs]
mount_point = "/mnt/mosaicfs"

[web_ui]
listen = "0.0.0.0:8443"

[couchdb]
url      = "http://localhost:5984"
user     = "admin"
password = "…"

[credentials]
access_key_id = "MOSAICFS_…"
secret_key    = "…"
```

Subsystems are independent modules within the binary. They share the
CouchDB client (consolidated in change 004), the notification publisher
(also from 004), and the same config struct. Inter-subsystem
communication is in-process channels or shared state; the previous
agent↔server HTTP hop disappears for any node that runs both.

**Justification:** The two-binary split made sense when MosaicFS was
control-plane / worker. After change 003 (no inter-node transport) and
the project direction toward peer nodes, the split is overhead: an extra
container per pod, an extra HTTP surface to maintain, an extra place to
mis-configure auth. One binary with feature toggles maps directly to
"each node decides what it does."

### Change B — Unified TOML config

**Today:** Agent reads TOML; server reads env + flags. There is no single
"node config."

**Proposed:** One TOML file describes the node. Required sections:
`[node]`, `[features]`, `[couchdb]`. Optional sections gate on the
features enabled — `[agent]` is required iff `features.agent = true`,
etc. Validation runs at startup with a clear error if a required section
is missing for an enabled feature.

Env-var overrides remain available for secrets (`COUCHDB_PASSWORD`,
`MOSAICFS_SECRET_KEY`) so that container deployments can keep secrets out
of the file. Settle the precedence rules in phase 1 (env wins over
file is the conventional choice).

**Justification:** Two configuration mechanisms for one node forces
operators to remember which knob lives where. One file with one schema is
easier to document, easier to template (Ansible/Helm/Nix), and easier to
back up.

### Change C — Update deployment manifest and docs

**Today:** `deploy/mosaicfs.yaml` runs three containers (CouchDB, server,
agent). `DEPLOYMENT.md` documents three-container layout.
`agent.toml.example` is agent-only.

**Proposed:** The pod manifest runs CouchDB + one `mosaicfs` container
configured with `features = { agent, vfs, web_ui }`. The agent.toml
example is replaced by a `mosaicfs.toml.example` that shows the unified
schema with all features enabled and comments showing how to disable each.
`DEPLOYMENT.md` documents the per-node-role pattern (NAS, laptop,
indexer) with example configs for each.

**Justification:** A pod manifest that still runs two containers after
the binary unifies is a missed opportunity — the whole point is fewer
moving parts at deploy time. The per-role examples make the
self-sufficient-node model concrete.

## Implementation Phases

Phases land in deployable increments. The two-binary build keeps working
through Phase 2; Phase 3 is the cutover.

**Phase 1 — Unified config schema.**
Define the new TOML schema and parser in `mosaicfs-common::config` (or a
new `mosaicfs-config` module). Ship parser + validator + tests with no
consumer yet. Settle the env-var override precedence.

**Phase 2 — Subsystem modules with shared startup.**
Refactor `mosaicfs-server/src/main.rs` and `mosaicfs-agent/src/main.rs`
so that each binary's startup logic moves into a `start_*` function
exposed by its crate (`mosaicfs_server::start_web_ui(cfg)`,
`mosaicfs_agent::start_agent(cfg)`, `mosaicfs_vfs::start_vfs(cfg)`). The
two binaries continue to exist — each one calls one start function. This
is the "make the change easy" step.

**Phase 3 — `mosaicfs` binary.**
Add the new binary (host crate). Its `main` reads the unified config,
inspects `[features]`, and calls each enabled subsystem's `start_*`
function. The two old binaries are deleted in this same phase. Update
the `Makefile`, `Dockerfile`, and `Dockerfile.mosaicfs` accordingly.

**Phase 4 — Deployment + docs.**
Update `deploy/mosaicfs.yaml` to one app container. Replace
`agent.toml.example` with `mosaicfs.toml.example`. Update
`DEPLOYMENT.md` with per-role config examples. Update `DEVELOPMENT.md`
to reflect the single-binary local-dev workflow.

**Phase dependencies:**

- Phase 2 requires Phase 1 (the start functions take the unified config).
- Phase 3 requires Phase 2 (the host calls into the start functions).
- Phase 4 requires Phase 3 (manifest cannot reference a binary that does
  not exist yet).

## What Does Not Change

- **Document model and CouchDB schema.** No doc-type changes, no
  migration. Federation still happens through CouchDB.
- **REST API surface.** Routes keep their shapes. They just live in a
  binary that may or may not have other subsystems running alongside.
- **Loco admin UI (change 005).** The Loco app moves into the
  `web_ui` feature module unchanged.
- **VFS access path.** The Tier 1/2/3 logic from change 003 is
  unaffected; the VFS just starts as an in-process subsystem instead of
  a sidecar binary.
- **Agent watcher, replication runner, heartbeat publisher.** Same
  logic, same documents written, same triggers. Only the process
  boundary disappears.
- **Authentication.** Access key + HMAC continues to gate the API.
  Inter-subsystem calls within the binary do not need HMAC (they are
  in-process); cross-node calls (e.g., agent posting heartbeat to a
  remote web_ui) still go through the API and still authenticate.
- **CouchDB itself.** Still runs as its own container in the pod.
  Single-process MosaicFS does not embed CouchDB.
- **Code consolidation (change 004).** Already landed; this change
  builds on the shared client/notification/replication modules.
- **Secrets manager (change 007).** Out of scope here; this change
  leaves `[credentials]` as plaintext TOML (or env vars) and 007 adds
  the keychain backend on top of the config schema this change
  defines.

## Deferred

- **Per-feature dynamic enable/disable.** Features are read at startup;
  toggling at runtime requires a restart. Live reconfiguration is
  deferred.
- **Multi-tenant single binary.** One process serves one node. Running
  multiple "nodes" in one process is not a goal.
- **Embedded metadata store.** CouchDB stays external. Embedding
  CouchDB or replacing it (redb, sqlite) is deferred to v2 per project
  decisions.
- **Service-manager integration (systemd units, launchd plists).** The
  pod manifest is the supported deployment. Native service files can
  come later if there is demand.
- **Hot-reload of config.** SIGHUP-style reloads are deferred. Restart
  the binary to pick up changes.
- **Removing CouchDB from the pod.** Out of scope; metadata store is
  unchanged.
- **macFUSE FileProvider and redb.** Deferred to v2.
