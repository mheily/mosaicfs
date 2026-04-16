FROM docker.io/debian:bookworm-slim

# Core build utilities + FUSE (mosaicfs-vfs) + Rust toolchain.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        git \
        ca-certificates \
        curl \
        fuse3 \
        libfuse3-dev \
        bindfs \
        pkg-config \
        procps \
        sudo \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile default \
    && rustup component add clippy rustfmt

WORKDIR /workspace
