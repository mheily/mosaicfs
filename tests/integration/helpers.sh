#!/usr/bin/env bash
# Integration test helpers for MosaicFS
# Sourced by run.sh and individual test suites.

set -euo pipefail

# ── Globals (set by run.sh) ──────────────────────────────────────────────────
# ACCESS_KEY_ID, SECRET_KEY, TOKEN, COMPOSE, SERVER_URL, COUCHDB_URL
# COMPOSE_PROJECT: podman-compose project name (default: "tests")
COMPOSE_PROJECT="${COMPOSE_PROJECT:-tests}"

# ── Colour output ────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No colour

# ── Counters ─────────────────────────────────────────────────────────────────
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0
FAILED_TESTS=()

# ── Authentication ───────────────────────────────────────────────────────────

# Authenticate and return a JWT token
api_login() {
  local key_id="${1:-$ACCESS_KEY_ID}"
  local secret="${2:-$SECRET_KEY}"
  curl -sk "${SERVER_URL}/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${key_id}\",\"secret_key\":\"${secret}\"}" \
    | jq -r '.token'
}

# Refresh the global TOKEN variable
refresh_token() {
  TOKEN=$(api_login)
  export TOKEN
}

# ── API helpers ──────────────────────────────────────────────────────────────

api_get() {
  curl -sk -H "Authorization: Bearer ${TOKEN}" "${SERVER_URL}${1}"
}

api_post() {
  curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${2:-{}}" \
    "${SERVER_URL}${1}"
}

api_put() {
  curl -sk -X PUT \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${2:-{}}" \
    "${SERVER_URL}${1}"
}

api_patch() {
  curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${2:-{}}" \
    "${SERVER_URL}${1}"
}

api_delete() {
  curl -sk -X DELETE \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "${2:-{}}" \
    "${SERVER_URL}${1}"
}

# Raw curl with custom options (for status code checks, range requests, etc.)
api_raw() {
  curl -sk -H "Authorization: Bearer ${TOKEN}" "$@"
}

# ── CouchDB direct access ───────────────────────────────────────────────────

couch_get() {
  curl -s -u "admin:testpassword" "${COUCHDB_URL}${1}"
}

couch_put() {
  curl -s -X PUT -u "admin:testpassword" \
    -H "Content-Type: application/json" \
    -d "${2:-{}}" \
    "${COUCHDB_URL}${1}"
}

couch_delete() {
  curl -s -X DELETE -u "admin:testpassword" "${COUCHDB_URL}${1}"
}

# ── Container helpers ────────────────────────────────────────────────────────

# Use podman exec directly — podman-compose exec prints its own diagnostic
# lines to stdout (not stderr), which corrupts captured output.
# Container names follow the pattern: {project}_{service}_1
compose_exec() {
  local service="$1"; shift
  local container="${COMPOSE_PROJECT}_${service}_1"
  podman exec -i "$container" "$@"
}

# ── Wait / polling ───────────────────────────────────────────────────────────

# Wait for a condition to become true.
# Usage: wait_for "description" "command" [timeout_secs]
wait_for() {
  local desc="$1" cmd="$2" timeout="${3:-30}"
  for i in $(seq 1 "$timeout"); do
    if eval "$cmd" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo -e "${RED}TIMEOUT waiting for: ${desc}${NC}" >&2
  return 1
}

# Wait for server health endpoint
wait_for_server() {
  wait_for "server healthy" \
    "curl -sk ${SERVER_URL}/api/health | jq -e '.ok'" \
    "${1:-60}"
}

# Wait for CouchDB to be reachable
wait_for_couchdb() {
  wait_for "couchdb up" \
    "curl -sf ${COUCHDB_URL}/_up" \
    "${1:-30}"
}

# ── Assertions ───────────────────────────────────────────────────────────────

assert_eq() {
  local actual="$1" expected="$2" msg="${3:-}"
  if [ "$actual" != "$expected" ]; then
    echo -e "${RED}ASSERTION FAILED${NC}: expected '${expected}', got '${actual}'" >&2
    [ -n "$msg" ] && echo "  Context: $msg" >&2
    return 1
  fi
}

assert_ne() {
  local actual="$1" not_expected="$2" msg="${3:-}"
  if [ "$actual" = "$not_expected" ]; then
    echo -e "${RED}ASSERTION FAILED${NC}: did not expect '${not_expected}'" >&2
    [ -n "$msg" ] && echo "  Context: $msg" >&2
    return 1
  fi
}

assert_contains() {
  local haystack="$1" needle="$2" msg="${3:-}"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo -e "${RED}ASSERTION FAILED${NC}: '${haystack}' does not contain '${needle}'" >&2
    [ -n "$msg" ] && echo "  Context: $msg" >&2
    return 1
  fi
}

assert_json_field() {
  local json="$1" field="$2" expected="$3" msg="${4:-}"
  local actual
  actual=$(echo "$json" | jq -r "$field")
  assert_eq "$actual" "$expected" "${msg:-field $field}"
}

assert_http_status() {
  local url="$1" expected_status="$2" msg="${3:-}"
  local status
  status=$(curl -sk -o /dev/null -w "%{http_code}" -H "Authorization: Bearer ${TOKEN}" "${SERVER_URL}${url}")
  assert_eq "$status" "$expected_status" "${msg:-HTTP status for ${url}}"
}

assert_gt() {
  local actual="$1" threshold="$2" msg="${3:-}"
  if [ "$actual" -le "$threshold" ] 2>/dev/null; then
    echo -e "${RED}ASSERTION FAILED${NC}: expected > ${threshold}, got '${actual}'" >&2
    [ -n "$msg" ] && echo "  Context: $msg" >&2
    return 1
  fi
}

# ── Database management ──────────────────────────────────────────────────────

# Wipe database via developer-mode endpoint (requires valid TOKEN)
wipe_db() {
  api_delete "/api/system/data" '{"confirm":"DELETE_ALL_DATA"}'
}

# Bootstrap and capture credentials; sets ACCESS_KEY_ID, SECRET_KEY, TOKEN
do_bootstrap() {
  local output
  output=$(compose_exec server /workspace/target/debug/mosaicfs-server bootstrap --json)
  ACCESS_KEY_ID=$(echo "$output" | jq -r '.access_key_id')
  SECRET_KEY=$(echo "$output" | jq -r '.secret_key')
  export ACCESS_KEY_ID SECRET_KEY
  refresh_token
}

# ── Agent management ─────────────────────────────────────────────────────────

# Write agent.toml into an agent container
write_agent_config() {
  local container="$1"
  local watch_path="${2:-/test-files}"
  compose_exec "$container" bash -c "mkdir -p /data/state && cat > /data/state/agent.toml" <<EOF
control_plane_url = "https://server:8443"
watch_paths = ["${watch_path}"]
excluded_paths = []
access_key_id = "${ACCESS_KEY_ID}"
secret_key = "${SECRET_KEY}"
EOF
}

# Start the agent binary inside a container (background)
start_agent() {
  local container="$1"
  compose_exec "$container" bash -c \
    'nohup /workspace/target/debug/mosaicfs-agent --config /data/state/agent.toml >/tmp/agent.log 2>&1 &'
}

# Stop the agent process inside a container
stop_agent() {
  local container="$1"
  compose_exec "$container" bash -c 'pkill -f mosaicfs-agent || true'
}

# Get the node_id for a running agent container
get_node_id() {
  local container="$1"
  compose_exec "$container" cat /data/state/node_id 2>/dev/null || \
    compose_exec "$container" cat /var/lib/mosaicfs/node_id 2>/dev/null || true
}

# ── Test runner ──────────────────────────────────────────────────────────────

# Run a single test function with pass/fail tracking
run_test() {
  local test_name="$1"
  TESTS_RUN=$((TESTS_RUN + 1))
  echo -n "  $test_name ... "
  if "$test_name" 2>/tmp/test_stderr; then
    echo -e "${GREEN}PASS${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
  else
    echo -e "${RED}FAIL${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
    FAILED_TESTS+=("$test_name")
    if [ -s /tmp/test_stderr ]; then
      sed 's/^/    /' /tmp/test_stderr
    fi
  fi
}

# Skip a test with a reason
skip_test() {
  local test_name="$1" reason="$2"
  TESTS_RUN=$((TESTS_RUN + 1))
  TESTS_SKIPPED=$((TESTS_SKIPPED + 1))
  echo -e "  $test_name ... ${YELLOW}SKIP${NC} ($reason)"
}

# Print suite header
suite_header() {
  echo ""
  echo -e "━━━ ${YELLOW}$1${NC} ━━━"
}

# Print final summary
print_summary() {
  echo ""
  echo "════════════════════════════════════════════════"
  echo -e "  Tests run:    ${TESTS_RUN}"
  echo -e "  Passed:       ${GREEN}${TESTS_PASSED}${NC}"
  echo -e "  Failed:       ${RED}${TESTS_FAILED}${NC}"
  echo -e "  Skipped:      ${YELLOW}${TESTS_SKIPPED}${NC}"
  echo "════════════════════════════════════════════════"
  if [ ${#FAILED_TESTS[@]} -gt 0 ]; then
    echo -e "${RED}Failed tests:${NC}"
    for t in "${FAILED_TESTS[@]}"; do
      echo "  - $t"
    done
  fi
  echo ""
}
