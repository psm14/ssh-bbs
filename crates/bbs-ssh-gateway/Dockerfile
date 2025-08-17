# Multi-stage build: Rust TUI + Go SSH gateway

FROM rust:1-bookworm AS rust-builder
WORKDIR /work
# Install musl toolchain for static linking
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools pkg-config && rm -rf /var/lib/apt/lists/*
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
COPY crates/bbs-tui/Cargo.toml crates/bbs-tui/Cargo.toml
COPY crates/bbs-tui/src crates/bbs-tui/src
COPY crates/bbs-tui/migrations crates/bbs-tui/migrations
ENV RUSTFLAGS="-C target-feature=+crt-static"
# Build statically linked Rust binary (musl) for computed target
RUN set -eux; \
    RUST_TRIPLE=$(cat /triple); \
    echo "Building for arch=${TARGETARCH:-amd64} triple=${RUST_TRIPLE}"; \
    cargo build -p bbs-tui --release --target "$RUST_TRIPLE"; \
    mkdir -p /out; \
    cp "target/$RUST_TRIPLE/release/bbs-tui" /out/bbs-tui

FROM golang:1.22-bookworm AS go-builder
WORKDIR /work/crates/bbs-ssh-gateway
COPY crates/bbs-ssh-gateway/go.mod ./go.mod
COPY crates/bbs-ssh-gateway/go.sum ./go.sum
RUN go mod download
COPY crates/bbs-ssh-gateway/ .
ARG TARGETARCH
# Build static Go binary for the target arch
ENV CGO_ENABLED=0 GOOS=linux
RUN if [ "$TARGETARCH" = "arm64" ]; then export GOARCH=arm64; else export GOARCH=amd64; fi \
 && echo "Building Go for arch=$TARGETARCH GOARCH=$GOARCH" \
 && go build -tags netgo -ldflags "-s -w -extldflags '-static'" -o /out/bbs-ssh-gateway ./

# Final minimal image
FROM scratch AS runtime
WORKDIR /app
COPY --from=go-builder /out/bbs-ssh-gateway /app/bbs-ssh-gateway
COPY --from=rust-builder /out/bbs-tui /app/bbs-tui
EXPOSE 2222
ENV BBS_CLIENT_PATH=/app/bbs-tui
ENTRYPOINT ["/app/bbs-ssh-gateway"]
