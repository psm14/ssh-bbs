# Repository Guidelines

## Project Structure & Modules
- `crates/bbs-tui/`: Rust TUI client (ratatui, sqlx, tokio). Contains `src/*.rs` and `migrations/` (schema + triggers).
- `crates/bbs-ssh-gateway/`: Go SSH gateway (wish/gliderlabs). Spawns the TUI with env from the SSH session.
- `docker-compose.yml`: Local Postgres + gateway + optional Cloudflare tunnel.
- Docs: `TDD.md` (design/spec), `CHECKLIST.md` (implementation tracker; update as you progress, order optional).

## Build, Test, and Dev Commands
- Rust build: `cargo build -p bbs-tui`
- Rust run (needs DB): `DATABASE_URL=... cargo run -p bbs-tui`
- Rust fmt/lint: `cargo fmt --all` and `cargo clippy --all-targets --all-features -D warnings`
- Go build: `cd crates/bbs-ssh-gateway && go build ./...`
- Go test: `cd crates/bbs-ssh-gateway && go test ./...`
- Postgres (local): `docker compose up -d postgres`
- Migrations: app runs `sqlx::migrate!()` on start; optionally use `sqlx migrate run` with `DATABASE_URL` set.

## Coding Style & Naming
- Indentation: 4 spaces; no tabs in Rust. Go uses gofmt defaults.
- Rust: idiomatic modules as in `TDD.md` (`ui.rs`, `input.rs`, `data.rs`, ...). Snake_case for files, lower_snake_case for fns/vars, UpperCamelCase for types.
- Go: package names lower_snake; keep files small and focused.
- Lint/format: rustfmt + clippy; gofmt (+ golangci-lint if configured).

## Testing Guidelines
- Unit tests: `cargo test -p bbs-tui` (parsers, validators, rate bucket). Place in `src/*` with `#[cfg(test)]` or `tests/`.
- Integration (DB): require `DATABASE_URL`; test migrations apply, user upsert, listen/notify.
- E2E: optional tmux/SSH script to validate fanout latency (<200ms median).
- Go: standard `go test ./...` for gateway session and key handling.

## Commit & PR Guidelines
- Commits: imperative, present tense; concise (<72 chars), e.g., "add tdd", "add implementation checklist" (see git log). Group logical changes.
- PRs: include summary, rationale, test notes, and any schema changes. Link issues. For schema, add a new file in `crates/bbs-tui/migrations/` and reference `TDD.md` updates.
- Progress: check items in `CHECKLIST.md` as you complete them; you may work out of order when practical.

## Security & Configuration
- Keys only: gateway rejects legacy algos; never log message bodies.
- Env vars: `DATABASE_URL`, `BBS_DEFAULT_ROOM`, `BBS_MSG_MAX_LEN`, `BBS_RATE_PER_MIN`, `BBS_RETENTION_DAYS`, `BBS_HISTORY_LOAD`.
- Defaults: UTC timestamps; sanitize/escape user-rendered content in the TUI.

