# Multi-stage build: Rust TUI + Go SSH gateway

FROM rust:1-bookworm AS rust-builder
WORKDIR /work
COPY Cargo.toml Cargo.toml
COPY crates/bbs-tui/Cargo.toml crates/bbs-tui/Cargo.toml
COPY crates/bbs-tui/src crates/bbs-tui/src
COPY crates/bbs-tui/migrations crates/bbs-tui/migrations
RUN cargo build -p bbs-tui --release

FROM golang:1.22-bookworm AS go-builder
WORKDIR /work
COPY crates/bbs-ssh-gateway/go.mod crates/bbs-ssh-gateway/go.mod
COPY crates/bbs-ssh-gateway/main.go crates/bbs-ssh-gateway/main.go
WORKDIR /work/crates/bbs-ssh-gateway
RUN go build -o /out/bbs-ssh-gateway ./

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=go-builder /out/bbs-ssh-gateway /app/bbs-ssh-gateway
COPY --from=rust-builder /work/target/release/bbs-tui /app/bbs-tui
EXPOSE 2222
ENV BBS_CLIENT_PATH=/app/bbs-tui
ENTRYPOINT ["/app/bbs-ssh-gateway"]

