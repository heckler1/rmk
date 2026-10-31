# Firmware build environment used by scripts/docker.sh.
# The `esp` stage adds the Xtensa toolchain, which only esp32s3 needs.

FROM debian:trixie-slim AS base

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUST_MIN_STACK=67108864 \
    CARGO_NET_GIT_FETCH_WITH_CLI=true

# gcc-arm-none-eabi and libnewlib-dev: cc-rs needs them for trouble's P-256 assembly on nRF.
# clang and libclang-dev: bindgen.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    clang \
    curl \
    gcc-arm-none-eabi \
    git \
    libclang-dev \
    libnewlib-dev \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none

COPY rust-toolchain.toml /tmp/toolchain/rust-toolchain.toml
RUN cd /tmp/toolchain && rustup toolchain install && rustup default stable

RUN curl --proto '=https' --tlsv1.2 -LsSf \
    https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
    | bash
RUN cargo binstall -y --locked cargo-make cargo-binutils cargo-hex-to-uf2 flip-link espflash \
    && rm -rf "$CARGO_HOME/registry" "$CARGO_HOME/git"

WORKDIR /workspace

FROM base AS esp

RUN cargo binstall -y --locked espup \
    && espup install --targets esp32s3 --export-file /usr/local/export-esp.sh \
    && rm -rf "$CARGO_HOME/registry" "$CARGO_HOME/git"
