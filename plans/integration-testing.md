# Integration Testing Plan

## Problem Statement

MosaicFS has good unit test coverage (document round-trips, HMAC signatures, credential hashing) and frontend E2E tests (Playwright), but no automated integration tests that exercise the real component interactions: server bootstrap, agent registration, filesystem crawling, CouchDB replication, VFS resolution, and file serving. The bootstrap step is manual — you must copy-paste credentials from server log output — which blocks full automation.

## Goals

1. Fully automated test suite: zero manual steps from `cargo test` to green.
2. Cover the core distributed interactions: agent ↔ control plane, agent ↔ agent, client ↔ control plane.
3. Simple infrastructure: single `docker-compose` stack, or all-in-one container with processes.
4. Fast enough to run in CI on every push.

---

## Bootstrap Automation

The current bootstrap flow prints credentials to stdout and expects a human to copy them into `agent.toml`. Integration tests need credentials programmatically.

### Approach: `bootstrap --json` flag

Add a `--json` flag to the `mosaicfs-server bootstrap` subcommand. When present, the command outputs a single JSON object to stdout instead of human-readable text:

```json
{"access_key_id": "MOSAICFS_A1B2C3...", "secret_key": "mosaicfs_xyz..."}
```

Implementation (in `mosaicfs-server/src/main.rs`, the existing bootstrap block):

```rust
let json_output = std::env::args().any(|a| a == "--json");

// ... existing credential creation ...

if json_output {
    println!("{}", serde_json::json!({
        "access_key_id": access_key,
        "secret_key": secret_key,
    }));
} else {
    println!("Access Key: {}", access_key);
    println!("Secret Key: {}", secret_key);
}
```

Test harness reads stdout, parses JSON, and injects credentials into agent config. This is a minimal, backwards-compatible change — existing users see no difference without `--json`.

### Idempotent bootstrap for tests

Currently, `bootstrap` exits with code 1 if credentials already exist. For tests that may restart the server container, add a `--if-empty` flag (or combine: `bootstrap --json --if-empty`) that silently succeeds and prints the existing credential's access_key_id (but NOT the secret, since that's hashed). For integration tests, the simpler pattern is:

1. Start with a fresh CouchDB database each test run (delete + recreate).
2. Call `bootstrap --json` once at the start of the suite.
3. Share the returned credentials across all tests in that run.

---

## Infrastructure: Single Compose Stack

### Why compose over all-in-one container

- CouchDB requires its own Erlang runtime and data directory — running it as a subprocess inside a test container is fragile and non-standard.
- Compose isolates failure domains: a crashed agent doesn't take down CouchDB.
- The production deployment will use separate containers, so tests should mirror that.
- Compose is already the dev workflow; tests reuse the same model.

### `tests/docker-compose.integration.yml`

A dedicated compose file for integration tests, separate from the dev compose:

```yaml
version: "3.9"

services:
  couchdb:
    image: docker.io/couchdb:3
    environment:
      COUCHDB_USER: admin
      COUCHDB_PASSWORD: testpassword
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:5984/_up"]
      interval: 2s
      timeout: 2s
      retries: 15

  server:
    image: localhost/mosaicfs-dev:latest
    volumes:
      - ../target:/workspace/target:ro
      - server-data:/data/mosaicfs
    environment:
      COUCHDB_URL: http://couchdb:5984
      COUCHDB_USER: admin
      COUCHDB_PASSWORD: testpassword
      MOSAICFS_PORT: "8443"
      MOSAICFS_DATA_DIR: /data/mosaicfs
      RUST_LOG: info
    depends_on:
      couchdb:
        condition: service_healthy
    command: /workspace/target/debug/mosaicfs-server --developer-mode

  agent-1:
    image: localhost/mosaicfs-dev:latest
    volumes:
      - ../target:/workspace/target:ro
      - agent1-data:/data
      - agent1-files:/test-files
    environment:
      COUCHDB_URL: http://couchdb:5984
      COUCHDB_USER: admin
      COUCHDB_PASSWORD: testpassword
      MOSAICFS_STATE_DIR: /data/state
      RUST_LOG: info
    depends_on:
      couchdb:
        condition: service_healthy
    # Started by the test harness after bootstrap injects agent.toml
    command: sleep infinity

  agent-2:
    image: localhost/mosaicfs-dev:latest
    volumes:
      - ../target:/workspace/target:ro
      - agent2-data:/data
      - agent2-files:/test-files
    environment:
      COUCHDB_URL: http://couchdb:5984
      COUCHDB_USER: admin
      COUCHDB_PASSWORD: testpassword
      MOSAICFS_STATE_DIR: /data/state
      RUST_LOG: info
    depends_on:
      couchdb:
        condition: service_healthy
    command: sleep infinity

volumes:
  server-data:
  agent1-data:
  agent1-files:
  agent2-data:
  agent2-files:
```

Key points:
- Two agent containers enable agent-to-agent replication tests.
- Agent containers start with `sleep infinity`; the test harness writes `agent.toml` and then exec's the agent binary, giving full control over timing.
- Server runs with `--developer-mode` so tests can wipe the database between suites via `DELETE /api/system/data`.
- Volumes are named (not host-mounted) so tests can seed files via `docker cp` or exec.

---

## Test Harness

### Language: Shell + Rust integration tests

The test harness is a shell script (`tests/integration/run.sh`) that:

1. Builds the project (`cargo build`).
2. Starts the compose stack (`podman-compose -f tests/docker-compose.integration.yml up -d`).
3. Waits for CouchDB and server health.
4. Runs `bootstrap --json` inside the server container, captures credentials.
5. Writes `agent.toml` files into agent containers.
6. Starts agent processes inside agent containers.
7. Runs the actual test assertions (see below).
8. Tears down the stack.

### Test runner: shell functions with `curl` + `jq`

Integration tests are shell functions in `tests/integration/test_*.sh` files, sourced by the main runner. Each test function:
- Has a descriptive name (e.g., `test_agent_registers_and_heartbeats`).
- Uses `curl` against the server API (HTTPS with `-k` for self-signed certs).
- Uses `jq` to parse JSON responses and assert values.
- Can exec commands inside containers (`podman-compose exec`).
- Can query CouchDB directly for verification.

Helper functions:

```bash
# Authenticate and get JWT token
api_login() {
  curl -sk "https://server:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"$ACCESS_KEY_ID\",\"secret_key\":\"$SECRET_KEY\"}" \
    | jq -r '.token'
}

# Make authenticated API call
api_get() {
  curl -sk -H "Authorization: Bearer $TOKEN" "https://server:8443$1"
}

api_post() {
  curl -sk -X POST -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "$2" "https://server:8443$1"
}

# Wait for a condition with timeout
wait_for() {
  local desc="$1" cmd="$2" timeout="${3:-30}"
  for i in $(seq 1 "$timeout"); do
    if eval "$cmd" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  echo "TIMEOUT: $desc" >&2; return 1
}

# Assert equality
assert_eq() {
  if [ "$1" != "$2" ]; then
    echo "FAIL: expected '$2', got '$1'" >&2; return 1
  fi
}

# Wipe database between test suites (requires developer mode)
wipe_db() {
  api_post "/api/system/data" '{"confirm":"DELETE_ALL_DATA"}' -X DELETE
}
```

### Why shell over a Rust test binary

- Simpler to orchestrate containers (`docker exec`, `docker cp`).
- No compilation step for test code — faster iteration.
- `curl` + `jq` is the natural language for HTTP API testing.
- No dependency on Rust test framework quirks (parallel execution, test ordering).
- Easy to run individual tests during development.

An alternative is a Rust integration test binary (`tests/integration.rs`) that uses `reqwest` and `tokio::process::Command` to drive containers. This is viable but adds compile time for the test itself. Shell is recommended for v1; Rust test binary can be added later if shell becomes unwieldy.

---

## Test Suites

### Suite 1: Bootstrap and Authentication

**Purpose:** Verify the credential lifecycle and authentication mechanisms.

Tests:
1. **`test_bootstrap_creates_credential`** — Run `bootstrap --json`, verify JSON output has `access_key_id` and `secret_key` fields. Query CouchDB directly to confirm the credential document exists.
2. **`test_bootstrap_rejects_when_credentials_exist`** — Run `bootstrap --json` a second time, verify it exits with code 1.
3. **`test_jwt_login`** — POST to `/api/auth/login` with bootstrap credentials, verify a JWT token is returned. Call `/api/auth/whoami` with the token, verify it returns the credential name.
4. **`test_jwt_login_bad_password`** — POST with wrong secret_key, verify 401.
5. **`test_credential_crud`** — Create a new credential via `POST /api/credentials`, list credentials, disable it, verify login fails, re-enable, verify login works, delete it.

### Suite 2: Agent Registration and Heartbeat

**Purpose:** Verify agent-to-control-plane lifecycle.

Tests:
1. **`test_agent_registers_node`** — Start agent-1, wait for it to appear in `GET /api/nodes`. Verify node document has expected fields (friendly_name, platform, status: online).
2. **`test_agent_heartbeat`** — After agent-1 is running, verify `last_heartbeat` in the node document updates over time (check twice with a 35-second gap).
3. **`test_agent_goes_offline`** — Stop agent-1 process, verify the node status transitions to "offline" or last_heartbeat goes stale.
4. **`test_two_agents_register`** — Start both agents, verify two distinct nodes appear in the node list.

### Suite 3: Filesystem Crawling

**Purpose:** Verify that files on agent nodes are indexed and visible via the API.

Setup: Seed test files into agent-1's `/test-files` directory before starting the agent.

```bash
podman-compose exec agent-1 bash -c '
  mkdir -p /test-files/docs /test-files/photos
  echo "hello world" > /test-files/docs/readme.txt
  dd if=/dev/urandom of=/test-files/photos/image.bin bs=1024 count=100
  echo "report data" > /test-files/docs/report.csv
'
```

Tests:
1. **`test_files_indexed`** — After agent crawl completes, `GET /api/files` returns at least 3 files. Verify fields: `filename`, `size`, `node_id`.
2. **`test_file_by_path`** — `GET /api/files/by-path?path=/test-files/docs/readme.txt&node_id=...` returns the correct file document with matching size (12 bytes).
3. **`test_file_content_download`** — `GET /api/files/{file_id}/content` returns the file bytes. Verify SHA-256 digest matches.
4. **`test_file_content_range_request`** — `GET /api/files/{file_id}/content` with `Range: bytes=0-4` returns "hello" (5 bytes, status 206).
5. **`test_new_file_detected`** — Create a new file in agent-1's `/test-files`, trigger a re-crawl (or wait for periodic crawl), verify the new file appears in the API.
6. **`test_deleted_file_soft_deleted`** — Delete a file from `/test-files`, wait for crawl, verify the file document has `status: "deleted"` but still exists in CouchDB.

### Suite 4: Labels and Rules

**Purpose:** Verify the labeling system.

Tests:
1. **`test_assign_label`** — `POST /api/labels/assignments` to assign label "important" to a file. `GET /api/labels/assignments/{file_id}` returns it.
2. **`test_label_rule_auto_applies`** — Create a label rule: path prefix `/test-files/docs/` → label "document". Verify that files under that path have the effective label via `GET /api/files/{id}/labels`.
3. **`test_search_by_label`** — After labeling, `GET /api/search?label=important` returns the labeled file.

### Suite 5: Virtual Filesystem

**Purpose:** Verify VFS directory tree and mount resolution.

Tests:
1. **`test_root_directory_exists`** — `GET /api/vfs/tree` returns a tree with at least a root directory.
2. **`test_create_virtual_directory`** — `POST /api/vfs/directories` to create `/projects`. Verify it appears in the tree.
3. **`test_mount_with_steps`** — Create a mount under `/projects` that selects files from agent-1 with a path-prefix step. `GET /api/vfs/directories/projects` returns the matching files.
4. **`test_mount_with_label_filter`** — Mount that filters by label. Assign a label, verify only labeled files appear in the mount.
5. **`test_nested_directories`** — Create `/projects/alpha` and `/projects/beta` with different mounts. Verify each directory resolves independently.

### Suite 6: Replication

**Purpose:** Verify file replication between agents and to storage backends.

Tests:
1. **`test_create_storage_backend`** — Create a "directory" storage backend via `POST /api/replication/backends`. Verify it appears in the list.
2. **`test_create_replication_rule`** — Create a rule: all files from agent-1 → directory backend. Verify the rule document.
3. **`test_replication_executes`** — After rule creation, wait for agent-1 to replicate files. Verify `replica` documents appear in CouchDB with `status: "complete"` and correct checksums.
4. **`test_agent_to_agent_replication`** — Create a storage backend of type "agent" pointing to agent-2. Create a rule. Verify files are transferred and replica documents are created.
5. **`test_replication_rule_with_steps`** — Rule that only replicates files matching a path prefix. Verify only matching files get replicated.
6. **`test_deletion_propagates`** — Delete a file on agent-1, verify the replica is cleaned up (or marked deleted) according to the rule's deletion policy.

### Suite 7: Notifications

**Purpose:** Verify the notification system end-to-end.

Tests:
1. **`test_initial_crawl_notification`** — After agent starts and crawls, verify a `first_crawl_complete` notification exists via `GET /api/notifications`.
2. **`test_notification_acknowledge`** — Acknowledge a notification, verify its status changes.
3. **`test_notification_deduplication`** — Trigger the same condition twice, verify only one notification exists with an incremented count (not two separate notifications).

### Suite 8: Events and Real-time Sync

**Purpose:** Verify the CouchDB changes feed and PouchDB sync.

Tests:
1. **`test_changes_feed_delivers_updates`** — Create a file via agent crawl, poll CouchDB `_changes` feed, verify the file document appears.
2. **`test_label_cache_updates`** — Assign a label, verify effective labels update without server restart (changes feed → cache invalidation).
3. **`test_readdir_cache_invalidation`** — Modify a virtual directory, verify subsequent readdir calls reflect the change.

### Suite 9: Backup and Restore (after Phase 8)

**Purpose:** Verify backup/restore round-trip.

Tests:
1. **`test_minimal_backup`** — `GET /api/system/backup?type=minimal` returns a JSON file with virtual directories, credentials, replication rules.
2. **`test_full_backup`** — `GET /api/system/backup?type=full` includes file documents.
3. **`test_restore_into_empty_db`** — Wipe database (developer mode), POST backup to `/api/system/restore`, verify documents reappear.
4. **`test_restore_rejected_on_non_empty_db`** — POST restore without wiping, verify 409.

### Suite 10: Error Handling and Edge Cases

Tests:
1. **`test_expired_jwt_rejected`** — Use a manually crafted expired JWT, verify 401.
2. **`test_hmac_replay_rejected`** — Send an HMAC-signed request with a timestamp >5 minutes old, verify rejection.
3. **`test_disabled_credential_rejected`** — Disable a credential, verify agent auth fails.
4. **`test_large_batch_crawl`** — Seed 10,000 small files, verify all are indexed without error. Check bulk_docs batching works (200-doc chunks).
5. **`test_concurrent_agent_registration`** — Start both agents simultaneously, verify no CouchDB conflicts on node documents.

---

## Test Lifecycle

```
┌──────────────────────────────────────────────────────┐
│  tests/integration/run.sh                            │
│                                                      │
│  1. cargo build                                      │
│  2. podman-compose up -d (couchdb, server)           │
│  3. wait for health checks                           │
│  4. bootstrap --json → capture credentials           │
│  5. FOR each suite:                                  │
│     a. wipe DB (DELETE /api/system/data)             │
│     b. re-bootstrap (fresh credentials)              │
│     c. seed test data into agent containers          │
│     d. write agent.toml, start agent processes       │
│     e. run test functions                            │
│     f. stop agent processes                          │
│  6. podman-compose down -v                           │
│  7. report results                                   │
└──────────────────────────────────────────────────────┘
```

Each suite starts with a clean database. The `--developer-mode` wipe endpoint makes this fast (no need to destroy and recreate containers). Agent processes are started and stopped per-suite, not per-test, to avoid startup overhead.

---

## CI Integration

Add to CI pipeline (GitHub Actions or similar):

```yaml
integration-tests:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Install podman-compose
      run: pip install podman-compose
    - name: Build
      run: cargo build
    - name: Run integration tests
      run: tests/integration/run.sh
      timeout-minutes: 15
```

Expected runtime: ~3-5 minutes (most time is agent crawl waits and replication convergence).

---

## File Layout

```
tests/
├── docker-compose.integration.yml
└── integration/
    ├── run.sh                    # Main orchestrator
    ├── helpers.sh                # api_login, api_get, assert_eq, wait_for
    ├── test_01_bootstrap.sh      # Suite 1
    ├── test_02_agent.sh          # Suite 2
    ├── test_03_crawl.sh          # Suite 3
    ├── test_04_labels.sh         # Suite 4
    ├── test_05_vfs.sh            # Suite 5
    ├── test_06_replication.sh    # Suite 6
    ├── test_07_notifications.sh  # Suite 7
    ├── test_08_events.sh         # Suite 8
    ├── test_09_backup.sh         # Suite 9
    └── test_10_edge_cases.sh     # Suite 10
```

---

## Code Changes Required

| File | Change |
|------|--------|
| `mosaicfs-server/src/main.rs` | Add `--json` flag to bootstrap subcommand |
| `tests/docker-compose.integration.yml` | New file: integration test compose stack |
| `tests/integration/run.sh` | New file: test orchestrator |
| `tests/integration/helpers.sh` | New file: shared test utilities |
| `tests/integration/test_*.sh` | New files: test suites |

The only production code change is the `--json` flag on bootstrap. Everything else is new test infrastructure.

---

## Open Questions

1. **CouchDB reset strategy** — The plan assumes `DELETE /api/system/data` (developer mode) is implemented in Phase 8. If Phase 8 is not yet done, the alternative is to drop and recreate the CouchDB database directly via CouchDB admin API (`DELETE /mosaicfs`, `PUT /mosaicfs`). This works but bypasses the server and may leave stale server-side caches.

2. **Crawl timing** — Agent crawl is triggered on startup. Tests that depend on crawl results need to wait. The simplest approach is to poll `GET /api/files?node_id=...` until the expected count appears, with a timeout. An alternative is to add a `/api/agent/crawl-status` endpoint that reports whether the initial crawl is complete.

3. **Replication convergence** — CouchDB replication is eventually consistent. Tests that verify replication must poll with timeouts. A reasonable default is 30 seconds for replication to converge within a local compose stack.

4. **FUSE testing** — FUSE mount tests require `--privileged` or `--device /dev/fuse` on the container. This is feasible but adds complexity. Recommend deferring FUSE integration tests to a later phase and testing the VFS logic through the HTTP API (`/api/vfs/*`) which exercises the same step evaluation and readdir code without requiring a kernel mount.
