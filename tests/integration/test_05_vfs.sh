#!/usr/bin/env bash
# Suite 5: Virtual Filesystem
# GET /api/vfs/tree → {"path": "...", "tree": [...]}
# GET /api/vfs?path=... → {"files": [...]}
# Node id field: "node::xyz" — strip prefix for node_id in steps

suite_header "Suite 5: Virtual Filesystem"

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
export AGENT1_NODE_ID

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 3"' 60

# ── Tests ────────────────────────────────────────────────────────────────────

test_root_directory_exists() {
  # {"path": "...", "tree": [...]}
  local tree
  tree=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs/tree")
  local path
  path=$(echo "$tree" | jq -r '.path')
  assert_ne "$path" "null" "VFS tree should have a path field"
}

test_create_virtual_directory() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"path":"/projects","name":"projects"}' \
    "https://localhost:8443/api/vfs/directories" >/dev/null

  local tree
  tree=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs/tree")
  local has_projects
  has_projects=$(echo "$tree" | jq '[.tree[] | select(.virtual_path == "/projects")] | length')
  assert_gt "$has_projects" 0 "/projects should appear in tree"
}

test_mount_with_steps() {
  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"mounts\":[{\"steps\":[{\"type\":\"node\",\"node_id\":\"${AGENT1_NODE_ID}\"},{\"type\":\"path_prefix\",\"prefix\":\"/test-files/docs/\"}]}]}" \
    "https://localhost:8443/api/vfs/directories/projects" >/dev/null

  # GET /api/vfs?path=... → {"files": [...]}
  local contents
  contents=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs?path=/projects")
  local file_count
  file_count=$(echo "$contents" | jq '.files | length')
  assert_gt "$file_count" 0 "mount should resolve files from agent-1 docs"
}

test_mount_with_label_filter() {
  local file_id
  file_id=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files/by-path?path=/test-files/docs/readme.txt" \
    | jq -r '.id')

  compose_exec server curl -sk -X PUT \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"file_id\":\"${file_id}\",\"labels\":[\"featured\"]}" \
    "https://localhost:8443/api/labels/assignments" >/dev/null

  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"path":"/featured","name":"featured"}' \
    "https://localhost:8443/api/vfs/directories" >/dev/null

  sleep 2  # label cache update

  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"mounts":[{"steps":[{"type":"label","label":"featured"}]}]}' \
    "https://localhost:8443/api/vfs/directories/featured" >/dev/null

  local contents
  contents=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs?path=/featured")
  local file_count
  file_count=$(echo "$contents" | jq '.files | length')
  assert_gt "$file_count" 0 "label-filtered mount should resolve at least 1 file"
}

test_nested_directories() {
  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"path":"/projects/alpha","name":"alpha"}' \
    "https://localhost:8443/api/vfs/directories" >/dev/null

  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"path":"/projects/beta","name":"beta"}' \
    "https://localhost:8443/api/vfs/directories" >/dev/null

  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"mounts\":[{\"steps\":[{\"type\":\"node\",\"node_id\":\"${AGENT1_NODE_ID}\"},{\"type\":\"path_prefix\",\"prefix\":\"/test-files/docs/\"}]}]}" \
    "https://localhost:8443/api/vfs/directories/projects/alpha" >/dev/null

  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"mounts\":[{\"steps\":[{\"type\":\"node\",\"node_id\":\"${AGENT1_NODE_ID}\"},{\"type\":\"path_prefix\",\"prefix\":\"/test-files/photos/\"}]}]}" \
    "https://localhost:8443/api/vfs/directories/projects/beta" >/dev/null

  local alpha_files beta_files
  alpha_files=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs?path=/projects/alpha" | jq '.files | length')
  beta_files=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/vfs?path=/projects/beta" | jq '.files | length')

  assert_gt "$alpha_files" 0 "alpha should have files"
  assert_gt "$beta_files" 0 "beta should have files"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_root_directory_exists
run_test test_create_virtual_directory
run_test test_mount_with_steps
run_test test_mount_with_label_filter
run_test test_nested_directories

stop_agent agent-1 2>/dev/null || true
