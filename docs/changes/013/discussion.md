# Change 013: Container network isolation

## Problem

The mosaicfs server and agent processes can make outbound network connections to
arbitrary hosts. The concern is data exfiltration: a compromised or misbehaving
process could send data to an attacker-controlled server. The server component
has no legitimate need for internet access at all; the agent's cloud-storage
replication is the only legitimate outbound use case.

## Approaches considered

### sandbox-exec (abandoned)

A Seatbelt profile (`desktop/mosaicfs-desktop.sb`) was written and tested.
`sandbox-exec` is officially deprecated on macOS and proved unreliable on
Darwin 25 (macOS 26): anonymous `mmap` for Rust thread stacks fails silently
under a deny-default profile with no log output, regardless of what rules are
added. The profile is kept as a resource-access audit document but is not used
for runtime enforcement.

### pf wrapper script

macOS `pf` cannot filter per-process — only per-UID. A wrapper that loaded
an anchor restricting outbound for the current user would affect all apps in
that session, not just mosaicfs. Running the app as a dedicated `_mosaicfs`
system user would allow per-UID filtering but breaks GUI apps (WindowServer
is tied to the logged-in user's session). A per-process pf approach is not
viable without Network Extensions, which require Apple entitlements.

### Apple container tool

Available on Darwin 25 (macOS 26 Tahoe). Each container runs in its own
lightweight VM with a dedicated IP and separate network stack (strong hardware-
level isolation). However, it has no built-in support for custom per-container
firewall rules or network policies. It also would require switching away from
the existing Podman deployment. Not pursued.

### Podman internal networks (recommended)

Podman (already used for the Linux deployment) supports `--internal` networks.
An internal network has no default gateway at the kernel level — containers on
it can reach each other but cannot route packets externally, regardless of what
the process attempts. This is enforced in the Linux VM's kernel routing table,
not in a user-space policy that the process could bypass.

## Recommended design

### Network topology

```
[host] ──── port 8443 ────► [mosaicfs-server pod]
                                      │
                              mosaicfs-internal (--internal, no gateway)
                                      │
                       ┌─────────────┴─────────────┐
                       │                           │
              [couchdb pod]              [mosaicfs-agent pod]
               (port 5984)
```

All three pods are on `mosaicfs-internal`. No pod has a default gateway.
CouchDB is not exposed to the host. Server port 8443 is still bound to the
host for admin UI access. No container can reach the internet.

### Why this works

- `podman network create --internal` sets `"internal": true` in the network
  config, which causes Podman to omit the default gateway route from the
  container's routing table. There is nothing for the process to override.
- Container name DNS (`aardvark-dns`) still works within the network, so
  server and agent reach CouchDB as `http://couchdb:5984` instead of
  `localhost:5984`.

### Trade-off

S3 and Backblaze B2 replication from the agent will not work with this design.
Accepted by the user — these backends are not in use or are acceptable to
disable in exchange for hard network isolation.

## Implementation outline

1. **Split `deploy/mosaicfs.yaml`** into three separate pod manifests:
   - `deploy/couchdb.yaml` — CouchDB container, no host port binding
   - `deploy/mosaicfs-server.yaml` — server binary, port 8443 bound to host
   - `deploy/mosaicfs-agent.yaml` — agent binary, no host port binding

2. **Add separate config files** (`/etc/mosaicfs/server.toml`,
   `/etc/mosaicfs/agent.toml`) that set the appropriate `[features]` block
   (`web_ui=true/agent=false` vs `agent=true/web_ui=false`) and point
   `[couchdb].url` at `http://couchdb:5984`.

3. **Update `Makefile`** Linux deploy target:
   ```sh
   podman network create --internal mosaicfs-internal 2>/dev/null || true
   podman kube play --replace --network mosaicfs-internal deploy/couchdb.yaml
   podman kube play --replace --network mosaicfs-internal deploy/mosaicfs-server.yaml
   podman kube play --replace --network mosaicfs-internal deploy/mosaicfs-agent.yaml
   ```

4. **Update `CLAUDE.md`** to reflect the new deploy sequence.

## Verification

```sh
# Network has no gateway
podman network inspect mosaicfs-internal | grep gateway   # should be empty

# CouchDB reachable from server container
podman exec <server-ctr> curl -f http://couchdb:5984      # should return CouchDB JSON

# Internet not reachable from server container
podman exec <server-ctr> curl --max-time 3 https://example.com  # should time out

# Admin UI still loads
curl -k https://localhost:8443                             # should return HTML
```
