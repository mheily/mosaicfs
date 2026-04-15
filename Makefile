DOCKER ?= $(shell which podman || which container || which docker)
CONTEXT = Dockerfile web
BUILD_FLAGS =

image:
	tar cf - $(CONTEXT) | $(DOCKER) build -t mosaicfs-dev:latest -


# For use in storage-constrained environments where keeping large layers around
# during the build causes it to run out of space.
squashed-image:
	make image BUILD_FLAGS=--layers=false


ts-types:
	cargo test -p mosaicfs-common --lib
	rm -rf web/src/types/generated/*.ts web/src/types/generated/serde_json
	cp mosaicfs-common/bindings/*.ts web/src/types/generated/
	cp -r mosaicfs-common/bindings/serde_json web/src/types/generated/

check-types-fresh:
	$(MAKE) ts-types
	git diff --exit-code web/src/types/generated/


# Production image: mosaicfs-server + mosaicfs-agent + web UI
# BUILDAH_ISOLATION=chroot bypasses rootless-Podman cgroup auth issues;
# Docker ignores it.
tauri-dev:
	cd web && npm run tauri:dev

tauri-build:
	cd web && npm run tauri:build


mosaicfs-image:
	BUILDAH_ISOLATION=chroot $(DOCKER) build $(BUILD_FLAGS) -f Dockerfile.mosaicfs -t mosaicfs:latest .

# ── Apple `container` dev deployment ─────────────────────────────────────────
# CouchDB runs in a container (port-forwarded to 127.0.0.1:5984). The
# Rust server and agent run on the host (via `cargo run`) and talk to
# CouchDB over localhost, sidestepping Apple `container`'s lack of
# automatic inter-container DNS.

CONTAINER ?= /usr/local/bin/container

mosaicfs-db-image:
	$(CONTAINER) build -f Dockerfile.mosaicfs-db -t mosaicfs-db:latest .

.PHONY: deploy-dev run-dev stop-dev

deploy-dev: mosaicfs-db-image

run-dev: deploy-dev
	$(CONTAINER) run -d --name mosaicfs-db \
	    -p 127.0.0.1:5984:5984 \
	    -e COUCHDB_USER=admin \
	    -e COUCHDB_PASSWORD=changeme \
	    -v mosaicfs-couchdb-data:/opt/couchdb/data \
	    mosaicfs-db:latest
	@echo
	@echo "CouchDB up at http://127.0.0.1:5984 (admin/changeme)."
	@echo "Now run the server on the host:"
	@echo "  COUCHDB_URL=http://127.0.0.1:5984 COUCHDB_USER=admin COUCHDB_PASSWORD=changeme \\"
	@echo "    cargo run -p mosaicfs-server"

stop-dev:
	-$(CONTAINER) rm -f mosaicfs-db
