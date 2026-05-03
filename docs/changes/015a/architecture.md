# Change 015a: Remove Untested/Stub Features Before Database Migration

> **Prerequisite to change 016** (CouchDB → SQLite migration). This
> change ships before 016's implementation work begins so the
> migration only has to carry features that are real and verified.
> Per the project's YAGNI principle and "one moving part at a time"
> rule, untested code should not ride along on a database swap.

## Goal

Remove three feature areas that are either pure scaffolding (no
implementation) or untestable (no working integration test
infrastructure), so change 016 has a smaller, more honest scope.
Designs and source pointers are preserved in `docs/future/` for clean
reintroduction later.

## Recovery anchor

**Commit prior to removal: `e44e1de` (`master`).**

To restore any removed file later:
```sh
git show e44e1de:<path> > <path>
```
The file inventory in §"What is removed" below is the canonical list
of paths that can be recovered this way.

---

## Current State Summary

_Inventory verified at commit `e44e1de`._

### Workspace
- `mosaicfs-common` (3,508 LOC)
- `mosaicfs-agent` (3,125 LOC) — includes the replication subsystem
- `mosaicfs-server` (9,580 LOC) — includes replication handlers + UI
- `mosaicfs-vfs` (3,176 LOC)
- `mosaicfs` (binary, 436 LOC)

### Plugin and annotation surface (pure stubs)
- **Routes** in `mosaicfs-server/src/routes.rs:102-110`: 8 routes,
  all returning `stub_501`.
  ```
  /api/annotations          GET, DELETE
  /api/nodes/{node_id}/plugins                        GET, POST
  /api/nodes/{node_id}/plugins/{plugin_name}          GET, PATCH, DELETE
  /api/nodes/{node_id}/plugins/{plugin_name}/sync     POST
  ```
- **Document types** in `mosaicfs-common/src/documents.rs`:
  `PluginDocument`, `PluginType`, `QueryEndpoint`,
  `AnnotationDocument`, `AnnotationType`, `AnnotationSource`,
  plus their unit tests.
- **Handlers**: none. No file in `mosaicfs-server/src/handlers/` for
  these — the routes point directly at `stub_501`.
- **Runtime references outside `documents.rs`**: zero (verified by
  grep across all `*.rs` files).

### Replication surface (untested)
- **Routes**: 11 routes under `/api/replication/*` registered in
  `mosaicfs-server/src/routes.rs`, plus the UI action endpoints under
  `/ui/replication/*` in `mosaicfs-server/src/ui/actions.rs`.
- **Handler crate file**: `mosaicfs-server/src/handlers/replication.rs`
  (871 LOC).
- **Agent crate files**:
  - `mosaicfs-agent/src/replication_subsystem.rs` (1,243 LOC) — the
    rule-engine + worker pool that consumes file events and writes to
    backends.
  - `mosaicfs-agent/src/replication.rs` (86 LOC) — the CouchDB-to-CouchDB
    push/pull setup. Already on death row in change 016 since it's
    purely a CouchDB integration; called out here for completeness.
  - `mosaicfs-agent/src/backend/` (4 files, 805 LOC total): backend
    abstraction plus three implementations (`agent_target.rs`,
    `directory.rs`, `s3.rs`).
- **Common crate file**: `mosaicfs-common/src/replication.rs` (38 LOC).
- **Document types** in `mosaicfs-common/src/documents.rs`:
  `ReplicaDocument`, `ReplicaType`, `ReplicaSource`,
  `ReplicationRuleDocument`, `ReplicationRuleType`,
  `ReplicationRuleSource`, `StorageBackendDocument`,
  `StorageBackendType`, `RetentionConfig`, plus their unit tests.
- **Templates**: `mosaicfs-server/templates/replication.html` (66 LOC),
  `mosaicfs-server/templates/replication_panel.html` (45 LOC).
- **State coupling**: `mosaicfs-server/src/state.rs` defines
  `restore_jobs: RestoreJobStore` referencing types from the
  replication handler.
- **UI actions**: `mosaicfs-server/src/ui/actions.rs` lines 552-595
  define `initiate_restore_action` and `cancel_restore_action`.
- **Crawler coupling**: `mosaicfs-agent/src/crawler.rs` imports
  `ReplicationHandle` and dispatches `FileEvent`s to it.
- **Agent startup**: `mosaicfs-agent/src/start.rs` starts the
  replication subsystem.
- **Tests**: `tests/integration/test_06_replication.sh` (169 lines, 6
  test cases). Unit tests in `replication_subsystem.rs` cover only
  utility functions (`parse_schedule`, `in_schedule_window`,
  `TokenBucket`).

### What is intentionally NOT removed

Several types and modules look replication-adjacent but are actually
load-bearing for the VFS:

- **`mosaicfs-common/src/steps.rs`** (542 LOC): the step pipeline
  engine. Used by `mosaicfs-vfs/src/readdir.rs` and
  `mosaicfs-server/src/readdir.rs` for virtual directory mount
  filtering. Stays.
- **In `documents.rs`**: `Step`, `StepResult`, `MountEntry`,
  `MountSource`, `MountStrategy`, `ConflictPolicy` — used by
  `VirtualDirectoryDocument.mounts`. Stay.
- **`CredentialDocument`**: used by the auth layer; stays.

---

## Changes

### 1. Remove plugin scaffolding

**Today.** 6 plugin routes return `stub_501`. `PluginDocument` and
related types defined but never read or written by any code path.

**Proposed.** Delete the routes from
`mosaicfs-server/src/routes.rs:105-110`. Delete `PluginDocument`,
`PluginType`, `QueryEndpoint`, `default_workers`, `default_timeout`,
`default_max_attempts`, and `test_plugin_document` from
`mosaicfs-common/src/documents.rs`.

**Justification.** No implementation; nothing breaks. Reduces 016
schema scope by one table and ~18 columns of speculative design.

### 2. Remove annotation scaffolding

**Today.** 2 annotation routes return `stub_501`. `AnnotationDocument`
and related types defined but never read or written.

**Proposed.** Delete the routes from
`mosaicfs-server/src/routes.rs:102-103`. Delete `AnnotationDocument`,
`AnnotationType`, `AnnotationSource`, and `test_annotation_document`
from `mosaicfs-common/src/documents.rs`.

**Justification.** Same as plugins. Reduces 016 schema scope by one
table.

### 3. Remove replication subsystem

**Today.** ~2,500 LOC of replication code across the agent, server,
and common crates. 11 REST routes, UI templates and actions, integration
test that depends on a docker-compose environment that does not currently
build. Unit-test coverage is limited to small utility functions.

**Proposed.** Delete the files and references listed in
"What is removed" below. Edit the call sites listed in "Edits to
remaining files" so the workspace still builds.

**Justification.**
- The developer cannot currently verify the feature works.
- Migrating untested code through a database swap is the worst case
  for finding bugs (you can't tell if a regression came from the
  migration or pre-existed).
- Per the decisions doc: "Prefer a working system with fewer features
  over a comprehensive design that takes longer to deliver."
- The design is preserved for clean reintroduction (see "Stash design
  in `docs/future/`" below).

### 4. Stash designs in `docs/future/`

**Proposed.** Create three new files capturing what was removed so the
ideas survive even though the code does not:

- `docs/future/plugins.md` — copy `PluginDocument` struct + the route
  shapes + a paragraph on intent.
- `docs/future/annotations.md` — same shape for annotations.
- `docs/future/replication.md` — copy `ReplicaDocument`,
  `ReplicationRuleDocument`, `StorageBackendDocument`, `RetentionConfig`,
  `Step`/`StepResult` (note that step types still exist in
  `documents.rs` for VFS use), the route shapes, the backend trait
  interface, and a paragraph each on the rule-engine and backend
  abstractions.

Each file's header says: `Frozen at commit e44e1de. Source files
listed below; recover with git show e44e1de:<path>.`

**Justification.** Cheap insurance against losing the design thinking.
A future reimplementation has a starting reference rather than a clean
slate.

---

## What is removed (file inventory for recovery)

### Files deleted entirely
```
mosaicfs-agent/src/replication.rs                                86 LOC
mosaicfs-agent/src/replication_subsystem.rs                  1,243 LOC
mosaicfs-agent/src/backend/mod.rs                              118 LOC
mosaicfs-agent/src/backend/agent_target.rs                     171 LOC
mosaicfs-agent/src/backend/directory.rs                        163 LOC
mosaicfs-agent/src/backend/s3.rs                               353 LOC
mosaicfs-server/src/handlers/replication.rs                    871 LOC
mosaicfs-common/src/replication.rs                              38 LOC
mosaicfs-server/templates/replication.html                      66 LOC
mosaicfs-server/templates/replication_panel.html                45 LOC
tests/integration/test_06_replication.sh                       169 LOC
                                                            -------
                                                             3,323 LOC
```

To recover any of these:
```sh
git show e44e1de:mosaicfs-agent/src/replication_subsystem.rs > /tmp/restored.rs
```

### Symbols deleted from `mosaicfs-common/src/documents.rs`
- `PluginDocument`, `PluginType`, `QueryEndpoint`,
  `default_workers`, `default_timeout`, `default_max_attempts`
- `AnnotationDocument`, `AnnotationType`, `AnnotationSource`
- `ReplicaDocument`, `ReplicaType`, `ReplicaSource`
- `ReplicationRuleDocument`, `ReplicationRuleType`,
  `ReplicationRuleSource`
- `StorageBackendDocument`, `StorageBackendType`, `RetentionConfig`
- Tests: `test_plugin_document`, `test_annotation_document`,
  `test_replica_document`, `test_replication_rule_document`,
  `test_storage_backend_document`

### Routes deleted from `mosaicfs-server/src/routes.rs`
- Plugin routes (lines 105-110): 6 routes pointing at `stub_501`
- Annotation routes (lines 102-103): 2 routes pointing at `stub_501`
- Replication routes (the 11 `/api/replication/*` entries)
- The `replication` module from the `use crate::handlers::{...}` import
  on line 17

### Edits to remaining files (so the workspace still builds)

| File | Edit |
|---|---|
| `mosaicfs-server/src/routes.rs` | Drop `replication` from `use` import; drop the 11 plugin/annotation/replication routes |
| `mosaicfs-server/src/state.rs` | Drop `use crate::handlers::replication::{RestoreJob, RestoreJobStore}`; drop `restore_jobs` field and its initializer |
| `mosaicfs-server/src/ui/actions.rs` | Drop `replication as rephandlers` from imports; drop `initiate_restore_action` and `cancel_restore_action` (lines 552-595); remove their UI route bindings in `mosaicfs-server/src/ui/mod.rs` |
| `mosaicfs-agent/src/start.rs` | Drop `use crate::replication_subsystem`; remove the replication-subsystem startup block |
| `mosaicfs-agent/src/crawler.rs` | Drop `use crate::replication_subsystem::{FileEvent, ReplicationHandle}`; remove the dispatch calls (the crawler reverts to indexing-only) |
| `mosaicfs-agent/src/lib.rs` | Drop `pub mod replication;`, `pub mod replication_subsystem;`, `pub mod backend;` |
| `mosaicfs-common/src/lib.rs` | Drop `pub mod replication;` |

### Tests deleted
- `tests/integration/test_06_replication.sh` (169 LOC)
- The 3 unit tests inside `replication_subsystem.rs` (deleted with the file)

### What else might surface during implementation
- Layout/template links in `templates/layout.html` or admin nav
  pointing at the replication page — remove the link, keep the page
  template gone.
- Imports in `templates/status_panel.html` that summarize replication
  status — remove that section, keep the rest of the panel.
- Any `Cargo.toml` dependencies that were only pulled in for backend
  implementations (e.g., S3 client crates) — drop unused entries.

---

## Implementation Phases

This is a single small change. One commit (or two if you prefer to
split docs from code).

1. **Code removal.** Delete the files listed above. Apply the edits to
   the remaining files. Verify `cargo build` and `cargo test --workspace`
   pass. Verify `cargo clippy` is clean.
2. **Stash documentation.** Create `docs/future/{plugins,annotations,replication}.md`
   each containing the relevant struct definitions, route shapes, and
   a paragraph of design intent. Headers note `Frozen at commit
   e44e1de`.
3. **Update `docs/changes/016/`.** Drop the affected document types
   from `architecture.md` (the schema-categorization list in resolved
   decision 1) and from `design-notes.md` §4 (remove the `plugin`,
   `annotation`, `replica`, `replication_rule`, `storage_backend`
   table sections).

---

## What Does Not Change

- The step pipeline engine (`mosaicfs-common/src/steps.rs`) and its
  consumers (`mosaicfs-vfs/src/readdir.rs`,
  `mosaicfs-server/src/readdir.rs`) — used by VFS for mount filtering.
- `Step`, `StepResult`, `MountEntry`, `MountSource`, `MountStrategy`,
  `ConflictPolicy` in `documents.rs` — used by `VirtualDirectoryDocument`.
- Labels — fully implemented (9 routes, 496-LOC handler, `LabelCache`).
- VFS, files, virtual directories, filesystems, credentials, agent
  status, utilization snapshots, notifications, access tracking — all
  remain intact.
- The change-016 plan structure — only the per-document-type details
  shrink.

---

## Deferred (still on the roadmap, just not in this codebase)

- **Replication.** Reintroduce after change 016 lands. New
  implementation gets fresh SQL schemas designed alongside the code,
  fresh integration tests against the new SQLite-backed harness, and
  honest unit-test coverage for the rule engine and backend
  implementations. Reference design in `docs/future/replication.md`.
- **Plugins.** Reintroduce when there is a concrete first plugin to
  ship. Reference design in `docs/future/plugins.md`.
- **Annotations.** Tied to plugins (annotations are how plugins
  surface results); reintroduce together. Reference design in
  `docs/future/annotations.md`.
