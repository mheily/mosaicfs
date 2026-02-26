#!/usr/bin/env bash
# Suite 4: Labels and Rules
# Assignments list: {"items": [...], "total": N}
# Effective labels: {"file_id": "...", "labels": [...]}
# Search: {"items": [...], "total": N}

suite_header "Suite 4: Labels and Rules"

# ── Setup ────────────────────────────────────────────────────────────────────

compose_exec agent-1 bash -c '
  mkdir -p /test-files/docs /test-files/photos
  printf "hello world\n" > /test-files/docs/readme.txt
  echo "report data" > /test-files/docs/report.csv
  echo "photo meta" > /test-files/photos/meta.txt
'

write_agent_config agent-1 /test-files
start_agent agent-1

wait_for "agent-1 online" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

AGENT1_NODE_ID=$(compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
  "https://localhost:8443/api/nodes" | jq -r '.items[0].id | ltrimstr("node::")')

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 3"' 60

# Get file_id for readme.txt (by-path returns doc directly with "id" field)
FILE_ID=$(compose_exec server curl -sk \
  -H "Authorization: Bearer ${TOKEN}" \
  "https://localhost:8443/api/files/by-path?path=/test-files/docs/readme.txt" \
  | jq -r '.id')
export FILE_ID

# ── Tests ────────────────────────────────────────────────────────────────────

test_assign_label() {
  compose_exec server curl -sk -X PUT \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"file_id\":\"${FILE_ID}\",\"labels\":[\"important\"]}" \
    "https://localhost:8443/api/labels/assignments" >/dev/null

  # Assignments: {"items": [...], "total": N}
  local assignments
  assignments=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/labels/assignments?file_id=${FILE_ID}")
  local has_important
  has_important=$(echo "$assignments" | jq '.items[0].labels // []' | grep -c "important" || true)
  assert_gt "$has_important" 0 "file should have 'important' label"
}

test_label_rule_auto_applies() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"path_prefix":"/test-files/docs/","labels":["document"]}' \
    "https://localhost:8443/api/labels/rules" >/dev/null

  sleep 2
  # Effective labels: {"file_id": "...", "labels": [...]}
  local effective
  effective=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/labels/effective?file_id=${FILE_ID}")
  local has_document
  has_document=$(echo "$effective" | jq '.labels' | grep -c "document" || true)
  assert_gt "$has_document" 0 "file under /docs/ should have effective label 'document'"
}

test_search_by_label() {
  # Search: {"items": [...], "total": N}
  local results
  results=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/search?label=important")
  local count
  count=$(echo "$results" | jq '.total')
  assert_gt "$count" 0 "search by label should return at least 1 result"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_assign_label
run_test test_label_rule_auto_applies
run_test test_search_by_label

stop_agent agent-1 2>/dev/null || true
