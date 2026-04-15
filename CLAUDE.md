After making any code changes, rebuild and redeploy:

- **Linux**: `make mosaicfs-image && podman kube play --replace deploy/mosaicfs.yaml`
- **macOS**: `make run-dev` (CouchDB container) + re-run `cargo run -p mosaicfs-server` on the host. See the "macOS development deployment" section of DEPLOYMENT.md.
