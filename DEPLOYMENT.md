# MosaicFS Deployment Guide

This guide covers the all-in-one deployment: CouchDB and the unified
`mosaicfs` binary running together in a single pod, managed by
`podman kube play`. This is the recommended setup for a Linux NAS.

## Node roles

Each node runs the same `mosaicfs` binary. Its role is selected by the
`[features]` block in `mosaicfs.toml`:

| Role               | `[features]`                                      | Typical host           |
|--------------------|---------------------------------------------------|------------------------|
| NAS (admin + data) | `agent = true, web_ui = true`                     | Linux NAS serving UI   |
| Laptop (consumer)  | `agent = true, vfs = true`                        | Workstation with FUSE  |
| Headless indexer   | `agent = true`                                    | Secondary data host    |

See the per-role example configs below.

## Prerequisites

Build the image on the NAS before deploying:

```sh
podman build -f Dockerfile.mosaicfs -t localhost/mosaicfs:latest .
```

## Host configuration

All runtime configuration lives in `/etc/mosaicfs/` on the host. Create
and lock down the directory first:

```sh
sudo install -d -m 750 -o root -g mosaicfs /etc/mosaicfs
```

### 1. CouchDB credentials — `couchdb.env`

Create `/etc/mosaicfs/couchdb.env` with the CouchDB username and
password. The CouchDB container sources this on startup and the
`mosaicfs` container inherits it as env overrides for
`[couchdb].user` / `[couchdb].password`.

```sh
cat <<'EOF' | sudo tee /etc/mosaicfs/couchdb.env
COUCHDB_USER=admin
COUCHDB_PASSWORD=changeme
EOF
sudo chmod 600 /etc/mosaicfs/couchdb.env
```

### 2. Node config — `mosaicfs.toml`

Copy the example file and edit it for the role this node should play:

```sh
sudo cp mosaicfs.toml.example /etc/mosaicfs/mosaicfs.toml
sudo chmod 600 /etc/mosaicfs/mosaicfs.toml
```

#### Example: NAS (admin UI + local crawler)

```toml
[features]
agent  = true
vfs    = false
web_ui = true

[agent]
watch_paths    = ["/data/mosaicfs-test"]
excluded_paths = []

[web_ui]
listen = "0.0.0.0:8443"

[couchdb]
url      = "http://localhost:5984"
user     = "admin"
# password provided via COUCHDB_PASSWORD env var
```

#### Example: laptop (consumer with FUSE mount)

```toml
[features]
agent  = true
vfs    = true
web_ui = false

[agent]
watch_paths = ["/Users/alice/Documents"]

[vfs]
mount_point = "/Users/alice/mnt/mosaicfs"

[couchdb]
url      = "https://nas.internal:5984"
user     = "alice"
# password via COUCHDB_PASSWORD
```

#### Example: headless indexer

```toml
[features]
agent  = true

[agent]
watch_paths = ["/srv/archive"]

[couchdb]
url      = "https://nas.internal:5984"
user     = "indexer-01"
# password via COUCHDB_PASSWORD
```

Update `watch_paths` and the `data` hostPath volume in
`deploy/mosaicfs.yaml` to point at the directory you want indexed.

## Deploy

```sh
podman kube play deploy/mosaicfs.yaml
```

To apply changes to the YAML or host config files:

```sh
podman kube play --replace deploy/mosaicfs.yaml
```

## First-time bootstrap

Before the admin UI can be used it needs an initial credential. On the
first run the `mosaicfs` binary writes a one-time bootstrap token to
its log; open the web UI, go through the bootstrap prompt, and the
first credential is created for you.

```sh
podman logs mosaicfs-mosaicfs | grep -i bootstrap
```

Alternatively run the `bootstrap` subcommand directly to print an
access key / secret key pair:

```sh
podman exec mosaicfs-mosaicfs \
    /usr/local/bin/mosaicfs bootstrap --config /etc/mosaicfs/mosaicfs.toml
```

## Accessing the MosaicFS web UI

On a node with `features.web_ui = true`:

```
https://<node-ip>:8443
```

## Accessing the CouchDB admin UI (Fauxton)

CouchDB 3 ships **Fauxton** at `/_utils`. Port 5984 is bound to
`127.0.0.1` only — it is not reachable directly from your workstation.
Use an SSH tunnel:

```sh
ssh -L 5984:localhost:5984 <nas-user>@<nas-ip>
```

Leave that session open, then open a browser on your workstation and
navigate to:

```
http://localhost:5984/_utils
```

Log in with the `COUCHDB_USER` and `COUCHDB_PASSWORD` from
`couchdb.env`.

## Updating credentials

**CouchDB password** — edit `/etc/mosaicfs/couchdb.env` on the host,
then redeploy:

```sh
podman kube play --replace deploy/mosaicfs.yaml
```

**Admin credentials** — use the Settings → Credentials page in the
admin UI to create, disable, or rotate access keys.

## macOS Keychain-backed secrets

On macOS, node-level secrets (CouchDB URL/user/password and the node's
own access key + secret key) can live in the macOS Keychain instead of
the TOML file. This is required for App Sandbox / notarization-bound
distributions, and useful any time you'd rather not store plaintext
credentials in `mosaicfs.toml`.

### Switching an existing node to the keychain

1. Set the backend in `mosaicfs.toml`:

   ```toml
   [secrets]
   manager = "keychain"
   ```

2. Import the existing inline values into the keychain and blank the
   file fields:

   ```sh
   mosaicfs secrets import --config /path/to/mosaicfs.toml
   ```

   The command prints every secret it migrated, then asks whether to
   blank the source fields in the file. Answer `y` to finish the
   migration, or `n` to leave the file alone and edit it manually.
   Pass `--yes` to skip both prompts.

3. Verify the backend is live:

   ```sh
   mosaicfs secrets list --config /path/to/mosaicfs.toml
   ```

4. Restart `mosaicfs`. The agent/web_ui reads every secret from the
   keychain on startup.

### Subcommands

- `mosaicfs secrets list` — print the names of every secret currently
  present in the active backend. Values are never shown.
- `mosaicfs secrets get NAME [--yes]` — print one secret value to
  stdout. Gated behind a `[y/N]` confirmation unless `--yes` is passed.
  Intended for recovery, not routine use.
- `mosaicfs secrets import [--yes]` — as described above.

### Known secret names

```
couchdb.url
couchdb.user
couchdb.password
credentials.access_key_id
credentials.secret_key
```

### Keychain service

Every entry lives under service name `mosaicfs` in the user's default
keychain, keyed by the fully-qualified secret name. You can inspect
them with Keychain Access (search for `mosaicfs`) or via the `security`
CLI.

### Not available on Linux/Windows

`[secrets].manager = "keychain"` is rejected at startup on non-macOS
platforms. Use env-var overrides
(`COUCHDB_PASSWORD`, `MOSAICFS_SECRET_KEY`) to keep secrets out of the
file on container deployments.

## macOS development deployment

The podman/kube workflow doesn't apply on macOS. Instead, run CouchDB
in Apple's native `container` CLI and run `mosaicfs` on the host via
`cargo run`.

### Prerequisites

- Apple `container` CLI at `/usr/local/bin/container`
- `libfuse`/`pkg-config` via Homebrew (for building `mosaicfs-vfs`)

### One-shot

```sh
make run-dev-server   # builds mosaicfs-db, starts it on 127.0.0.1:5984,
                      # auto-generates dev-config/mosaicfs.toml (web_ui
                      # only, insecure_http, loopback), and runs
                      # `cargo run -p mosaicfs` against it.
```

The admin UI is then at `http://127.0.0.1:8443/admin`. On first run
the bootstrap token is printed to stdout — paste it into the
`/admin/bootstrap` page to create the initial credential.

To tear it down: `make stop-dev`.

> `insecure_http = true` is dev-only. Never set it on a shared or
> remote deployment.

## Stopping

```sh
podman kube play --down deploy/mosaicfs.yaml
```

This stops and removes the pod but leaves the persistent volumes
intact. Data is preserved across stop/start cycles.
