# Architecture Review Comments — Round 1

**Date:** 2026-02-18
**Reviewer:** Claude
**Document:** docs/architecture.md (v0.1 Draft)

---

## Critical Blockers for Implementation

### 1. ~~CouchDB Replication Filter Functions~~ — RESOLVED
Added `_replicator` document structure for Flow 1 (push) and Flow 2 (pull), including the Axum replication proxy endpoint, TLS verification, and replication health monitoring.

### 2. ~~Agent-to-Agent File Transfer Discovery~~ — RESOLVED
Added discovery sequence to Tier 4 in the VFS Tiered Access Strategy: file doc -> node doc -> `transfer.endpoint` -> HMAC-signed GET request. Clarified cloud bridge nodes are reachable at the control plane's address.

### 3. ~~CouchDB Conflict Resolution Strategy~~ — RESOLVED
Added "CouchDB Document Conflict Resolution" section covering per-document-type strategies: optimistic concurrency for user-edited docs, last-write-wins for node docs, exclusive ownership for agent-written docs. Added conflict monitoring background task.

### 4. ~~Conflict Policy Ambiguity Across Mounts~~ — RESOLVED
Added rule: when colliding files come from mounts with different policies, the more conservative policy (`suffix_node_id`) wins.

### 5. ~~Circular Mount Detection~~ — NOT A BLOCKER
Mount sources reference `{node_id, export_path}` — they pull from real filesystem paths on physical nodes, not from other virtual directories. Circular mounts are not possible by design. Virtual directories can nest as parent/child, but mount evaluation always queries file documents by real export path, never by virtual path.

### 6. ~~Plugin Invocation Contract~~ — RESOLVED
Added "Executable plugin invocation contract" table covering working directory, user/group, stdin/stdout/stderr handling, exit codes (including EX_CONFIG=78 for permanent errors), timeout/signal behavior, stdout size limit (10 MB), environment, arguments, and file descriptors.

---

## Internal Inconsistencies

### 7. ~~Document Type Count~~ — RESOLVED
Count was wrong: there are 11 document types in v1, not 8 or 12. Fixed in architecture doc, implementation plan, and implementation plan updates. The 11 types are: `file`, `virtual_directory`, `node`, `credential`, `agent_status`, `utilization_snapshot`, `label_assignment`, `label_rule`, `plugin`, `annotation`, `notification`.

### 8. ~~Bridge Node Role Field~~ — RESOLVED
Clarified: `role` is omitted (not present in JSON) for physical nodes, set to `"bridge"` for bridge nodes. Code should treat both absent and `null` as equivalent.

### 9. ~~Plugin Event Subscription Validation~~ — RESOLVED
Clarified: the API does not reject `crawl_requested`/`materialize` subscriptions on non-bridge nodes. The events simply never fire, so the subscription is harmless. This avoids coupling plugin validation to node configuration.

---

## Security Concerns

### 10. ~~Plaintext Secrets in Backups~~ — RESOLVED
Added secret redaction: plugin `settings` fields declared as `type: "secret"` are replaced with `"__REDACTED__"` in backup files. Users re-enter secrets after restore.

### 11. ~~Path Traversal in `export_path`~~ — RESOLVED
Added `export_path` containment check to the transfer server: local file opens verify that the canonicalized path is under a configured watch path. Rejects with 403 on failure. Bridge nodes skip this check (files served from cache or materialized by plugins).

### 12. ~~Plugin Execution Privileges~~ — RESOLVED
Added explicit "Privilege model" paragraph: plugins run as same user as agent (typically root on Linux). Plugin directory is the security boundary. v1 does not sandbox plugins. Noted that future versions could use namespaces or unprivileged users.

### 13. ~~No Rate Limiting on Login~~ — RESOLVED
Added rate limiting to `/api/auth/login`: 5 attempts per minute per source IP. Generic 401 on failure to prevent credential enumeration.

### 14. ~~JWT Secret Management~~ — RESOLVED
Added "JWT signing key" paragraph: 256-bit random key generated at first startup, stored in Docker Compose volume. No rotation in v1; future rotation via dual-key transition window.

### 15. ~~HMAC Clock Skew~~ — RESOLVED
Added "Clock skew handling" paragraph: 5-minute window is bidirectional. Agent logs server `Date` header on auth failures. Persistent failures surface via notification system. Clock management is host OS responsibility.

---

## Under-Specified Areas

### 16. ~~Error Recovery Procedures~~ — RESOLVED
Added standardized retry parameters: initial delay 1s, 2x multiplier, 60s cap, ±25% jitter. Added table specifying max attempts and exhaustion behavior for each retry context (plugin jobs, socket reconnect, HTTP transfer, replication, heartbeat, bridge polling).

### 17. ~~Database Transaction Boundaries~~ — RESOLVED
Added "Atomicity model" paragraph to the Data Model section. CouchDB has no multi-document transactions; `_bulk_docs` is per-document atomic. MosaicFS is designed so no operation requires atomically updating two documents. Temporary inconsistency between related documents is tolerated. SQLite sidecars use local transactions.

### 18. ~~API Versioning Strategy~~ — RESOLVED
Added versioning policy: v1 is unversioned under `/api/`. Future breaking changes use `/api/v2/` with `/api/` kept as v1 alias for one release cycle. Additive changes (new fields, new endpoints) are non-breaking. Clients should ignore unknown fields.

### 19. ~~Stale Agent Status Detection~~ — RESOLVED
Added "Stale status detection" paragraph. Control plane is sole authority on node status via polling. Crashed agents detected within 90 seconds. On control plane restart, all nodes treated as unknown and re-polled. No scenario for permanently stuck "online" status.

### 20. ~~Scale Limits~~ — RESOLVED
Added "Target scale" table to Problem Statement section defining "home-deployment scale": 500K files, 20 nodes, 500 virtual directories, 200 label rules/node, 10 plugins/node, etc. Degradation beyond these limits is gradual, not catastrophic.

---

## Over-Specified Areas

### 21. ~~UI Layout Details~~ — RESOLVED
Added note at the top of the Web Interface section: "The UI descriptions below are design guidance for the implementer, not a rigid specification." Layout details retained as they're useful for AI-assisted implementation.

### 22. ~~Hardcoded Default Values~~ — RESOLVED
Added "Tunable defaults" note in the Deployment section: all numeric defaults are configurable via `agent.toml` or control plane config. Architecture specifies defaults to communicate intent, not to mandate fixed values.

---

## Missing Edge Cases

### 23. ~~Inode Collision~~ — RESOLVED
Added collision probability analysis: ~7×10⁻⁹ at 500K files in 2⁶⁴ space. No detection needed — if collision occurs, VFS returns first match (acceptable degradation).

### 24. ~~Block Map Interval Fragmentation~~ — RESOLVED
Added "Fragmentation guard": if interval count exceeds 1,000, the agent promotes to a full-file download and coalesces to a single interval. Caps blob size at ~16 KB.

### 25. ~~Plugin Job Queue Growth~~ — RESOLVED
Added queue size cap: 100,000 pending jobs per plugin. New events dropped when cap reached, with notification. Completed/failed jobs purged after 24 hours.

### 26. ~~PouchDB Browser Replica Size~~ — RESOLVED
Added "Browser replica size" paragraph: ~250 MB at 500K files, within typical 1-2 GB browser limit. Settings page displays replica size. Warning at 500 MB. No client-side purge in v1 due to PouchDB limitations.

### 27. ~~inotify Event Storms~~ — RESOLVED
Added "Event storm throttling": when events exceed 1,000/sec for 5 seconds, agent switches to full reconciliation crawl instead of processing individually. Resumes incremental watching after crawl completes.

### 28. ~~Filename Sanitization~~ — RESOLVED
Added to `name` field description: no Unicode normalization (preserves round-trip fidelity). Null bytes, forward slashes, and control characters (U+0000–U+001F) rejected by crawler. VFS presents names as-is; OS enforces its own rules.

---

## Unresolved Open Questions

### 29. ~~Seven Open Questions in Architecture Doc~~ — RESOLVED
All 7 questions resolved with formal decisions:
1. VFS/label event hooks — deferred to v2 (backwards-compatible addition)
2. Query result streaming — v1 uses gather-then-return (latency negligible at home scale)
3. Bridge storage Option A vs B — Option A default; plugin authors choose based on data characteristics; guidance added
4. Scheduled backups — deferred to v2 (users can script `curl` for v1)
5. Global plugin settings — deferred to v2 (ID scheme reserves room)
6. TCP plugins — deferred to v2 (protocol is transport-agnostic by design)
7. Push notifications — v1 uses pull; socket protocol accommodates future push extension

---

## Implementation Plan Issues

### 30. ~~Phase Ordering~~ — RESOLVED
Not actually a problem. Plugin backend (substeps 6.1–6.6) can be built in parallel with Phases 3–5, immediately after Phase 2. Only substep 6.7 (UI integration) requires Phase 5. Updated dependency note to recommend building plugin backend during Phases 3–4.

### 31. ~~Missing Testing Strategy~~ — RESOLVED
Added "Testing Strategy" section to implementation plan updates covering: unit tests (document round-trips, rule engine, HMAC), integration tests with Docker CouchDB (replication filters, backup/restore, plugin invocation, transfers), and Phase 11 performance benchmarks (500K files, readdir latency, replication sync).

### 32. ~~Missing Development Environment Setup~~ — RESOLVED
Added to testing strategy: `docker-compose.dev.yml`, local agent with test directory, `seed-test-data.sh` script, `--developer-mode` for database wipe. Added mock mode guidance for bridge plugins (`mock: true` config flag for synthetic files).

### 33. ~~Missing Migration Paths~~ — RESOLVED
Added "Migration Between Phases" section: schema is additive (new doc types, new optional fields, new indexes at startup). No migration scripts needed. Old-format document rewriting handled by startup migration functions if ever needed. `--developer-mode` wipe available as fallback.

---

## Summary

All 33 review items have been resolved. Changes were made to three files:

**`docs/architecture.md`** — 20+ additions and clarifications including:
- CouchDB `_replicator` document structures and replication health monitoring
- Agent-to-agent file transfer discovery sequence
- CouchDB document conflict resolution strategy (per-document-type)
- Mount conflict policy tie-breaking rule
- Plugin executable invocation contract (complete table)
- Secret redaction in backups
- `export_path` containment check in transfer server
- Plugin privilege model documentation
- Login rate limiting and credential enumeration prevention
- JWT signing key management
- HMAC clock skew handling
- Standardized retry parameters with per-context table
- CouchDB atomicity model
- API versioning policy
- Stale agent status detection
- Target scale definition ("home-deployment scale" table)
- Block map fragmentation guard
- Plugin job queue size cap
- Browser replica size monitoring
- Event storm throttling
- Filename sanitization rules
- All 7 open questions formally resolved

**`docs/MosaicFS-implementation-plan.md`** — document type count corrected (8 → 11)

**`docs/MosaicFS-implementation-plan-updates.md`** — document type count corrected (12 → 11), phase dependency clarified, testing strategy added, migration guidance added

**False positives identified:** Item 5 (circular mount detection) — not possible by design since mounts source from real filesystem paths, not virtual directories.
