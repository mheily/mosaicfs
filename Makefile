DOCKER ?= $(shell which podman || which container || which docker)
BUILD_FLAGS =


# Production image: mosaicfs-server + mosaicfs-agent.
# BUILDAH_ISOLATION=chroot bypasses rootless-Podman cgroup auth issues;
# Docker ignores it.
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

.PHONY: deploy-dev run-dev-database run-dev-server stop-dev

deploy-dev: mosaicfs-db-image

run-dev-database: deploy-dev
	@running=$$($(CONTAINER) list        2>/dev/null | grep -w mosaicfs-db || true); \
	existing=$$($(CONTAINER) list --all  2>/dev/null | grep -w mosaicfs-db || true); \
	if [ -n "$$running" ]; then \
	    echo "mosaicfs-db already running."; \
	elif [ -n "$$existing" ]; then \
	    echo "mosaicfs-db exists but not running; starting it."; \
	    $(CONTAINER) start mosaicfs-db >/dev/null; \
	else \
	    $(CONTAINER) run -d --name mosaicfs-db \
	        -p 127.0.0.1:5984:5984 \
	        -e COUCHDB_USER=admin \
	        -e COUCHDB_PASSWORD=changeme \
	        -v mosaicfs-couchdb-data:/opt/couchdb/data \
	        mosaicfs-db:latest; \
	fi
	@printf "Waiting for CouchDB on 127.0.0.1:5984"
	@for i in $$(seq 1 60); do \
	    if curl -fsS -o /dev/null http://127.0.0.1:5984/ 2>/dev/null; then \
	        echo " ready."; \
	        exit 0; \
	    fi; \
	    printf "."; \
	    sleep 1; \
	done; \
	echo; echo "ERROR: CouchDB did not come up on 127.0.0.1:5984 within 60s." >&2; exit 1

run-dev-server: run-dev-database
	COUCHDB_URL=http://127.0.0.1:5984 \
	COUCHDB_USER=admin \
	COUCHDB_PASSWORD=changeme \
	MOSAICFS_DATA_DIR=/tmp/mosaicfs-server-data \
	MOSAICFS_INSECURE_HTTP=1 \
	    cargo run -p mosaicfs-server

stop-dev:
	-$(CONTAINER) rm -f mosaicfs-db
