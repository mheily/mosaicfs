DOCKER ?= $(shell which podman || which docker)
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
mosaicfs-image:
	BUILDAH_ISOLATION=chroot $(DOCKER) build $(BUILD_FLAGS) -f Dockerfile.mosaicfs -t localhost/mosaicfs:latest .
