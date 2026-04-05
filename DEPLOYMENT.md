# MosaicFS Deployment Guide

This guide covers the all-in-one deployment: CouchDB, the MosaicFS control
plane, and a local agent running together in a single pod, managed by
`podman kube play`. This is the recommended setup for a Linux NAS.

## Prerequisites

Build the image on the NAS before deploying:

```sh
podman build -f Dockerfile.mosaicfs -t localhost/mosaicfs:latest .
```

## Host configuration

All runtime configuration lives in `/etc/mosaicfs/` on the host. Create and
lock down the directory first:

```sh
sudo install -d -m 750 -o root -g mosaicfs /etc/mosaicfs
```

### 1. CouchDB credentials — `couchdb.env`

Create `/etc/mosaicfs/couchdb.env` with your chosen CouchDB username and
password. All three containers (CouchDB, server, agent) source this file on
startup.

```sh
cat <<'EOF' | sudo tee /etc/mosaicfs/couchdb.env
COUCHDB_URL=http://localhost:5984
COUCHDB_USER=admin
COUCHDB_PASSWORD=changeme
EOF
sudo chmod 600 /etc/mosaicfs/couchdb.env
```

### 2. Agent config — `agent.toml`

Create `/etc/mosaicfs/agent.toml`. The `access_key_id` and `secret_key` come
from the first-time bootstrap process — see
[First-time bootstrap](#first-time-bootstrap) below.

```sh
cat <<'EOF' | sudo tee /etc/mosaicfs/agent.toml
control_plane_url = "https://localhost:8443"
watch_paths = ["/data/mosaicfs-test"]
# excluded_paths = ["/data/mosaicfs-test/.cache"]

access_key_id = "MOSAICFS_..."
secret_key    = "..."
EOF
sudo chmod 600 /etc/mosaicfs/agent.toml
```

Update `watch_paths` and the `data` hostPath volume in `deploy/mosaicfs.yaml`
to point at the directory you want indexed.

## Deploy

```sh
podman kube play deploy/mosaicfs.yaml
```

To apply changes to the YAML or host config files:

```sh
podman kube play --replace deploy/mosaicfs.yaml
```

## First-time bootstrap

On the first run the MosaicFS server has no credentials in the database. It
generates a one-time bootstrap token, writes it to the server log, and waits
for it to be redeemed:

```sh
podman logs mosaicfs-mosaicfs-server | grep -i bootstrap
```

Open the web UI at `https://<nas-ip>:8443`. It will detect that bootstrap is
required and prompt for the token. Enter the token — the server will
immediately create the initial admin credential and return the `access_key_id`
and `secret_key`. Copy both values into `agent.toml`, then restart:

```sh
podman kube play --replace deploy/mosaicfs.yaml
```

## Accessing the MosaicFS web UI

The web UI is served by the MosaicFS server on port 8443:

```
https://<nas-ip>:8443
```

## Accessing the CouchDB admin UI (Fauxton)

CouchDB 3 ships **Fauxton** (the successor to the old Futon interface) at
`/_utils`. Port 5984 is bound to `127.0.0.1` only — it is not reachable
directly from your workstation. Use an SSH tunnel:

```sh
ssh -L 5984:localhost:5984 <nas-user>@<nas-ip>
```

Leave that session open, then open a browser on your workstation and navigate
to:

```
http://localhost:5984/_utils
```

Log in with the `COUCHDB_USER` and `COUCHDB_PASSWORD` from `couchdb.env`.

## Updating credentials

**CouchDB password** — edit `/etc/mosaicfs/couchdb.env` on the host, then
redeploy:

```sh
podman kube play --replace deploy/mosaicfs.yaml
```

**Agent access key** — edit `/etc/mosaicfs/agent.toml`, then redeploy the
same way.

## Stopping

```sh
podman kube play --down deploy/mosaicfs.yaml
```

This stops and removes the pod but leaves the persistent volumes intact. Data
is preserved across stop/start cycles.
