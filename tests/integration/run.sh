#!/usr/bin/env bash
# MosaicFS Integration Test Runner
# Usage: tests/integration/run.sh [suite_number...]
#   No args  → run all suites
#   1 3 5    → run only suites 1, 3, and 5

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$PROJECT_ROOT/tests/docker-compose.integration.yml"

# Detect container runtime
if command -v podman-compose &>/dev/null; then
  COMPOSE="podman-compose -f $COMPOSE_FILE"
  # podman-compose uses the directory name of the compose file as the project name
  COMPOSE_PROJECT="tests"
elif command -v docker-compose &>/dev/null; then
  COMPOSE="docker-compose -f $COMPOSE_FILE"
  COMPOSE_PROJECT="tests"
elif command -v docker &>/dev/null && docker compose version &>/dev/null 2>&1; then
  COMPOSE="docker compose -f $COMPOSE_FILE"
  COMPOSE_PROJECT="tests"
else
  echo "ERROR: No container compose tool found (podman-compose, docker-compose, docker compose)" >&2
  exit 1
fi

export COMPOSE COMPOSE_FILE PROJECT_ROOT COMPOSE_PROJECT

# Server URL — all curl calls go through compose_exec (inside the container network)
SERVER_URL="https://localhost:8443"
COUCHDB_URL="http://couchdb:5984"
export SERVER_URL COUCHDB_URL

# Source helpers (sets compose_exec and test utilities)
source "$SCRIPT_DIR/helpers.sh"

# Redirect podman-compose's verbose stdout to stderr so it doesn't pollute
# captured output. compose_exec uses podman exec directly and is unaffected.
compose_up() {
  $COMPOSE up -d "$@" >/dev/null
}

compose_down() {
  # --remove-orphans is not supported by all versions of podman-compose; use -v only
  $COMPOSE down -v >/dev/null 2>&1 || true
  # Belt-and-suspenders: remove any leftover containers by name
  for svc in couchdb server agent-1 agent-2; do
    podman rm -f "${COMPOSE_PROJECT}_${svc}_1" >/dev/null 2>&1 || true
  done
}

# Tear down any leftover stack from a previous run
compose_down

# ── Build ────────────────────────────────────────────────────────────────────

echo "Building MosaicFS..."
(cd "$PROJECT_ROOT" && cargo build 2>&1 | tail -5)
echo "Build complete."

# ── Start infrastructure ─────────────────────────────────────────────────────

echo "Starting compose stack..."
compose_up couchdb server agent-1 agent-2

echo "Waiting for CouchDB..."
wait_for "couchdb healthy" \
  "compose_exec couchdb curl -sf http://localhost:5984/_up" 30

echo "Waiting for server..."
wait_for "server healthy" \
  "compose_exec server curl -sk https://localhost:8443/api/health" 60

echo "Infrastructure ready."

# ── Determine which suites to run ────────────────────────────────────────────

ALL_SUITES=(01 02 03 04 05 06 07 08 09 10)
if [ $# -gt 0 ]; then
  SUITES=()
  for s in "$@"; do
    SUITES+=("$(printf "%02d" "$s")")
  done
else
  SUITES=("${ALL_SUITES[@]}")
fi

# ── Run suites ───────────────────────────────────────────────────────────────

run_suite() {
  local suite_num="$1"
  local suite_file="$SCRIPT_DIR/test_${suite_num}_*.sh"

  # shellcheck disable=SC2086
  local matched
  matched=$(ls $suite_file 2>/dev/null | head -1) || true
  if [ -z "$matched" ]; then
    echo "WARNING: No test file found for suite $suite_num"
    return
  fi

  # Fresh database for each suite
  if [ -n "${BOOTSTRAPPED:-}" ]; then
    # Wipe via developer-mode endpoint using the existing token
    compose_exec server curl -sk -X DELETE \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Content-Type: application/json" \
      -d '{"confirm":"DELETE_ALL_DATA"}' \
      "https://localhost:8443/api/system/data" >/dev/null
  fi

  # Bootstrap fresh credentials (compose_exec now uses podman exec directly)
  local bootstrap_out
  bootstrap_out=$(compose_exec server /workspace/target/debug/mosaicfs-server bootstrap --json)
  ACCESS_KEY_ID=$(echo "$bootstrap_out" | jq -r '.access_key_id')
  SECRET_KEY=$(echo "$bootstrap_out" | jq -r '.secret_key')
  export ACCESS_KEY_ID SECRET_KEY

  # Get a JWT token
  TOKEN=$(compose_exec server curl -sk "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${ACCESS_KEY_ID}\",\"secret_key\":\"${SECRET_KEY}\"}" \
    | jq -r '.token')
  export TOKEN BOOTSTRAPPED=1

  # Stop any running agents from the previous suite
  stop_agent agent-1 2>/dev/null || true
  stop_agent agent-2 2>/dev/null || true

  # Source and run the suite
  source "$matched"
}

for suite in "${SUITES[@]}"; do
  run_suite "$suite"
done

# ── Teardown ─────────────────────────────────────────────────────────────────

echo ""
echo "Tearing down compose stack..."
compose_down

# ── Summary ──────────────────────────────────────────────────────────────────

print_summary

if [ "$TESTS_FAILED" -gt 0 ]; then
  exit 1
fi
