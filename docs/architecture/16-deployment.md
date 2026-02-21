<\!-- MosaicFS Architecture · ../architecture.md -->

## Deployment

**Tunable defaults.** Numeric defaults mentioned throughout this document (readdir cache TTL, cache size cap, health check interval, retry parameters, etc.) are initial values chosen for typical home deployments. All are configurable via `agent.toml` or the control plane's configuration file. The architecture document specifies defaults to communicate intent and typical operating parameters, not to mandate fixed values. Implementers should expose these as configuration options with the documented defaults.

### Control Plane

The control plane runs as a Docker Compose stack. CouchDB is bound to localhost only and is not directly reachable from outside the host. The Axum API server is the only externally-accessible process — it serves the REST API, proxies the CouchDB replication endpoint for authenticated agent connections, issues PouchDB session tokens for browser clients, and terminates TLS. On first start, the Compose stack initialises CouchDB with an admin credential and creates the `mosaicfs_browser` CouchDB role with read-only access to the `mosaicfs` database. TLS is enabled by default using an automatically generated self-signed CA and server certificate.

**Developer mode:** The control plane binary accepts a `--developer-mode` flag. When enabled, the `DELETE /api/system/data` endpoint becomes accessible, allowing complete database wipes without destroying the Docker Compose stack. This is intended for development and testing workflows where quickly cycling between backup/restore states is useful. Developer mode should never be enabled in production — a production database wipe should be done by destroying and recreating the Compose stack, not via an API endpoint. The flag is disabled by default.

### Agents

Agents are distributed as single static binaries. The `MosaicFS-agent init` command configures the agent, registers it with the control plane, installs the systemd unit (or launchd plist on macOS), and starts the service. The secret key is never passed as a CLI argument; it is read from stdin with echo disabled via the `rpassword` crate, or from the `MOSAICFS_SECRET_KEY` environment variable for scripted deployments.

### State Directory

```
/var/lib/mosaicfs/
  agent.toml          # configuration
  node_id             # persistent node identity
  pouchdb/            # local database
  cache/              # VFS file cache
    a3/               # sharded by first 2 chars of path-key hash
    tmp/              # in-progress full-file downloads
    index.db          # SQLite: file_uuid, file_id, mtime, size, block_map, last_access
  plugin_jobs.db      # SQLite: plugin event job queue and ack tracking
  replication/        # replication subsystem state
    replication.db    # SQLite: replication work queue, deletion log
  certs/
    ca.crt            # control plane CA certificate
  storage-backends/   # storage backend credentials (hosting agent only)
    google_drive/
      credentials.enc

/run/mosaicfs/plugin-sockets/   # Unix domain sockets for socket plugins
  ai-summariser.sock            # bound by the plugin process, connected to by the agent
  fulltext-search.sock

/usr/lib/mosaicfs/plugins/      # plugin executables (Linux path)
  ai-summariser                 # executable or script
  exif-extractor
```

---

