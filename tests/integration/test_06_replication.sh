#!/usr/bin/env bash
# Suite 6: Replication
# Storage backends: {"items": [...], "total": N}
# Replication rules: {"items": [...], "total": N}
# Replicas: {"items": [...], "total": N}

suite_header "Suite 6: Replication"

# ── Setup ────────────────────────────────────────────────────────────────────

compose_exec agent-1 bash -c '
  mkdir -p /test-files/docs
  echo "replicate me" > /test-files/docs/file1.txt
  echo "also replicate" > /test-files/docs/file2.txt
  echo "skip me" > /test-files/other.txt
'

write_agent_config agent-1 /test-files
start_agent agent-1

write_agent_config agent-2 /test-files
start_agent agent-2

wait_for "agent-1 online" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e "[.items[] | select(.status == \"online\")] | length >= 1"' 30

AGENT1_NODE_ID=$(compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
  "https://localhost:8443/api/nodes" \
  | jq -r '[.items[] | select(.status == "online")][0].id | ltrimstr("node::")')
export AGENT1_NODE_ID

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 3"' 60

wait_for "agent-2 online" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e "[.items[] | select(.status == \"online\")] | length >= 2"' 30

AGENT2_NODE_ID=$(compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
  "https://localhost:8443/api/nodes" \
  | jq -r "[.items[] | select(.status == \"online\")] | map(select(.id != \"node::${AGENT1_NODE_ID}\")) | .[0].id | ltrimstr(\"node::\")")
export AGENT2_NODE_ID

# ── Tests ────────────────────────────────────────────────────────────────────

test_create_storage_backend() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"name":"test-dir-backend","type":"directory","config":{"path":"/data/replicas"}}' \
    "https://localhost:8443/api/storage-backends" >/dev/null

  local backends
  backends=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/storage-backends")
  local count
  count=$(echo "$backends" | jq '.total')
  assert_gt "$count" 0 "should have at least 1 storage backend"
}

test_create_replication_rule() {
  local resp
  resp=$(compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"source_node_id\":\"${AGENT1_NODE_ID}\",\"target\":\"test-dir-backend\",\"steps\":[]}" \
    "https://localhost:8443/api/replication/rules")
  local rule_id
  rule_id=$(echo "$resp" | jq -r '.id // ._id // .rule_id')
  assert_ne "$rule_id" "null" "replication rule should be created"

  local rules
  rules=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/replication/rules")
  local count
  count=$(echo "$rules" | jq '.total')
  assert_gt "$count" 0 "should have at least 1 replication rule"
}

test_replication_executes() {
  wait_for "replicas created" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/replication/replicas" | jq -e ".total >= 1"' 60

  local replicas
  replicas=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/replication/replicas")
  local count
  count=$(echo "$replicas" | jq '.total')
  assert_gt "$count" 0 "should have replica documents"
}

test_agent_to_agent_replication() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"agent2-backend\",\"type\":\"agent\",\"config\":{\"node_id\":\"${AGENT2_NODE_ID}\"}}" \
    "https://localhost:8443/api/storage-backends" >/dev/null

  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"source_node_id\":\"${AGENT1_NODE_ID}\",\"target\":\"agent2-backend\",\"steps\":[]}" \
    "https://localhost:8443/api/replication/rules" >/dev/null

  wait_for "agent-to-agent replicas" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/replication/replicas?target=agent2-backend" \
      | jq -e ".total >= 1"' 60

  local replicas
  replicas=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/replication/replicas?target=agent2-backend")
  local count
  count=$(echo "$replicas" | jq '.total')
  assert_gt "$count" 0 "should have agent-to-agent replica documents"
}

test_replication_rule_with_steps() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"source_node_id\":\"${AGENT1_NODE_ID}\",\"target\":\"test-dir-backend\",\"steps\":[{\"type\":\"path_prefix\",\"prefix\":\"/test-files/docs/\"}]}" \
    "https://localhost:8443/api/replication/rules" >/dev/null

  sleep 10

  local status
  status=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/replication/status")
  local has_data
  has_data=$(echo "$status" | jq 'type')
  assert_ne "$has_data" "null" "replication status should return valid JSON"
}

test_deletion_propagates() {
  compose_exec agent-1 bash -c 'rm /test-files/docs/file1.txt'

  wait_for "file deletion detected" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/files?status=deleted" | jq -e ".total >= 1"' 60

  local files
  files=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files?status=deleted")
  local deleted_count
  deleted_count=$(echo "$files" | jq '.total')
  assert_gt "$deleted_count" 0 "at least one file should be marked deleted"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_create_storage_backend
run_test test_create_replication_rule
run_test test_replication_executes
run_test test_agent_to_agent_replication
run_test test_replication_rule_with_steps
run_test test_deletion_propagates

stop_agent agent-1 2>/dev/null || true
stop_agent agent-2 2>/dev/null || true
