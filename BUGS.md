# Known Bugs

## Mismatch between frontend and backend field names

There has been a pattern of frontend documents not matching the Rust backend expectations.
Example: node_id vs node. We should do a full audit of the architecture doc and codebase
to make everything consistent.

## TLS certificate verification disabled in `mosaicfs-agent init`

**File:** `mosaicfs-agent/src/init.rs:129`

The `init` subcommand builds its reqwest client with `danger_accept_invalid_certs(true)`,
silently accepting any certificate when contacting the control plane during setup.
This exposes the access key and secret key to a MitM attack at enrollment time.

The fix is to use `Client::new()` (verification on by default) and, if self-signed
certs are needed, accept a user-supplied CA certificate path instead.
