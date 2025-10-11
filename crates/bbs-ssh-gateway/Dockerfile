FROM rust:1-bookworm AS builder
WORKDIR /work

# Install musl toolchain for static linking
RUN apt-get update \
    && apt-get install -y --no-install-recommends musl-tools pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Select musl target per architecture (supports amd64/arm64)
ARG TARGETARCH
RUN set -eux; \
    if [ "${TARGETARCH:-amd64}" = "arm64" ]; then \
      RUST_TRIPLE=aarch64-unknown-linux-musl; \
    else \
      RUST_TRIPLE=x86_64-unknown-linux-musl; \
    fi; \
    echo "$RUST_TRIPLE" > /triple; \
    rustup target add "$RUST_TRIPLE"

COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY crates/bbs-tui/Cargo.toml crates/bbs-tui/Cargo.toml
COPY crates/bbs-tui/src crates/bbs-tui/src
COPY crates/bbs-tui/migrations crates/bbs-tui/migrations
COPY crates/bbs-ssh-gateway/Cargo.toml crates/bbs-ssh-gateway/Cargo.toml
COPY crates/bbs-ssh-gateway/src crates/bbs-ssh-gateway/src
ENV RUSTFLAGS="-C target-feature=+crt-static"

RUN set -eux; \
    RUST_TRIPLE=$(cat /triple); \
    echo "Building for arch=${TARGETARCH:-amd64} triple=${RUST_TRIPLE}"; \
    cargo build --release --target "$RUST_TRIPLE" -p bbs-tui -p bbs-ssh-gateway; \
    mkdir -p /out; \
    cp "target/$RUST_TRIPLE/release/bbs-tui" /out/bbs-tui; \
    cp "target/$RUST_TRIPLE/release/bbs-ssh-gateway" /out/bbs-ssh-gateway
RUN mkdir -p /emptydir

FROM scratch AS runtime
WORKDIR /app
COPY --chown=65532:65532 --from=builder /emptydir/ /app/host-keys/
COPY --chown=65532:65532 --from=builder /out/bbs-ssh-gateway /app/bbs-ssh-gateway
COPY --chown=65532:65532 --from=builder /out/bbs-tui /app/bbs-tui
EXPOSE 2222
ENV BBS_CLIENT_PATH=/app/bbs-tui
USER 65532:65532
ENTRYPOINT ["/app/bbs-ssh-gateway"]
