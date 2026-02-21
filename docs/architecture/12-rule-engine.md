<\!-- MosaicFS Architecture · ../architecture.md -->

## Rule Evaluation Engine

### Evaluation Model

Rules are embedded in virtual directory documents as `mounts` arrays rather than existing as separate documents. Evaluation is on-demand: when the VFS layer receives a `readdir` call for a virtual directory, the rule engine evaluates that directory's mount sources at that moment against the file documents in the local CouchDB replica. There is no background worker and no pre-computed `virtual_path` on file documents — a file's virtual location is derived when someone asks for it.

This model has several attractive properties:

- **Changes take effect immediately.** Editing a directory's mounts is reflected on the next `readdir` — no background recomputation, no propagation delay.
- **Lazy evaluation.** A directory that is never accessed never evaluates its mounts. Deep trees with many directories are traversed only as the user navigates into them.
- **Multiple appearances are natural.** The same file can satisfy the mount criteria of many directories simultaneously. The engine evaluates each directory independently; there is no global deduplication or single-winner policy across the tree.
- **No orphaned virtual paths.** Because virtual paths are never stored, there is nothing to go stale when mounts change or files are deleted.

### Readdir Evaluation

When `readdir` is called for a virtual directory:

```
readdir(virtual_path)
  → load virtual_directory document from local replica
  → collect ancestor step chain (root → ... → parent)
      for each ancestor with enforce_steps_on_children: true
          prepend its mount steps to the inherited chain
  → for each mount in directory.mounts:
      query file documents matching source (node_id, export_path prefix)
      for each candidate file:
          if status != "active": skip
          resolve effective_labels(file)              ← O(1) lookup in materialized label cache
          resolve annotations(file)                   ← lazy: load annotation docs only for plugin_names referenced in steps
          run inherited step chain → if excluded: skip
          run mount's own steps  → if excluded: skip
          apply mapping strategy to derive filename within this directory
          if name collision: apply conflict_policy
          emit (filename, file_doc)
  → also include child virtual_directory documents as subdirectory entries
  → return combined listing
```

The query for candidate files uses the `(type, source.node_id, source.export_parent)` index — fetching all files whose `export_parent` starts with the mount's `source.export_path`. This is an indexed prefix scan, not a full collection scan.

### Step Pipeline

Each mount's `steps` array is an ordered list of filter steps evaluated in sequence after any inherited ancestor steps. The ten supported ops are:

- **`glob`** — matches against the file's `export_path` using a glob pattern. Supports wildcards (`*`, `**`, `?`).
- **`regex`** — matches against the file's `export_path` using a regular expression. Supports a `flags` field (e.g. `"i"` for case-insensitive).
- **`age`** — compares `mtime`. `max_days` requires the file to be newer than N days; `min_days` requires it to be older. Both may be specified for a date range.
- **`size`** — compares file size in bytes. Accepts `min_bytes`, `max_bytes`, or both.
- **`mime`** — matches against `mime_type`. Accepts an array of type strings with wildcard support (e.g. `"image/*"`).
- **`node`** — matches files originating from a specific set of nodes by `node_ids` array.
- **`label`** — matches against the file's effective label set. Accepts a `labels` array; the step matches if the file's effective label set contains **all** of the specified labels (AND semantics). To implement OR semantics, use multiple label steps with `on_match: "include"`.
- **`replicated`** — matches against a file's replica status. Requires `target_name` (the replication target to check). Optionally accepts `status` (default `"current"`) to match only replicas with a specific status. The step matches if a `replica` document exists for the file on the named target with the specified status. If no replica document exists, the step does not match. Example: `{ "op": "replicated", "target_name": "offsite-backup", "on_match": "exclude" }` excludes files already backed up, while `{ "op": "replicated", "target_name": "offsite-backup", "invert": true, "on_match": "include" }` includes only files that are NOT yet replicated.
- **`access_age`** — compares a file's last access time (from the materialized access cache) against the current time. `max_days` requires the file to have been accessed within N days; `min_days` requires it to have been accessed more than N days ago. Both may be specified for a date range. Accepts a `missing` field (`"include"` or `"exclude"`, default `"include"`) that controls behavior when no access document exists for the file — i.e. the file has never been accessed through MosaicFS. `"include"` treats unaccessed files as matching (useful for archival rules that target old/unused files); `"exclude"` skips them.
- **`annotation`** — matches against a value in a file's annotation data. Requires `plugin_name` (identifies which annotation document to inspect) and `key` (a dot-notation path into the `data` object, e.g. `"language"` or `"tags.primary"`). Optionally accepts `value` for exact match or `regex` for pattern match against a string value. If only `plugin_name` and `key` are provided, the step matches if the key exists with any non-null value. If the annotation document does not exist (the plugin has not yet processed the file), the step does not match.

Step evaluation logic:

```
for each step in [inherited_steps..., mount.steps...]:
    raw_match   = evaluate(step.op, file)
    final_match = step.invert ? !raw_match : raw_match
    if final_match:
        if step.on_match == "continue": proceed to next step
        if step.on_match == "include":  file → included (stop)
        if step.on_match == "exclude":  file → excluded (stop)
    else:
        proceed to next step

file → mount.default_result
```

A non-match always continues to the next step. Unknown `op` values are treated as a non-match and continue, allowing future op types to be introduced without breaking existing rule engine versions.

### Inheritance

When an ancestor directory has `enforce_steps_on_children: true`, its mount steps are prepended to every mount evaluation in all descendant directories — outermost ancestor first, nearest parent last. A child's own mount steps are appended last. Ancestor steps always evaluate before descendant steps. A child can narrow an ancestor's results further (by adding more restrictive steps) but cannot widen them (cannot surface files the ancestor excluded).

### Mapping Strategies

- **`prefix_replace`** — strip `source_prefix` from `export_path` and mount the remaining path hierarchy as a subtree within this directory. The most common case.
- **`flatten`** — place all matching files directly in this directory, discarding their subdirectory structure. All matching files appear as immediate children regardless of nesting depth.

### Conflict Resolution

When two mount sources within the same directory produce a file with the same name, the `conflict_policy` on the originating mount determines the outcome. `"last_write_wins"` keeps the file with the most recent `mtime`. `"suffix_node_id"` appends the node ID to the losing file's name, making both visible.

When the colliding files come from mounts with *different* conflict policies, the more conservative policy wins: if either mount specifies `"suffix_node_id"`, both files are made visible with suffixed names. `"last_write_wins"` only applies when both mounts agree on it. This prevents a mount with `"last_write_wins"` from silently hiding a file that another mount intended to keep visible.

### Readdir Cache

The VFS layer caches the output of `readdir` for each virtual directory with a short TTL (default 5 seconds). This prevents re-evaluating mount sources on every `lookup` within a directory during rapid directory traversal. The cache is invalidated when the directory document changes via the PouchDB live changes feed — so edits to a directory's mounts take effect within one TTL window on agents that have already cached the listing, and immediately on the next `readdir` on agents that have not.

---

