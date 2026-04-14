# Architecture Change 007: Secrets Manager / Keychain Backend

This change introduces a `secrets_manager` config key with two backends:
`"inline"` (default, all platforms — secrets live in the TOML file as
today) and `"keychain"` (macOS, secrets live in the macOS Keychain via
the `keyring` crate). The motivating use case is macOS App Sandbox
compliance and notarization, which forbid plaintext credentials in
distributed configuration.

Depends on change 006 (the unified `mosaicfs.toml` schema must exist
before adding a top-level `secrets_manager` key to it).

## Current State Summary

_Verified against the tree at the head of `master`. After change 006
lands, the unified config will look as described in
`docs/changes/006/architecture.md`._

**Today's secrets** (post-006 layout):

- `[credentials].access_key_id` and `[credentials].secret_key` — the
  agent's MosaicFS access credential, used to authenticate API calls
  this node makes to other nodes.
- `[couchdb].user` / `[couchdb].password` — CouchDB admin credentials.
- OAuth tokens for replication backends (Google Drive, etc.) — currently
  stored as documents in CouchDB after the OAuth flow completes. The
  OAuth client secret is in the unified config.

All of the above currently live in either the TOML file or env vars.
There is no abstraction over secret retrieval — code reads
`config.credentials.secret_key` directly.

**macOS posture:** MosaicFS does not currently target the macOS App
Sandbox or notarization. macFUSE itself precludes a fully-sandboxed
distribution today, but the project decisions document calls out that
notarization compliance becomes relevant if/when MosaicFS ships a
signed installer for macOS users (independent of the FileProvider
deferral, since the agent and web_ui can be notarized separately from
the FUSE component).

## Goal

Add an indirection between "the code that needs a secret" and "where
the secret is stored," so that on macOS the secret can live in the
Keychain and never appear in any file MosaicFS distributes or the user
edits. On other platforms, behavior is unchanged: secrets live inline in
the TOML.

## Changes

### Change A — `secrets_manager` config key with two backends

**Today:** Secrets are read directly from the unified config struct
(post-006). No abstraction.

**Proposed:** Add to `mosaicfs.toml`:

```toml
[secrets]
manager = "inline"   # default; or "keychain" on macOS
```

Define a `SecretsBackend` trait with `get(name) -> Result<String>` and
`set(name, value) -> Result<()>`. Two implementations:

- `InlineBackend` — wraps the existing `[credentials]` and
  `[couchdb]` blocks. `get("credentials.secret_key")` returns
  `config.credentials.secret_key`. `set` rewrites the TOML file (or
  errors if the file is read-only).
- `KeychainBackend` — uses the `keyring` crate, with service name
  `"mosaicfs"` and the secret name as the key. Only available when
  built on macOS (cfg-gated). `set` writes to the Keychain;
  `get` reads from it.

Code that needs a secret calls `secrets.get("credentials.secret_key")`
rather than reading the config struct directly. The set of secret
names is fixed and enumerated in one place.

**Justification:** A two-line abstraction (trait + two impls) is enough
to remove plaintext secrets from the macOS distribution. Building it
behind a config key keeps the default behavior unchanged for Linux and
container deployments where the existing model is fine.

### Change B — Bootstrap and migration flow for keychain mode

**Today:** Secrets land in the config file when the user runs
`mosaicfs-server bootstrap` and pastes the resulting access key into
`agent.toml`.

**Proposed:** When `secrets.manager = "keychain"`, the bootstrap path
writes the generated credentials into the Keychain via `set` instead of
printing them for paste. A `mosaicfs secrets import` subcommand reads an
existing inline TOML, calls `set` for each secret, and (if the user
confirms) blanks the secret fields in the file. A `mosaicfs secrets list`
subcommand lists which keys are present in the active backend (without
revealing values).

OAuth flows that currently store the resulting refresh token as a CouchDB
document continue to do so — those tokens are per-backend-target, not
per-node, and live in shared metadata. Only node-level secrets move to
the keychain.

**Justification:** Without a migration command, switching modes means
hand-editing files and remembering which secrets to copy where. A scripted
import + a list command makes the mode switch a one-shot operation.

## Implementation Phases

**Phase 1 — Trait and inline backend.**
Define `SecretsBackend` in `mosaicfs-common::secrets`. Implement
`InlineBackend` as a wrapper over the existing config struct. Update all
secret-reading code sites to call through the trait. No behavior change
yet — `InlineBackend` is the only impl, selected unconditionally.

**Phase 2 — Add the `secrets.manager` config key.**
Extend the unified config schema with `[secrets].manager`. Default
`"inline"`. Validate values (`"inline"` everywhere; `"keychain"` only on
macOS — error at startup with a clear message on other platforms).

**Phase 3 — Keychain backend.**
Add the `keyring` dependency (cfg-gated to macOS). Implement
`KeychainBackend`. Wire it up so that `secrets.manager = "keychain"`
selects it. Test on macOS: bootstrap, restart, secret retrieval all work
without any plaintext credentials in the config file.

**Phase 4 — Migration commands.**
Add `mosaicfs secrets import`, `mosaicfs secrets list`, and
`mosaicfs secrets get NAME` (the last gated behind a confirmation, for
recovery scenarios). Document the macOS-distribution workflow in
`DEPLOYMENT.md`.

**Phase dependencies:**

- Phase 2 requires Phase 1 (the config key has nothing to select between
  without two backends to choose from — but the second backend lands in
  Phase 3, so Phase 2 can land with `keychain` returning a
  "not yet implemented" error if that simplifies review).
- Phase 3 requires Phase 1 + Phase 2.
- Phase 4 requires Phase 3.

## What Does Not Change

- **Document model and CouchDB schema.** No new doc types. OAuth refresh
  tokens for backend targets continue to live in CouchDB documents
  unchanged.
- **REST API surface.** No new routes; the secrets layer is internal.
- **Auth wire format.** Access key + HMAC continues to gate the API. The
  only change is where the agent's secret-key value comes from when it
  signs requests.
- **Default deployment.** Linux containers continue to read secrets from
  TOML or env vars. The pod manifest does not change.
- **Non-macOS platforms.** `secrets.manager = "keychain"` is rejected at
  startup. Users on Linux/Windows see no behavior change.
- **Unified binary, Loco UI, code consolidation, no-transport
  (changes 003–006).** All untouched. This change adds one indirection
  on top of the config schema 006 defined.
- **CouchDB credentials handling for the embedded process.** CouchDB
  itself continues to be configured via env vars in the pod manifest;
  the keychain backend stores the URL/user/password MosaicFS uses to
  reach it, but does not change how CouchDB authenticates internally.

## Deferred

- **Linux Secret Service / GNOME Keyring / KWallet backends.** The
  `keyring` crate supports them, but Linux deployments are
  container-first and have no equivalent of the App Sandbox driver
  forcing the change. Add when there is demand.
- **Windows Credential Manager backend.** Same reasoning.
- **HashiCorp Vault / cloud KMS backends.** Out of scope. Inline +
  keychain covers the v1 distribution targets.
- **Encrypted TOML at rest.** Not the same problem; keychain mode
  removes the secrets entirely rather than encrypting them in place.
- **Per-secret rotation policy.** The trait supports `set`, so rotation
  is mechanically possible, but a rotation workflow (re-issue access
  key, update CouchDB record, restart node) is deferred.
- **App Sandbox entitlements and notarization workflow.** This change
  removes the "plaintext secrets in distributed config" blocker; the
  remaining sandbox/notarization work (entitlements, signing, stapling)
  is its own change once a macOS distribution path is being built.
- **macFUSE FileProvider replacement and redb.** Both deferred to v2 per
  project decisions.
- **Hiding OAuth refresh tokens from CouchDB.** Per-backend OAuth
  tokens continue to live in CouchDB documents (they are per-target,
  not per-node, and need to federate). Moving them to per-node keychains
  would break federation; out of scope.
