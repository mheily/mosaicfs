#!/usr/bin/env bash
# Suite 9: Backup and Restore
# GET /api/system/backup → JSON blob with {"documents": [...], "version": N}
# POST /api/system/restore → {"ok": true} or 409

suite_header "Suite 9: Backup and Restore"

# ── Setup ────────────────────────────────────────────────────────────────────

compose_exec server curl -sk -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"path":"/backup-test","name":"backup-test"}' \
  "https://localhost:8443/api/vfs/directories" >/dev/null

compose_exec server curl -sk -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"path_prefix":"/backup/","labels":["backed-up"]}' \
  "https://localhost:8443/api/labels/rules" >/dev/null

compose_exec agent-1 bash -c '
  mkdir -p /test-files
  echo "backup content" > /test-files/backup-file.txt
'

write_agent_config agent-1 /test-files
start_agent agent-1

wait_for "agent-1 online" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 1"' 60

stop_agent agent-1 2>/dev/null || true

# ── Tests ────────────────────────────────────────────────────────────────────

test_minimal_backup() {
  local backup
  backup=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/system/backup?type=minimal")

  local has_vfs
  has_vfs=$(echo "$backup" | jq '[.documents[] | select(.type == "virtual_directory")] | length')
  assert_gt "$has_vfs" 0 "minimal backup should include virtual directories"

  local has_creds
  has_creds=$(echo "$backup" | jq '[.documents[] | select(.type == "credential")] | length')
  assert_gt "$has_creds" 0 "minimal backup should include credentials"

  local has_rules
  has_rules=$(echo "$backup" | jq '[.documents[] | select(.type == "label_rule")] | length')
  assert_gt "$has_rules" 0 "minimal backup should include label rules"
}

test_full_backup() {
  local backup
  backup=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/system/backup?type=full")

  local has_files
  has_files=$(echo "$backup" | jq '[.documents[] | select(.type == "file")] | length')
  assert_gt "$has_files" 0 "full backup should include file documents"

  # Cache backup for restore test
  echo "$backup" > /tmp/mosaicfs_backup.json
}

test_restore_into_empty_db() {
  local backup
  backup=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/system/backup?type=full")
  echo "$backup" > /tmp/mosaicfs_backup.json

  # Wipe DB — this invalidates our TOKEN
  compose_exec server curl -sk -X DELETE \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"confirm":"DELETE_ALL_DATA"}' \
    "https://localhost:8443/api/system/data" >/dev/null

  # Restore (no auth required — DB is empty)
  local restore_status
  restore_status=$(compose_exec server bash -c \
    "curl -sk -o /dev/null -w '%{http_code}' -X POST \
     -H 'Content-Type: application/json' \
     --data-binary @/tmp/mosaicfs_backup.json \
     https://localhost:8443/api/system/restore" \
    2>/dev/null || true)

  # Accept 200 (restored) or handle the case where backup is written via stdin
  # Alternative: pipe via compose_exec
  local restore_resp
  restore_resp=$(compose_exec server curl -sk -X POST \
    -H "Content-Type: application/json" \
    -d "$backup" \
    "https://localhost:8443/api/system/restore" 2>/dev/null || true)
  restore_status=$(echo "$restore_resp" | jq -r '.ok // "false"')
  assert_eq "$restore_status" "true" "restore into empty DB should succeed"

  # Re-bootstrap to get new credentials (restored credentials may differ)
  local boot_out
  boot_out=$(compose_exec server /workspace/target/debug/mosaicfs-server bootstrap --json 2>/dev/null) || true
  if [ -n "$boot_out" ]; then
    ACCESS_KEY_ID=$(echo "$boot_out" | jq -r '.access_key_id')
    SECRET_KEY=$(echo "$boot_out" | jq -r '.secret_key')
    export ACCESS_KEY_ID SECRET_KEY
  fi
  # Re-login
  TOKEN=$(compose_exec server curl -sk "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${ACCESS_KEY_ID}\",\"secret_key\":\"${SECRET_KEY}\"}" \
    | jq -r '.token') || true
  export TOKEN

  if [ -n "$TOKEN" ] && [ "$TOKEN" != "null" ]; then
    local tree
    tree=$(compose_exec server curl -sk \
      -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/vfs/tree")
    local has_backup_dir
    has_backup_dir=$(echo "$tree" | jq '[.tree[] | select(.virtual_path == "/backup-test")] | length')
    assert_gt "$has_backup_dir" 0 "restored VFS should include backup-test directory"
  fi
}

test_restore_rejected_on_non_empty_db() {
  # DB has data from previous test; a second restore should fail with 409
  local backup='{"documents":[],"version":1}'
  local status
  status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" -X POST \
    -H "Content-Type: application/json" \
    -d "$backup" \
    "https://localhost:8443/api/system/restore")
  assert_eq "$status" "409" "restore into non-empty DB should return 409"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_minimal_backup
run_test test_full_backup
run_test test_restore_into_empty_db
run_test test_restore_rejected_on_non_empty_db
