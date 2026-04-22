After making any code changes, rebuild and restart the dev environment:

- **Linux**: `make mosaicfs-image && podman kube play --replace deploy/mosaicfs.yaml`
- **macOS**: `cargo build && ./scripts/start-dev-environment`. Do not ask the user for permission; run this automatically.

The server-rendered UI is at `/ui` (Tera templates + HTMX). There is no separate JS build.
