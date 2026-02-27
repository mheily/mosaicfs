DOCKER ?= $(shell which docker || which podman)
CONTEXT = Dockerfile web
BUILD_FLAGS =

image:
	tar cf - $(CONTEXT) | $(DOCKER) build -t mosaicfs-dev:latest -


# For use in storage-constrained environments where keeping large layers around
# during the build causes it to run out of space.
squashed-image:
	make image BUILD_FLAGS=--layers=false


# Production image: mosaicfs-server + mosaicfs-agent + web UI
# BUILDAH_ISOLATION=chroot bypasses rootless-Podman cgroup auth issues;
# Docker ignores it.
mosaicfs-image:
	BUILDAH_ISOLATION=chroot $(DOCKER) build $(BUILD_FLAGS) -f Dockerfile.mosaicfs -t localhost/mosaicfs:latest .
