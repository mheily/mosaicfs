<\!-- MosaicFS Architecture · ../architecture.md -->

## Agent Crawl and Watch Strategy

### Initial and Periodic Crawl

The agent walks all configured watch paths and emits `file` documents. Paths listed in `excluded_paths` in `agent.toml` are skipped during the walk — this prevents the crawler from indexing replication storage directories or other directories that should not be part of the MosaicFS file index. For each file, it stats the path and checks whether `(export_path, size, mtime)` matches the existing document — if so, the file is unchanged and the document is skipped. Changed and new files are written in batches of up to 200 documents using CouchDB's `_bulk_docs` endpoint. No content hashing is performed during the crawl — change detection relies entirely on `size` and `mtime`. Full crawls are scheduled nightly as a consistency safety net.

### Incremental Watching

After the initial crawl, the agent uses the OS native filesystem event API (`inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows) via the Rust `notify` crate. Events are debounced over a 500ms window per path to handle noisy editor saves. Rename events are correlated into a single path-update operation rather than a delete-and-create pair.

**Event storm throttling.** When the filesystem event rate exceeds 1,000 events per second sustained over 5 seconds, the agent switches from incremental watching to a full reconciliation crawl. This handles bulk operations like extracting a large archive or running `git checkout` across thousands of files — processing each event individually would be slower than a single crawl pass. The agent logs the transition, completes the crawl, and resumes incremental watching. Plugin job enqueuing is batched during the crawl to avoid flooding the plugin queue.

### inotify Watch Limit

Linux defaults to 8,192 inotify watches. Installations with large directory trees should raise this limit. The agent installation process sets `fs.inotify.max_user_watches = 524288` via `/etc/sysctl.d/`. Directories that cannot be watched due to limit exhaustion fall back to coverage by the nightly full crawl, and their paths are recorded in the agent's `watch_state` document.

### Reconciliation After Reconnect

When an agent reconnects after being offline, it runs an expedited full crawl of all watched paths before resuming normal watch mode. The mtime/size fast-path makes this much faster than a cold crawl. Reconnection is detected by monitoring the CouchDB replication connection state.

### Agent Main Loop

```
startup
  → load config
  → connect to local CouchDB
  → start CouchDB replication (bidirectional, continuous)
  → build materialized label cache from local CouchDB replica
  → watch changes feed for label_assignment / label_rule / file changes → update label cache incrementally
  → if first run:  full crawl → write notification (first_crawl_complete, info) on completion
                               then start watcher
  → if resuming:   reconciliation crawl → then start watcher
  → start transfer HTTP server
  → start heartbeat (update node document every 30s)
  → enumerate plugin directory → report available_plugins in agent_status
  → load plugin configurations from local CouchDB (type=plugin, node_id=this_node)
  → start plugin runner:
      for each enabled executable plugin: start worker pool (N workers)
      for each enabled socket plugin: connect to /run/mosaicfs/plugin-sockets/<n>.sock
          on connect: replay unacknowledged events from SQLite queue
                      resolve plugin_disconnected notification for this plugin
          on disconnect: write plugin_disconnected notification
                         retry with exponential backoff
  → watch changes feed for plugin document updates → reload plugin config live
  → run watcher event loop
      on file.added / file.modified: enqueue plugin jobs for subscribed plugins
      on file.deleted: delete annotation documents for this file, notify subscribed plugins
      on watch path becoming inaccessible: write watch_path_inaccessible notification
      on watch path restored: resolve watch_path_inaccessible notification
  → on schedule (nightly):    run full crawl as consistency check
  → on schedule (hourly):     collect storage inventory, write utilization_snapshot
                              check inotify watch count → write/resolve inotify_limit_approaching
                              check cache utilization → write/resolve cache_near_capacity
  → on schedule (per plugin health_check_interval_s):
                              send health_check to each enabled socket plugin
                              process notifications[] → write/update notification documents
                              process resolve_notifications[] → mark notification documents resolved
                              after 3 missed responses: write plugin_health_check_failed notification
  → on sync request (manual or API): run full sync (see Plugin Full Sync below)
```

---

