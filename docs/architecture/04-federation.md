<\!-- MosaicFS Architecture · ../architecture.md -->

## Federation

Federation is the planned approach to multi-user support in MosaicFS. Rather than building per-user access control within a single instance — which requires rearchitecting the replication model, the rule engine, and every API endpoint — each user runs their own sovereign MosaicFS instance. Sharing between users is explicit and opt-in: an instance exposes a slice of its virtual namespace to a peer instance, and the peer mounts that slice into its own virtual tree.

This preserves the simplicity of the single-user security model within each instance while enabling sharing across instances. Federation is not implemented in v1, but the v1 design accommodates it with minimal forward-looking additions described below.

### The Sovereignty Model

A MosaicFS instance is fully self-contained. It controls its own files, its own rules, its own virtual namespace, and its own credentials. No external entity can read from or write to an instance without that instance explicitly agreeing to the relationship. A federated peer is not a participant in the local CouchDB replication topology — it is accessed only through the transfer API, at the discretion of the exporting instance.

This boundary means federation adds no new trust surface to an existing instance. An instance that has no peering agreements configured behaves identically to a v1 instance. Federation is purely additive.

### Export Modes

Three export modes are planned, forming a permission gradient from surgical to broad:

**Mode 1 — Virtual export rule.** The exporting instance creates a dedicated rule whose step pipeline filters exactly what should be shared with a named peer. The rule uses the same pipeline model as any other virtual mount rule — globs, age filters, MIME filters, and so on — but its `export` field identifies the peer instances it is visible to. This is the most precise sharing mechanism, appropriate for sharing a specific project folder with a collaborator.

**Mode 2 — Re-export of existing rule.** Rather than duplicating rule logic, an existing virtual mount rule is flagged for export by populating its `export.peer_ids` field. The rule's existing step pipeline determines what is visible; the export field makes that filtered view available to the named peers. Changes to the rule's steps are immediately reflected in what peers can see. This is appropriate when a rule already describes exactly the right set of files and writing a separate export rule would duplicate the logic.

**Mode 3 — Peering agreement.** A `peering_agreement` document establishes a broad sharing relationship between two instances. Rather than configuring exports per rule, the agreement defines what named exports or the entire virtual namespace are shared with the peer. This is appropriate for trusted peers — family members, a partner — where granular per-rule control is more friction than it is worth.

### How Federated Peers Map to Existing Concepts

From the receiving instance's perspective, a federated peer looks structurally similar to a source-mode storage backend: it is a remote source of file metadata and file bytes, accessed via an HTTP endpoint, with its files appearing under a subtree of the local virtual namespace. The key differences are that the peer is another MosaicFS instance rather than a cloud API, and the relationship is governed by a peering agreement rather than an OAuth token.

This maps onto existing concepts cleanly:

- A federated peer is represented as a `node` document — structurally similar to any other node, with federation-specific fields added in a future version.
- Files imported from a peer are represented as `file` documents with `source.node_id` pointing to the federated peer node. The VFS tiered access system gains a new tier — "fetch from peer instance via transfer API" — sitting between the control plane remote fetch and a future write tier.
- The virtual path prefix `/federation/` is reserved for imported peer namespaces. A peer named "alice" whose exported documents folder is mounted locally would appear at `/federation/alice/documents/`. This prefix is not used by local rules.
- The unified `export_path` field on rule sources works identically for local nodes and federated peers. A merge rule spanning both looks exactly like a merge rule spanning two local nodes:

```json
"sources": [
  { "node_id": "node-laptop",  "export_path": "/home/bob/documents" },
  { "node_id": "peer-alice",   "export_path": "/home/alice/documents" }
]
```

The rule engine resolves each source by asking the node what files live at that export path — physical agents answer with filesystem paths, federated peers answer with their virtual paths. The step pipeline then applies uniformly to all results.

### Cross-Instance Authentication

Authentication between instances uses instance-level keypairs rather than the shared credential model used within a single instance. Each MosaicFS instance generates an Ed25519 keypair at setup time. When two instances establish a peering agreement, they exchange public keys. Transfer requests from a peer instance are signed with the requesting instance's private key and verified against the stored public key — no per-user credentials are issued across the instance boundary.

This preserves the sovereignty model: instance A never issues credentials to instance B's users, and instance B never has direct access to instance A's CouchDB. The transfer endpoint on instance A simply validates that a request is signed by a known peer key and that the requested file is covered by an active peering agreement.

### Planned Document Types

Two new document types are planned for the federation implementation. They are not part of v1 but are designed here to ensure the v1 schema does not conflict with them.

**`peering_agreement`** — describes a bilateral sharing relationship. Lives on both instances. Contains the peer's instance ID, the peer's transfer endpoint, the peer's public key, what is shared (a list of export rule IDs, or `"all"` for the full virtual namespace), the direction of sharing (`"outbound"`, `"inbound"`, or `"bilateral"`), and the agreement status (`"pending"`, `"active"`, or `"suspended"`). An agreement begins in `"pending"` state and becomes `"active"` only when both instances have confirmed it — preventing one-sided peering.

**`federated_import`** — describes how an imported peer namespace is mounted into the local virtual tree. Lives on the receiving instance only. Contains the peer's instance ID, which of their exports to mount, the local virtual path prefix to mount it under, and a polling interval for metadata refresh. The VFS layer and rule engine treat an active `federated_import` as a read-only subtree source, similar to how embedded `network_mounts` entries on a node document drive tiered access.

### v1 Accommodations

The federation implementation itself is deferred, but three small additions to the v1 design ensure future compatibility without adding implementation complexity:

**`export` field on virtual directory mounts.** An optional `export` object is included in each mount entry schema. In v1 the rule engine ignores it entirely. Users who want to flag specific mounts for future export can populate it without any schema migration when federation ships.

```json
"export": {
  "enabled": false,
  "peer_ids": []
}
```

**Federation node support reserved.** The node document schema accommodates future federation by allowing additional fields for federated peers. v1 components that encounter unknown node fields should ignore them rather than erroring. This allows federation-capable agents to be deployed alongside v1 agents without breaking the existing system.

**`/federation/` virtual path prefix reserved.** Local directories must not use `/federation/` as a path prefix. This prefix is reserved for imported peer namespaces. The Virtual Filesystem editor in the web UI will warn if a user attempts to create a directory at this path. A `mirror_source` mount strategy — where a federated peer's `export_path` is used verbatim as the local virtual path — is planned as an additional strategy alongside `prefix_replace` and `flatten`. Because a federated peer's `export_path` is already a virtual path, mirroring it locally requires no transformation.

---

