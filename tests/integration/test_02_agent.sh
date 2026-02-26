#!/usr/bin/env bash
# Suite 2: Agent Registration and Heartbeat
# Response format: GET /api/nodes → {"items": [...], "total": N}
# Node doc fields (after strip_internals): id="node::xyz", status, last_heartbeat

suite_header "Suite 2: Agent Registration and Heartbeat"

# ── Setup ────────────────────────────────────────────────────────────────────

write_agent_config agent-1 /test-files
start_agent agent-1

wait_for "agent-1 registered" \
  'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes" | jq -e ".items[] | select(.status == \"online\")"' 30

# ── Tests ────────────────────────────────────────────────────────────────────

test_agent_registers_node() {
  local nodes
  nodes=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes")
  local count
  count=$(echo "$nodes" | jq '.total')
  assert_gt "$count" 0 "at least one node registered"

  local node
  node=$(echo "$nodes" | jq '.items[0]')
  local status
  status=$(echo "$node" | jq -r '.status')
  assert_eq "$status" "online" "node status should be online"

  # id is "node::xyz" after strip_internals renames _id
  local node_id
  node_id=$(echo "$node" | jq -r '.id')
  assert_ne "$node_id" "null" "node should have an id"
  assert_contains "$node_id" "node::" "node id should have node:: prefix"
}

test_agent_heartbeat() {
  local nodes
  nodes=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes")
  local hb1
  hb1=$(echo "$nodes" | jq -r '.items[0].last_heartbeat')
  assert_ne "$hb1" "null" "should have a heartbeat timestamp"

  sleep 5
  nodes=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes")
  local hb2
  hb2=$(echo "$nodes" | jq -r '.items[0].last_heartbeat')
  assert_ne "$hb2" "null" "second heartbeat should exist"
}

test_agent_goes_offline() {
  stop_agent agent-1

  local went_offline=false
  for i in $(seq 1 60); do
    local nodes
    nodes=$(compose_exec server curl -sk \
      -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/nodes")
    local status
    status=$(echo "$nodes" | jq -r '.items[0].status')
    if [ "$status" = "offline" ] || [ "$status" = "stale" ]; then
      went_offline=true
      break
    fi
    sleep 2
  done

  if [ "$went_offline" = true ]; then
    return 0
  else
    echo "Node did not transition to offline within timeout" >&2
    return 1
  fi
}

test_two_agents_register() {
  start_agent agent-1
  write_agent_config agent-2 /test-files
  start_agent agent-2

  wait_for "two agents registered" \
    'compose_exec server curl -sk -H "Authorization: Bearer ${TOKEN}" \
      "https://localhost:8443/api/nodes" | jq -e "[.items[] | select(.status == \"online\")] | length >= 2"' 30

  local nodes
  nodes=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/nodes")
  local online_count
  online_count=$(echo "$nodes" | jq '[.items[] | select(.status == "online")] | length')
  assert_gt "$online_count" 1 "should have at least 2 online nodes"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_agent_registers_node
run_test test_agent_heartbeat
run_test test_agent_goes_offline
run_test test_two_agents_register

stop_agent agent-1 2>/dev/null || true
stop_agent agent-2 2>/dev/null || true
