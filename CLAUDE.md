After making any code changes, rebuild and redeploy:

- **Linux**: `make mosaicfs-image && podman kube play --replace deploy/mosaicfs.yaml`
- **macOS**: `make run-dev-server` (starts CouchDB container + `cargo run -p mosaicfs`). See the "macOS development deployment" section of DEPLOYMENT.md.

The server-rendered UI is at `/ui` (Tera templates + HTMX). There is no separate JS build.
