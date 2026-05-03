# Feasibility Analysis: Replacing CouchDB with SQLite + Custom Sync

## Context

This is a feasibility-stage analysis, not an implementation plan. The user is
evaluating whether replacing CouchDB with SQLite + a hand-rolled peer-to-peer
sync protocol is a viable direction before committing to detailed design. The
discussion document is `docs/changes/016/discussion.md`. The relevant project
constraint — "CouchDB stays" in `.claude/skills/decisions/SKILL.md` — has been
explicitly opened up for revision by the user.

The question this document answers: **what would we have to build ourselves
to live without CouchDB, and is the scope tolerable for a single-developer
hobby project?**

## Summary judgment

Feasible, but the cost is concentrated in two places that are easy to
underestimate: (1) the sync protocol's failure-mode surface area and (2) the
peer-pairing/auth UX. Both are tractable. SQLite as the local store is a
straightforward win; the custom federation layer is the real work.

The "boring technology" principle does not actually favor CouchDB here —
SQLite is as boring as it gets. The relevant trade is: CouchDB gives us a
*replication protocol* for free, and we'd be writing one. That's the lever
to weigh, not the storage engine.

## Scope of net-new code

What CouchDB provides today that we'd need to replace:

| CouchDB capability | Replacement | Effort |
|---|---|---|
| Document storage + MVCC | SQLite tables; we add versioning only where needed (config doc) | Low |
| `_changes` feed | Intent log table, append-only, with per-node sequence numbers | Low–Mod |
| Multi-master replication | Custom HTTP sync endpoints + client; high-water exchange + log shipping | **High** |
| Conflict resolution | Not needed for file index (node-sharded); field-level LWW for config | Mod |
| HTTP auth | Mutual-TLS with TOFU peer pairing (see security section) | Mod |
| Compaction | Per-node intent-log compaction by age/size | Low |
| DB admin UI | `sqlite3` CLI; deferred web admin | None (deferred) |

Estimated net-new Rust LOC: low thousands for the core, plus a sync
test harness that is likely as large as the protocol itself.

### The genuinely hard correctness concerns

These are the things that will produce subtle, hard-to-reproduce bugs if
gotten wrong:

1. **Atomicity of derived-table write + intent-log append.** Both must
   commit in the same SQLite transaction or the log diverges from state.
   SQLite handles this trivially as long as the writer task wraps both in
   one `BEGIN`. Verifiable with property tests.

2. **Snapshot consistency for full sync.** The snapshot must capture the
   file index *and* the per-node sequence numbers at the same logical
   instant, so the receiver knows where to resume. WAL-mode snapshot
   isolation (`BEGIN DEFERRED` + read both in one transaction) covers this.

3. **Compaction during in-flight snapshot.** Anchor the snapshot to a
   specific sequence number captured at snapshot time. Receiver picks up
   from that anchor regardless of what the source compacts afterward.
   Materialize the snapshot to a temp file before streaming so compaction
   isn't blocked on a slow network peer.

4. **Crash mid-replay on receiver.** Apply each batch of intent-log
   entries plus its high-water bump in one transaction. On restart, the
   receiver either has the batch fully applied or not at all, and replays
   from the last persisted high-water.

5. **Idempotency on flaky transport.** Per-origin sequence numbers make
   replay naturally idempotent; receiver discards entries it has already
   applied.

The *engineering* on each of these is well-understood. The risk is
discovering which of them you forgot when a user reports a desync six
months in. This is why the test harness is as important as the protocol.

## Security story for v1

A reasonable, conservative v1 design:

- **Per-node Ed25519 keypair**, generated on first run. Stored in the
  SQLite DB (or macOS Keychain where available; file with 0600 on Linux).
- **TOFU pairing** with out-of-band verification: when adding a peer, the
  current node displays a short fingerprint derived from its public key.
  The new peer enters or scans it (or vice versa). Each side persists the
  other's public key after pairing. Same trust model as SSH host keys,
  but mutual.
- **Transport: HTTPS with rustls** and a custom certificate verifier that
  checks against the stored peer-pubkey set. Self-signed certs derived
  from the node keypair. No public CA dependency.
- **Authorization granularity: cluster-wide.** Any paired peer can pull
  anything. No per-resource ACLs in v1. The trust unit is "this is one of
  my devices."
- **Listen scope:** bind configurable, default LAN-only. Document that
  remote access should ride a VPN (Tailscale, WireGuard) — no
  NAT-traversal in v1.
- **Replay protection** comes from TLS; no extra layer needed.

Hard part isn't the crypto — it's the pairing UX. The init screen needs
to handle both "I'm the first node" and "I'm joining; here's the
fingerprint I just saw on my other device." Worth prototyping the
pairing flow on paper before committing.

This is materially less capable than CouchDB's auth model (no roles, no
per-doc validation) but appropriate for personal-cluster scope.

## Concurrent config edits

The concern: edit settings on laptop while NAS offline, edit something
on NAS in the same window, naive LWW silently drops one of them.

Options considered:

- **Field-level LWW + audit log + drop notification (recommended for v1).**
  Every config change is appended to a changes table with timestamp,
  source node, before/after. On merge, LWW per field, but the loser is
  preserved in the log. UI surfaces a "config merge dropped N fields"
  banner so the user can inspect and re-apply. Cheap to build; fails
  safely; reversible.
- **Single config-master node.** Other nodes proxy edits via API. Simple
  but bad UX when the master is offline.
- **Per-key vector clocks with explicit conflict UI.** Fancier; the user
  picks on conflict. Probably overkill for v1; the surface area of
  config likely to be edited concurrently is small.

Recommendation: option 1. Honest about the trade, recoverable in practice.

## redb's intended role

Per the user's clarification, redb was proposed as a *local cache for
offline operation* — when the laptop is disconnected from the NAS where
CouchDB runs. In the SQLite design, the local SQLite DB *is* the
authoritative store, and offline operation is the default mode. redb is
genuinely redundant in that world and can be dropped from the roadmap.

## What's still open before any implementation plan

1. **Confirm decision to replace CouchDB.** Update
   `.claude/skills/decisions/SKILL.md` to remove "CouchDB stays" and
   capture the new direction with rationale.
2. **Sync protocol design doc** as a separate change directory with full
   detail on wire format, failure modes, and test plan.
3. **Pairing UX prototype** before locking the auth model in.
4. **Migration story for existing CouchDB dev data** — likely a one-shot
   tool, not a long-lived compatibility layer.
5. **Scoping into multiple change directories** per the project's
   "one moving part at a time" rule. (User has set this aside for the
   feasibility discussion but it returns once we move to design.)

## Files and references reviewed

- `docs/changes/016/discussion.md` — the proposal under review
- `.claude/skills/decisions/SKILL.md` — settled-decisions doc that needs
  updating if we proceed
- prior context including the change 014 machine_id collision (relevant to node
  identity choices in any sync protocol)
