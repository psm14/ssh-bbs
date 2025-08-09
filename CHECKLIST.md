# bbs-over-ssh: implementation plan (checklist)

## m0 — repo + tooling

* [x] create mono-repo

  * [ ] `mkdir bbs-over-ssh && cd bbs-over-ssh`
  * [ ] `git init -b main`
* [x] rust workspace

  * [x] `cargo new --bin crates/bbs-tui`
  * [x] top-level `Cargo.toml` `[workspace]` with `members = ["crates/bbs-tui"]`
* [x] go ssh gateway

  * [x] `mkdir -p crates/bbs-ssh-gateway && cd crates/bbs-ssh-gateway`
  * [x] `go mod init github.com/you/bbs-ssh-gateway`
* [x] toolchain + lint

  * [x] rust: `rustup override set stable` in repo
  * [ ] add rust `clippy` + `rustfmt` configs
  * [ ] go: `gofmt` + `golangci-lint` (optional)
* [ ] license + readme
* [x] `.editorconfig`, `.gitignore` (target/, node\_modules/ just in case)

## m1 — database + migrations

* [x] dockerized postgres 16

  * [x] add `docker-compose.yml` with `postgres:16` and healthcheck
* [x] rust db deps in `bbs-tui`

  * [x] `sqlx = { version = "0.7", features = ["runtime-tokio-rustls","postgres","macros","uuid","chrono","migrate"] }`
  * [x] `tokio`, `tracing`, `serde`, `rand`, `unicode-segmentation`
* [x] create migration `crates/bbs-tui/migrations/0001_init.sql` containing:

  * [x] `users`, `rooms`, `room_members`, `messages`, `name_changes`, `bans` tables
  * [x] indexes as designed
  * [x] `messages_notify` trigger + `notify_new_message()` function
* [x] `DATABASE_URL` wiring (env)
* [x] migration runner

  * [x] on startup, `sqlx::migrate!().run(&pool).await?`
* [x] seed default room `lobby` if missing

## m2 — tui foundation (non-network)

* [ ] add crates: `ratatui`, `crossterm`, `tui-textarea` (or diy), `anyhow` (for ergonomics)
* [x] boot skeleton

  * [x] read env: `BBS_PUBKEY_SHA256`, `BBS_PUBKEY_TYPE`, `REMOTE_ADDR`, `DATABASE_URL`, `BBS_DEFAULT_ROOM`
  * [x] establish db pool
  * [x] upsert user by fingerprint; create random ascii handle if new
* [ ] layout

  * [ ] main messages pane
  * [ ] right sidebar (rooms + unread)
  * [ ] bottom input line
  * [ ] statusline (nick, room, token bucket, fp short)
* [ ] input loop

  * [ ] key handling: enter/pgup/pgdn/tab/esc/ctrl+c
  * [ ] slash command parser stub
* [ ] rendering helpers

  * [ ] timestamp `[hh:mm:ss]` utc
  * [ ] escape ansi/control chars in message bodies

## m3 — realtime via listen/notify

* [ ] add pg listener task

  * [ ] `LISTEN room_events;` using `sqlx::postgres::PgListener`
* [ ] payload model

  * [ ] json `{"t":"msg","room_id":<id>,"id":<id>}`
* [ ] on receive:

  * [ ] if joined to room: `select * from messages where id=$1`
  * [ ] append to buffer, update unread if room not focused
* [ ] reconnect strategy

  * [ ] exponential backoff on listener error
  * [ ] fallback poll every 2s for `created_at > last_seen`

## m4 — commands + validations

* [ ] `/nick <name>`

  * [ ] validate ascii `[a-z0-9_-]{2,16}`
  * [ ] `update users set handle=$new; insert into name_changes(...)`
  * [ ] unique constraint error → show message
* [ ] `/join <room>`

  * [ ] validate `[a-z0-9_-]{1,24}`
  * [ ] upsert room (with `created_by = current_user`)
  * [ ] upsert `room_members`
  * [ ] load last `BBS_HISTORY_LOAD` messages ordered desc then render
* [ ] `/leave [room]` (no db delete; just ui focus/unsubscribe)
* [ ] `/rooms` list
* [ ] `/who [room]` (recent active = `last_joined_at` or last message timestamp)
* [ ] `/me <action>` (client-side formatting)
* [ ] `/help`, `/quit`
* [ ] message send path

  * [ ] trim + reject empty
  * [ ] enforce `≤ BBS_MSG_MAX_LEN`

## m5 — server-side rate limiting (pg-only)

* [ ] config env: `BBS_RATE_PER_MIN` (default 10)
* [ ] `insert ... where` gate:

  * [ ] cte counts recent per user in last minute
  * [ ] only insert if `< rate`
* [ ] client-side token bucket

  * [ ] burst = rate
  * [ ] ui indicator for remaining tokens
* [ ] friendly error on 429 condition (custom sqlx error mapping)

## m6 — room deletion by creator

* [ ] room model: `created_by`, `is_deleted`, `deleted_at`
* [ ] `/roomdel <name>`

  * [ ] ensure `created_by == me`
  * [ ] `update rooms set is_deleted=true, deleted_at=now() where ... and is_deleted=false`
  * [ ] prevent joins to `is_deleted` rooms
  * [ ] exclude from `/rooms` listing
* [ ] acceptance: creator can delete; others get permission error

## m7 — ssh gateway (go, wish)

* [x] deps: `gliderlabs/ssh`, `golang.org/x/crypto/ssh`, `creack/pty`
* [x] server options

  * [x] host keys (ephemeral ed25519 for now)
  * [x] disable passwords/keyboard-interactive
  * [x] disable forwarding/subsystems
  * [x] idle timeout (e.g., 2h)
* [x] key acceptance

  * [x] allow: ed25519, ecdsa p256/p384, rsa-sha2-256/512, sk-ssh-ed25519
  * [x] reject: dss, rsa-sha1
* [x] compute fingerprint: `ssh.FingerprintSHA256(pubkey)`
* [x] session handler

  * [x] allocate pty
  * [x] `exec.Command(BBS_CLIENT_PATH)`; set env: `BBS_PUBKEY_SHA256`, `BBS_PUBKEY_TYPE`, `REMOTE_ADDR`, `DATABASE_URL`, `BBS_DEFAULT_ROOM`
  * [x] wire stdio ↔ pty
  * [x] exit on disconnect
* [x] containerize gateway with `BBS_CLIENT_PATH=/app/bbs-tui`

## m8 — docker-compose + cloudflare

* [ ] compose services: `postgres`, `ssh-gateway`, `cloudflared`
* [ ] ports: expose `2222` for local; cf tunnel runs with `TUNNEL_TOKEN`
* [ ] volumes: `pg` persistent
* [ ] env: inject `DATABASE_URL` into gateway
* [ ] sanity test locally: `ssh -p 2222 user@localhost` (any key)
* [ ] sanity test via cf: set up tunnel to gateway:2222

## m9 — retention job (app-level, phase 1)

* [ ] env `BBS_RETENTION_DAYS=30`
* [ ] background task every hour:

  * [ ] `delete from messages where created_at < now() - interval '$D days'`
* [ ] log purged row count
* [ ] ensure no blocking: run with small batch `limit 1000` loop

## m10 — logging + config

* [ ] structured logs

  * [ ] rust: `tracing` json formatter to stdout
  * [ ] go: log connect/disconnect, key type, fp short
* [ ] cli/env config

  * [ ] rust: read env with sane defaults, print on boot
  * [ ] `--history-load`, `--rate-per-min`, `--retention-days` override env (optional)

## m11 — security hardening (baseline)

* [ ] ssh:

  * [ ] passwords off, only pubkeys
  * [ ] restrict key algos to modern set
  * [ ] cgroups/ulimits in compose (cpu/mem)
* [ ] data:

  * [ ] store fingerprint only, not full pubkey blob
  * [ ] do not log message bodies
* [ ] input:

  * [ ] enforce ascii for nicks; nfkc + control-strip for message bodies
  * [ ] escape ansi sequences before render

## m12 — tests

* [ ] unit (rust)

  * [ ] command parsing `/nick|/join|/me`
  * [ ] validators (nick/room regex; length)
  * [ ] token bucket behavior
* [ ] integration (rust + pg)

  * [ ] migrations apply cleanly
  * [ ] user upsert by fingerprint
  * [ ] message insert with rate gate (allow→deny edge)
  * [ ] listen/notify rx
* [ ] e2e (scripted)

  * [ ] spawn 3 ssh sessions; send msgs; others receive within <200ms median
  * [ ] rename nick collision case
  * [ ] room create/join/delete path (creator vs non-creator)
* [ ] load probe (optional)

  * [ ] 100 concurrent sessions, 1 msg/5s, verify fanout and no drops

## m13 — ci + release

* [ ] github actions (or whatever)

  * [ ] rust: build + `sqlx prepare` (offline) + tests
  * [ ] go: build gateway
  * [ ] docker: build/push multi-arch images for `bbs-tui` and `bbs-ssh-gateway`
* [ ] semver tag + changelog
* [ ] sample `.env.example`

## m14 — acceptance criteria (tick before ship)

* [ ] login via ssh with ed25519/ecdsa/rsa-sha2 and get tui
* [ ] first-login assigns random ascii handle; `/nick` works; uniqueness enforced
* [ ] multi-room: `/join`, `/leave`, `/rooms`, `/who` all work
* [ ] messages persist; last `BBS_HISTORY_LOAD` load on join
* [ ] realtime delivery via listen/notify; fallback poll works if notify drops
* [ ] server-side rate limit 10/min enforced; client bucket mirrors
* [ ] retention deletes msgs older than 30 days
* [ ] room deletion allowed for creator; blocked for others
* [ ] compose up → system usable behind cf tunnel (raw tcp)

## m15 — nice-to-haves (post-mvp parking lot)

* [ ] moderation: roles, /mute, /ban, delete message
* [ ] per-room topics + pins
* [ ] emoji/reactions (width-aware rendering)
* [ ] metrics (prometheus), logs (loki), dashboards (grafana)
* [ ] pg cron for retention instead of app job
* [ ] unicode nicknames with nfkc + width accounting
* [ ] cf access/sso in front of ssh
