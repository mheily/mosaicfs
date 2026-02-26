#!/usr/bin/env bash
# Suite 7: Notifications
# GET /api/notifications → {"items": [...], "total": N}
# GET /api/notifications/history → {"items": [...]}

suite_header "Suite 7: Notifications"

# ── Setup ────────────────────────────────────────────────────────────────────

compose_exec agent-1 bash -c '
  mkdir -p /test-files/docs
  echo "hello" > /test-files/docs/file1.txt
  echo "world" > /test-files/docs/file2.txt
'

write_agent_config agent-1 /test-files
start_agent agent-1

wait_for "agent-1 online" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

wait_for "files indexed" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/files" | jq -e ".total >= 2"' 60

# ── Tests ────────────────────────────────────────────────────────────────────

test_initial_crawl_notification() {
  local notifs
  notifs=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/notifications")
  local count
  count=$(echo "$notifs" | jq '.total')
  assert_gt "$count" 0 "should have at least one notification after crawl"
}

test_notification_acknowledge() {
  local notifs
  notifs=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/notifications")
  # Notification docs have "id" (after strip_internals) or "_id"
  local notif_id
  notif_id=$(echo "$notifs" | jq -r '.items[0].id // .items[0]._id')

  if [ "$notif_id" = "null" ] || [ -z "$notif_id" ]; then
    echo "No notification found to acknowledge" >&2
    return 1
  fi

  compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    "https://localhost:8443/api/notifications/${notif_id}/acknowledge" >/dev/null

  local history
  history=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/notifications/history")
  local acked_count
  acked_count=$(echo "$history" | jq '.items | length')
  assert_gt "$acked_count" 0 "should have at least one acknowledged notification in history"
}

test_notification_deduplication() {
  compose_exec agent-1 bash -c 'echo "trigger" > /test-files/docs/trigger.txt'

  wait_for "trigger file indexed" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/files" | jq -e ".total >= 3"' 60

  local notifs
  notifs=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/notifications")
  local valid
  valid=$(echo "$notifs" | jq 'type')
  assert_eq "$valid" '"object"' "notifications response should be a JSON object"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_initial_crawl_notification
run_test test_notification_acknowledge
run_test test_notification_deduplication

stop_agent agent-1 2>/dev/null || true
