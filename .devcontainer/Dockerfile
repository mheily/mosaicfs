FROM debian:bookworm-slim

# Core build utilities + git + curl + doc-generation tools
# TODO: 3GB for pandoc..lmodern packages -- find a different tool?
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        git \
        curl \
        fuse3 \
        libfuse3-dev \
        bindfs \
        wget \
        ca-certificates \
        gnupg \
        lsb-release \
        pkg-config \
        sudo \
        pandoc \
        texlive-xetex \
        texlive-fonts-recommended \
        texlive-fonts-extra \
        texlive-latex-extra \
        lmodern \
    && rm -rf /var/lib/apt/lists/*

# Node.js (LTS) — useful for tooling; replace/remove once the stack is decided
RUN curl -fsSL https://deb.nodesource.com/setup_lts.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Rust toolchain (stable)
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile default \
    && rustup component add clippy rustfmt

# Install Claude Code globally
RUN curl -fsSL https://claude.ai/install.sh | bash && \
    ln -s /root/.local/bin/claude /usr/bin/claude

WORKDIR /workspace
