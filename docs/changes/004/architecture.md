# Architecture Change 004: Code Consolidation

This change extracts the modules that have been copy-pasted across the four
crates into a single shared home, so that subsequent changes (005 Loco UI,
006 unified binary) start from a workspace where each concern lives in one
place. It is purely structural — no behavior changes, no schema changes, no
new features.

## Current State Summary

_Verified against the tree at the head of `master` (post change 003)._

**Workspace:** `mosaicfs-common`, `mosaicfs-agent`, `mosaicfs-server`,
`mosaicfs-vfs`. The first holds shared types (documents, steps, backend
enums); the other three each hold their own copy of several modules that
started as one file and drifted as features landed in different binaries.

**Duplicated modules** (line counts at HEAD):

- `couchdb.rs` — three copies:
  - `mosaicfs-vfs/src/couchdb.rs` (231)
  - `mosaicfs-agent/src/couchdb.rs` (271)
  - `mosaicfs-server/src/couchdb.rs` (406)
  All three implement a thin wrapper around CouchDB HTTP: GET/PUT a doc by id,
  `_find` queries against the typed indexes, `_changes` feed setup. The
  server copy carries extra helpers for bulk operations and view queries; the
  agent copy carries heartbeat-specific upserts; the vfs copy is read-only.

- `notifications.rs` — three copies:
  - `mosaicfs-agent/src/notifications.rs` (111)
  - `mosaicfs-server/src/notifications.rs` (96)
  - `mosaicfs-server/src/handlers/notifications.rs` (273)
  The first two are publishers (write `notification::*` docs into CouchDB);
  the third is the HTTP handler that serves notifications to the React UI.
  Publisher logic — id construction, severity levels, retention pruning — has
  drifted between the agent and server copies.

- `replication.rs` — two copies:
  - `mosaicfs-agent/src/replication.rs` (86) plus
    `mosaicfs-agent/src/replication_subsystem.rs`
  - `mosaicfs-server/src/handlers/replication.rs` (871)
  Agent side runs replication jobs against `StorageBackendDocument` targets;
  server side exposes the REST routes that configure rules and inspect job
  state. They share the rule/job document shapes (already in
  `mosaicfs-common`) but each redefines small helpers — backend-target
  resolution, rule-matching predicates, status enums — locally.

- `readdir.rs` — two copies:
  - `mosaicfs-vfs/src/readdir.rs` (307)
  - `mosaicfs-server/src/readdir.rs` (363)
  Both resolve a virtual-directory path against `VirtualDirectoryDocument`
  and the file-source index. The vfs copy returns FUSE-shaped entries; the
  server copy returns JSON for the API. They share the underlying mount-entry
  evaluator and conflict-policy logic but each carries its own copy.

**No shared crate** sits between `mosaicfs-common` (pure types) and the
binary crates. Anything that needs to do I/O or talk to CouchDB ends up
duplicated.

## Goal

Bring each duplicated module to one canonical location and have all consumers
import that location. Establish a place for shared infrastructure (CouchDB
client, notification publisher) that does I/O — `mosaicfs-common` is for
types only and should stay that way.

## Changes

### Change A — Extract a shared CouchDB client

**Today:** Three near-identical wrappers around `reqwest` calls to CouchDB.
Each binary maintains its own `CouchClient` struct, its own retry/backoff,
its own `_find` query builder.

**Proposed:** Move the union of useful methods into
`mosaicfs-common::couchdb` (or a new `mosaicfs-couchdb` crate if pulling
`reqwest` into `common` would over-broaden its dependency surface — decide
during phase 1). One `CouchClient` type. Specialized helpers (server bulk
ops, agent heartbeat upserts, vfs read-only views) become methods or
free functions in the same module rather than separate types.

**Justification:** CouchDB is the federation substrate; the wire details
should not be reimplemented per binary. With three copies, a bug fix or
schema convention change has to be made three times and it is easy to miss
one. Consolidation is a prerequisite for change 006 (single binary), where
having three `CouchClient` types in the same process would be obviously
wrong.

### Change B — Extract a shared notification publisher

**Today:** `mosaicfs-agent/src/notifications.rs` and
`mosaicfs-server/src/notifications.rs` both write `notification::*` docs
into CouchDB; `mosaicfs-server/src/handlers/notifications.rs` reads them
back for the API.

**Proposed:** Move the publisher (id construction, severity handling,
retention) into `mosaicfs-common::notifications`. The HTTP handler in
`mosaicfs-server/src/handlers/notifications.rs` stays where it is — it
belongs to the server's REST surface — and depends on the shared publisher
for any writes it does.

**Justification:** The publisher is shared logic with no HTTP surface; the
handler is HTTP and stays in the server. Splitting the file along that line
prevents the current drift where the agent and server format
"replication failed" notifications slightly differently.

### Change C — Extract shared replication helpers

**Today:** Agent runs jobs; server serves config/status routes. Both
re-derive the same rule-matching predicates and target-resolution logic
locally.

**Proposed:** Pull the rule-matching predicates, target-resolution helpers,
and status enums into `mosaicfs-common::replication` (next to where the
rule/job document types already live). The 871-line server handler keeps
its HTTP routing; the 86-line agent module keeps its job runner. Only the
shared logic moves.

**Justification:** The duplication here is smaller in line count but
semantically dangerous — the rule-matching predicate is the contract
between the two sides. If they disagree about whether a rule applies to a
file, the UI shows one answer and the agent does another.

### Change D — Extract a shared readdir evaluator

**Today:** Two copies of the virtual-directory evaluator, one returning
FUSE entries and one returning JSON.

**Proposed:** Move the evaluator (mount-entry walk, conflict-policy
application, source-index queries) into `mosaicfs-vfs::readdir` and have
`mosaicfs-server/src/readdir.rs` shrink to a thin wrapper that calls into
`mosaicfs-vfs` and converts the result to its JSON shape. `mosaicfs-vfs`
becomes a dependency of `mosaicfs-server` (it already depends on
`mosaicfs-common`); cycles do not exist because `mosaicfs-server` does not
depend on the agent and `mosaicfs-vfs` does not depend on either binary
crate.

**Justification:** Readdir semantics are the most user-visible part of the
namespace. Two copies guarantees that the FUSE listing and the API
listing eventually disagree on edge cases (conflict resolution between
overlapping mount entries, hidden-file handling, timestamp precision). One
evaluator with two output adapters is the right shape.

## Implementation Phases

Phases are organized by topical focus, not by deployability. The tree must
build at the end of each phase (this is a refactor — there is no "broken
intermediate state" excuse).

**Phase 1 — CouchDB client.**
Decide between extending `mosaicfs-common` vs. adding a new
`mosaicfs-couchdb` crate. Move the union of methods into the chosen home.
Update `mosaicfs-vfs`, `mosaicfs-agent`, and `mosaicfs-server` to import the
shared client. Delete the three local copies. Run the existing test suite —
no behavior should change.

**Phase 2 — Notifications publisher.**
Move publisher logic into `mosaicfs-common::notifications`. Update both
binaries to depend on it. Leave
`mosaicfs-server/src/handlers/notifications.rs` in place (HTTP handler).
Delete `mosaicfs-agent/src/notifications.rs` and
`mosaicfs-server/src/notifications.rs`.

**Phase 3 — Replication helpers.**
Move rule-matching, target-resolution, and shared enums into
`mosaicfs-common::replication`. Update both crates to use them. The agent
job runner and server REST handler stay in their respective crates.

**Phase 4 — Readdir evaluator.**
Make `mosaicfs-vfs` a dependency of `mosaicfs-server`. Move the evaluator
core into `mosaicfs-vfs::readdir`. Reduce `mosaicfs-server/src/readdir.rs`
to a JSON-shaped wrapper. Delete duplicated evaluator code.

**Phase dependencies:** All four phases are independent and can land in any
order. Doing them in this order minimizes diff size per PR (Phase 1 touches
the most files; Phase 4 introduces a new inter-crate edge that is easier to
review on a quiet tree).

## What Does Not Change

- **Document model.** No new document types, no field changes.
  `mosaicfs-common::documents` is untouched.
- **REST API surface.** All `/api/*` routes keep their shapes and behavior.
  Handlers move only to the extent of swapping out an internal `use`.
- **CouchDB schema.** No new id conventions, no new indexes, no migration.
- **Deployment.** Same two binaries, same container image, same pod
  manifest. (Unification is change 006.)
- **VFS access path.** Tier 1/2/3 logic from change 003 is unaffected.
- **React UI.** The frontend continues to consume the same REST surface.
  (Replacement is change 005.)
- **Authentication.** Auth stack and credential handling are untouched.
- **Loco/HTMX migration (005), unified binary (006), secrets manager (007).**
  This change is a prerequisite for 005 and 006 but does nothing toward
  them on its own.

## Deferred

- **Splitting `mosaicfs-common` into typed-only and runtime sub-crates.**
  If pulling `reqwest` into `common` proves too heavy a dependency for the
  vfs crate's link time, introduce `mosaicfs-couchdb` as its own crate. Do
  not pre-emptively split — measure first.
- **Consolidating `auth/`, `credentials.rs`, `access_cache.rs`,
  `label_cache.rs`, `readdir_cache.rs`.** These live only in the server
  today and are not duplicated. They may move during change 006 when the
  binary boundary disappears, but not as part of this change.
- **Trait-based abstraction over CouchDB.** A `MetadataStore` trait would
  let tests run against an in-memory backend. Useful, but a separate
  change — this one is a flat extract.
- **Removing `mosaicfs-agent/src/replication_subsystem.rs`.** It contains
  agent-side scheduling that is not duplicated; it stays put.
