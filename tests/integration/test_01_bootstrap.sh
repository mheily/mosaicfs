#!/usr/bin/env bash
# Suite 1: Bootstrap and Authentication

suite_header "Suite 1: Bootstrap and Authentication"

# ── Tests ────────────────────────────────────────────────────────────────────

test_bootstrap_creates_credential() {
  assert_contains "$ACCESS_KEY_ID" "MOSAICFS_" "access_key_id format"
  assert_contains "$SECRET_KEY" "mosaicfs_" "secret_key format"

  # Verify the credential document exists in CouchDB
  local doc
  doc=$(compose_exec couchdb curl -s -u admin:testpassword \
    "http://localhost:5984/mosaicfs/credential::${ACCESS_KEY_ID}")
  local doc_type
  doc_type=$(echo "$doc" | jq -r '.type')
  assert_eq "$doc_type" "credential" "credential doc type in CouchDB"
}

test_bootstrap_rejects_when_credentials_exist() {
  if compose_exec server /workspace/target/debug/mosaicfs-server bootstrap --json 2>/dev/null; then
    echo "Expected bootstrap to fail but it succeeded" >&2
    return 1
  fi
  return 0
}

test_jwt_login() {
  local response
  response=$(compose_exec server curl -sk "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${ACCESS_KEY_ID}\",\"secret_key\":\"${SECRET_KEY}\"}")
  local token
  token=$(echo "$response" | jq -r '.token')
  assert_ne "$token" "null" "token should not be null"
  assert_ne "$token" "" "token should not be empty"

  local whoami
  whoami=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${token}" \
    "https://localhost:8443/api/auth/whoami")
  local key_id
  key_id=$(echo "$whoami" | jq -r '.access_key_id // .key_id // .sub')
  assert_contains "$key_id" "MOSAICFS_" "whoami returns credential identity"
}

test_jwt_login_bad_password() {
  local status
  status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${ACCESS_KEY_ID}\",\"secret_key\":\"wrong_secret\"}")
  assert_eq "$status" "401" "bad password should return 401"
}

test_credential_crud() {
  # Create a new credential
  local create_resp
  create_resp=$(compose_exec server curl -sk -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"name":"test-agent"}' \
    "https://localhost:8443/api/credentials")
  local new_key_id
  new_key_id=$(echo "$create_resp" | jq -r '.access_key_id')
  local new_secret
  new_secret=$(echo "$create_resp" | jq -r '.secret_key')
  assert_contains "$new_key_id" "MOSAICFS_" "new credential has valid key_id"

  # List — response is {"items": [...], "total": N}
  local list_resp
  list_resp=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/credentials")
  local count
  count=$(echo "$list_resp" | jq '.total')
  assert_eq "$count" "2" "should have 2 credentials"

  # Disable the new credential
  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"enabled":false}' \
    "https://localhost:8443/api/credentials/${new_key_id}" >/dev/null

  # Verify login fails with disabled credential
  local status
  status=$(compose_exec server curl -sk -o /dev/null -w "%{http_code}" \
    "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${new_key_id}\",\"secret_key\":\"${new_secret}\"}")
  assert_eq "$status" "401" "disabled credential login should fail"

  # Re-enable
  compose_exec server curl -sk -X PATCH \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"enabled":true}' \
    "https://localhost:8443/api/credentials/${new_key_id}" >/dev/null

  # Verify login works again
  local relogin
  relogin=$(compose_exec server curl -sk \
    "https://localhost:8443/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"access_key_id\":\"${new_key_id}\",\"secret_key\":\"${new_secret}\"}")
  local retoken
  retoken=$(echo "$relogin" | jq -r '.token')
  assert_ne "$retoken" "null" "re-enabled credential should login"

  # Delete the credential
  compose_exec server curl -sk -X DELETE \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/credentials/${new_key_id}" >/dev/null

  # Verify list is back to 1 — check .total field
  list_resp=$(compose_exec server curl -sk \
    -H "Authorization: Bearer ${TOKEN}" \
    "https://localhost:8443/api/credentials")
  count=$(echo "$list_resp" | jq '.total')
  assert_eq "$count" "1" "should have 1 credential after delete"
}

# ── Run ──────────────────────────────────────────────────────────────────────

run_test test_bootstrap_creates_credential
run_test test_bootstrap_rejects_when_credentials_exist
run_test test_jwt_login
run_test test_jwt_login_bad_password
run_test test_credential_crud
