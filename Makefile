DOCKER ?= $(shell which docker)

image:
	tar cf - Dockerfile web | $(DOCKER) build -t mosaicfs-dev:latest -
