#!/usr/bin/env bash
# Suite 10: Error Handling and Edge Cases

suite_header "Suite 10: Error Handling and Edge Cases"

# ── Tests ────────────────────────────────────────────────────────────────────

test_expired_jwt_rejected() {
  local expired_token="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ0ZXN0IiwiZXhwIjoxMDAwMDAwMDAwfQ.invalid"
  local status
  status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer ${expired_token}" \
    "https://localhost:8443/api/nodes")
  assert_eq "$status" "401" "expired/invalid JWT should return 401"
}

test_hmac_replay_rejected() {
  local stale_ts="2020-01-01T00:00:00Z"
  local status
  status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    -H "Authorization: MOSAICFS-HMAC-SHA256 Credential=${ACCESS_KEY_ID},Timestamp=${stale_ts},Signature=fakesig" \
    "https://localhost:8443/api/agent/credentials")
  assert_eq "$status" "401" "stale HMAC timestamp should be rejected"
}

test_disabled_credential_rejected() {
  local create_resp
  create_resp=$(compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"name":"disable-test"}' \
    "https://localhost:8443/api/credentials")
  local new_key
  new_key=$(echo "$create_resp" | jq -r '.access_key_id')
  local new_secret
  new_secret=$(echo "$create_resp" | jq -r '.secret_key')

  local login_status
  login_status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${new_key}\",\"secret_key\":\"${new_secret}\"}")
  assert_eq "$login_status" "200" "new credential should login successfully"

  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"enabled":false}' \
    "https://localhost:8443/api/credentials/${new_key}" >/dev/null

  login_status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${new_key}\",\"secret_key\":\"${new_secret}\"}")
  assert_eq "$login_status" "401" "disabled credential should fail login"

  compose_exec server curl -sk -X DELETE \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/credentials/${new_key}" >/dev/null
}

test_large_batch_crawl() {
  compose_exec agent-1 bash -c '
    mkdir -p /test-files/batch
    for i in $(seq 1 10000); do
      echo "file $i" > "/test-files/batch/file_${i}.txt"
    done
  '

  write_agent_config agent-1 /test-files
  start_agent agent-1

  wait_for "agent-1 online" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

  wait_for "batch files indexed" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/files" | jq -e ".total >= 1000"' 180

  local files
  files=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files")
  local count
  count=$(echo "$files" | jq '.total')
  assert_gt "$count" 999 "should have indexed at least 1000 files from batch"

  stop_agent agent-1 2>/dev/null || true
}

test_concurrent_agent_registration() {
  stop_agent agent-1 2>/dev/null || true
  stop_agent agent-2 2>/dev/null || true

  compose_exec agent-1 bash -c 'mkdir -p /test-files && echo "a" > /test-files/a.txt'
  compose_exec agent-2 bash -c 'mkdir -p /test-files && echo "b" > /test-files/b.txt'

  write_agent_config agent-1 /test-files
  write_agent_config agent-2 /test-files

  start_agent agent-1
  start_agent agent-2

  wait_for "both agents online" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/nodes" | jq -e "[.items[] | select(.status == \"online\")] | length >= 2"' 30

  local nodes
  nodes=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes")
  local online_count
  online_count=$(echo "$nodes" | jq '[.items[] | select(.status == "online")] | length')
  assert_gt "$online_count" 1 "both agents should register without conflicts"

  # Verify distinct node IDs
  local ids
  ids=$(echo "$nodes" | jq -r '[.items[] | select(.status == "online")] | .[].id')
  local unique_count
  unique_count=$(echo "$ids" | sort -u | wc -l | tr -d ' ')
  assert_gt "$unique_count" 1 "agents should have distinct node IDs"

  stop_agent agent-1 2>/dev/null || true
  stop_agent agent-2 2>/dev/null || true
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_expired_jwt_rejected
run_test test_hmac_replay_rejected
run_test test_disabled_credential_rejected
run_test test_large_batch_crawl
run_test test_concurrent_agent_registration
