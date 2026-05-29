FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

# System libraries for GTK4 + WebKitGTK 6 + layer-shell + build tooling.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        ca-certificates \
        curl \
        git \
        libssl-dev \
        libglib2.0-dev \
        libgtk-4-dev \
        libwebkitgtk-6.0-dev \
        libcairo2-dev \
        libpango1.0-dev \
        libgdk-pixbuf-2.0-dev \
        libgraphene-1.0-dev \
        libpulse-dev \
        libsoup-3.0-dev \
        libwayland-dev \
        wayland-protocols \
        libgirepository1.0-dev \
        valac \
        gir1.2-gtk-4.0 \
        gir1.2-webkit-6.0 \
        meson \
        ninja-build \
        gettext \
        just \
    && rm -rf /var/lib/apt/lists/*

# Ubuntu 24.04 lacks libgtk4-layer-shell-dev. Build from source (pinned tag).
ARG GTK4_LAYER_SHELL_VERSION=1.1.1
RUN git clone --depth 1 --branch v${GTK4_LAYER_SHELL_VERSION} https://github.com/wmww/gtk4-layer-shell.git /tmp/gtk4-layer-shell \
    && meson setup -Dexamples=false -Ddocs=false -Dtests=false /tmp/gtk4-layer-shell/build /tmp/gtk4-layer-shell \
    && meson install -C /tmp/gtk4-layer-shell/build \
    && ln -sf /usr/local/lib/x86_64-linux-gnu/pkgconfig/gtk4-layer-shell-0.pc /usr/local/lib/x86_64-linux-gnu/pkgconfig/gtk4-layer-shell.pc \
    && ldconfig \
    && rm -rf /tmp/gtk4-layer-shell

# Pinned Rust toolchain — rust-toolchain.toml in the repo will keep this in sync,
# but seed the image with the same version so first build is fast.
ARG RUST_VERSION=1.85.0
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile minimal --default-toolchain ${RUST_VERSION} \
        --component rustfmt --component clippy \
    && chmod -R a+rwX /usr/local/rustup /usr/local/cargo

# Node 20 LTS via NodeSource. Install pnpm globally via npm so it doesn't go
# through corepack — corepack tries to re-fetch on every invocation when
# $HOME is non-default, which can break offline use and pulls non-pinned
# versions through the network.
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g pnpm@9.15.0 \
    && rm -rf /var/lib/apt/lists/*

# Cargo target dir lives inside the container's persisted volume so rebuilds are fast.
# COREPACK_ENABLE_AUTO_PIN=0 stops corepack from trying to redirect pnpm calls.
ENV CARGO_TARGET_DIR=/workspaces/quantum/target \
    PKG_CONFIG_PATH=/usr/local/lib/x86_64-linux-gnu/pkgconfig:/usr/local/lib/pkgconfig \
    LD_LIBRARY_PATH=/usr/local/lib/x86_64-linux-gnu:/usr/local/lib \
    COREPACK_ENABLE_AUTO_PIN=0 \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0

WORKDIR /workspaces/quantum
