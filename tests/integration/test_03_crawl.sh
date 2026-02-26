#!/usr/bin/env bash
# Suite 3: Filesystem Crawling
# File list response: {"items": [...], "total": N}
# File doc fields: id, name, size, status, source.node_id, source.export_path
# by-path response: single doc directly ({"id":..., "name":..., "size":...})
# Node doc id field: "node::xyz" → raw node_id is stripped of prefix

suite_header "Suite 3: Filesystem Crawling"

# ── Setup ────────────────────────────────────────────────────────────────────

compose_exec agent-1 bash -c '
  mkdir -p /test-files/docs /test-files/photos
  printf "hello world\n" > /test-files/docs/readme.txt
  dd if=/dev/urandom of=/test-files/photos/image.bin bs=1024 count=100 2>/dev/null
  echo "report data" > /test-files/docs/report.csv
'

write_agent_config agent-1 /test-files
start_agent agent-1

wait_for "agent-1 registered" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

# Get the raw node_id (strip "node::" prefix from the id field)
AGENT1_NODE_ID=$(compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
  "https://localhost:8443/api/nodes" \
  | jq -r '.items[0].id | ltrimstr("node::")')
export AGENT1_NODE_ID

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 3"' 60

# ── Tests ────────────────────────────────────────────────────────────────────

test_files_indexed() {
  local files
  files=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files")
  local count
  count=$(echo "$files" | jq '.total')
  assert_gt "$count" 2 "at least 3 files indexed"

  # Verify file documents have required fields
  local first
  first=$(echo "$files" | jq '.items[0]')
  local has_name
  has_name=$(echo "$first" | jq -r '.name')
  assert_ne "$has_name" "" "file should have a name"
  assert_ne "$has_name" "null" "file name should not be null"
}

test_file_by_path() {
  # by-path returns a single doc directly
  local file
  file=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/by-path?path=/test-files/docs/readme.txt")

  local size
  size=$(echo "$file" | jq -r '.size')
  assert_eq "$size" "12" "readme.txt should be 12 bytes (hello world + newline)"
}

test_file_content_download() {
  local file
  file=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/by-path?path=/test-files/docs/readme.txt")
  local file_id
  file_id=$(echo "$file" | jq -r '.id')
  assert_ne "$file_id" "null" "file should have an id"

  local content
  content=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/${file_id}/content")
  assert_eq "$content" "hello world" "file content should match"
}

test_file_content_range_request() {
  local file
  file=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/by-path?path=/test-files/docs/readme.txt")
  local file_id
  file_id=$(echo "$file" | jq -r '.id')

  local status
  status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Range: bytes=0-4" \
    "https://localhost:8443/api/files/${file_id}/content")
  assert_eq "$status" "206" "range request should return 206"

  local partial
  partial=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Range: bytes=0-4" \
    "https://localhost:8443/api/files/${file_id}/content")
  assert_eq "$partial" "hello" "range 0-4 should return 'hello'"
}

test_new_file_detected() {
  compose_exec agent-1 bash -c 'printf "new content\n" > /test-files/docs/newfile.txt'

  wait_for "new file detected" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/files/by-path?path=/test-files/docs/newfile.txt" \
      | jq -e ".id"' 60

  local file
  file=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/by-path?path=/test-files/docs/newfile.txt")
  local size
  size=$(echo "$file" | jq -r '.size')
  assert_eq "$size" "12" "newfile.txt should be 12 bytes"
}

test_deleted_file_soft_deleted() {
  compose_exec agent-1 bash -c 'rm /test-files/docs/report.csv'

  # Need status=deleted in the query to see soft-deleted files
  wait_for "deleted file marked" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/files?status=deleted" | jq -e ".total >= 1"' 60

  local files
  files=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files?status=deleted")
  local deleted_count
  deleted_count=$(echo "$files" | jq '.total')
  assert_gt "$deleted_count" 0 "deleted file should appear in status=deleted query"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_files_indexed
run_test test_file_by_path
run_test test_file_content_download
run_test test_file_content_range_request
run_test test_new_file_detected
run_test test_deleted_file_soft_deleted

stop_agent agent-1 2>/dev/null || true
