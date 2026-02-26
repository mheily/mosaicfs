#!/usr/bin/env bash
# Suite 8: Events and Real-time Sync
# File by-path returns single doc with "id" field

suite_header "Suite 8: Events and Real-time Sync"

# ── Setup ────────────────────────────────────────────────────────────────────

compose_exec agent-1 bash -c '
  mkdir -p /test-files/docs
  echo "initial" > /test-files/docs/initial.txt
'

write_agent_config agent-1 /test-files
start_agent agent-1

wait_for "agent-1 online" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

AGENT1_NODE_ID=$(compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
  "https://localhost:8443/api/nodes" | jq -r '.items[0].id | ltrimstr("node::")')
export AGENT1_NODE_ID

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 1"' 60

# ── Tests ────────────────────────────────────────────────────────────────────

test_changes_feed_delivers_updates() {
  local db_info
  db_info=$(compose_exec couchdb curl -s -u admin:testpassword "http://localhost:5984/mosaicfs")
  local seq
  seq=$(echo "$db_info" | jq -r '.update_seq')

  compose_exec agent-1 bash -c 'echo "changes test" > /test-files/docs/changes_test.txt'

  wait_for "changes test file indexed" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/files/by-path?path=/test-files/docs/changes_test.txt" \
      | jq -e ".id"' 60

  local changes
  changes=$(compose_exec couchdb curl -s -u admin:testpassword \
    "http://localhost:5984/mosaicfs/_changes?since=${seq}&limit=100")
  local results_count
  results_count=$(echo "$changes" | jq '.results | length')
  assert_gt "$results_count" 0 "changes feed should have new entries"
}

test_label_cache_updates() {
  local file_id
  file_id=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/by-path?path=/test-files/docs/initial.txt" \
    | jq -r '.id')
  assert_ne "$file_id" "null" "initial.txt should be indexed"

  compose_exec server curl -sk -X PUT \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"file_id\":\"${file_id}\",\"labels\":[\"cached-label\"]}" \
    "https://localhost:8443/api/labels/assignments" >/dev/null

  sleep 3
  # Effective labels: {"file_id": "...", "labels": [...]}
  local effective
  effective=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/labels/effective?file_id=${file_id}")
  local has_label
  has_label=$(echo "$effective" | jq '.labels' | grep -c "cached-label" || true)
  assert_gt "$has_label" 0 "effective labels should include newly assigned label"
}

test_readdir_cache_invalidation() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"path":"/cache-test","name":"cache-test"}' \
    "https://localhost:8443/api/vfs/directories" >/dev/null

  # First read (empty)
  compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs?path=/cache-test" >/dev/null

  # Add mount
  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"mounts\":[{\"steps\":[{\"type\":\"node\",\"node_id\":\"${AGENT1_NODE_ID}\"}]}]}" \
    "https://localhost:8443/api/vfs/directories/cache-test" >/dev/null

  sleep 2
  # Second read (should have files now)
  local contents
  contents=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs?path=/cache-test")
  local count
  count=$(echo "$contents" | jq '.files | length')
  assert_gt "$count" 0 "directory contents should reflect mount after cache invalidation"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_changes_feed_delivers_updates
run_test test_label_cache_updates
run_test test_readdir_cache_invalidation

stop_agent agent-1 2>/dev/null || true
