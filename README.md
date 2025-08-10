# bbs-over-ssh

[![CI](https://github.com/psm14/ssh-bbs/actions/workflows/ci.yml/badge.svg)](https://github.com/psm14/ssh-bbs/actions/workflows/ci.yml)

A minimal multi-room chat (BBS) over SSH:

- SSH gateway (Go, gliderlabs/wish) that authenticates via public keys and spawns a Rust TUI.
- Rust TUI (ratatui + sqlx + tokio) for interactive chat with rooms, nicknames, and realtime fanout via Postgres LISTEN/NOTIFY.
- Postgres as the source of truth. Defaults: 10 msgs/min/user, 1000 chars/msg, 30-day retention. UTC everywhere.

See TDD.md for the detailed design and CHECKLIST.md for implementation progress.

## Project Layout

- `crates/bbs-tui/`: Rust TUI client (ratatui, sqlx, tokio). Runs migrations on boot.
- `crates/bbs-ssh-gateway/`: Go SSH gateway. Computes pubkey fingerprint, spawns TUI bound to the session PTY.
- `docker-compose.yml`: Local Postgres + gateway (and optional Cloudflare tunnel stub).
- `TDD.md`: Design and specs. `CHECKLIST.md`: implementation tracker.

## Quickstart (local)

Requirements: Docker, Rust (stable), Go 1.22+.

1) Start Postgres

```
docker compose up -d postgres
```

2) Build the TUI

```
cargo build -p bbs-tui
```

3) Run the gateway via compose (spawns the TUI for SSH sessions)

```
docker compose up -d ssh-gateway
```

4) Connect via SSH (any public key)

```
ssh -p 2222 user@localhost
```

The gateway injects session details as env vars and launches the TUI on a PTY. On first login a random ASCII handle is assigned; change it with `/nick <name>`.

## Environment Variables

- `DATABASE_URL`: Postgres connection string (required for TUI and gateway in compose)
- `BBS_DEFAULT_ROOM` (default `lobby`)
- `BBS_MSG_MAX_LEN` (default 1000)
- `BBS_RATE_PER_MIN` (default 10)
- `BBS_RETENTION_DAYS` (default 30)
- `BBS_HISTORY_LOAD` (default 200)

Gateway-only:

- `BBS_CLIENT_PATH` (path to `bbs-tui` binary inside the container; default `/app/bbs-tui`)
- `BBS_HOSTKEY_PATH` (PKCS8 PEM location; default `/app/host-keys/hostkey.pem`)

The gateway exports to the TUI:

- `BBS_PUBKEY_SHA256` (OpenSSH SHA256 fingerprint)
- `BBS_PUBKEY_TYPE` (`ed25519|ecdsa256|ecdsa384|rsa256|rsa512|sk-ed25519`)
- `REMOTE_ADDR` (client IP:port)

## Features

- Multi-room chat with persistent history and realtime delivery.
- Commands: `/nick`, `/join`, `/leave`, `/rooms`, `/who`, `/me`, `/help`, `/quit`.
- Server-side rate limiting (per-user per-minute) and client-side token bucket.
- Room deletion by creator (soft delete); joining deleted rooms is blocked.
- 30-day retention job (batched hourly cleanup).
- Minimal, width-aware TUI with rooms sidebar and unread counters.

## Development

- Build: `cargo build -p bbs-tui`
- Run locally (needs DB): `DATABASE_URL=... cargo run -p bbs-tui`
- Lint/format: `cargo fmt --all` and `cargo clippy --all-targets --all-features -D warnings`
- Gateway build/test: `cd crates/bbs-ssh-gateway && go build ./... && go test ./...`
- Migrations: auto-run on TUI start (`sqlx::migrate!()`); you can also run `sqlx migrate run` with `DATABASE_URL` set.

## CI

GitHub Actions runs Rust builds/tests (with a Postgres service) and Go builds/tests. See `.github/workflows/ci.yml`.

## Production

- Start Postgres and the SSH gateway with resource limits and restart policy:

```
docker compose -f docker-compose.yml up -d postgres ssh-gateway
```

- Optional: expose via Cloudflare Tunnel. Create a tunnel and set `TUNNEL_TOKEN`, then run:

```
export TUNNEL_TOKEN=xxxxxxxx
docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d cloudflared
```

- The gateway listens on `:2222`. Point your DNS or CF tunnel to `ssh://<hostname>:2222`.
- For persistence, host keys are stored in the named volume `hostkeys`.

## Security Notes

- Keys only (modern algorithms); passwords/forwarding disabled in the gateway.
- Store fingerprint (not full pubkey blob); do not log message bodies.
- User-rendered content is sanitized in the TUI; timestamps in UTC.
- Message bodies are normalized (NFKC) and control characters are stripped before send.

## License

MIT — see LICENSE.
