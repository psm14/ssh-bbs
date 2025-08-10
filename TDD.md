# bbs-over-ssh: technical design (phase 1)

## goals

* ssh-accessible “bbs” with a tui.
* accept key auth for everyone; pubkey fingerprint = identity.
* multi-room persistent chat w/ nicknames.
* postgres-only for storage + realtime.
* defaults: 10 msgs/min/user, 1000 chars/msg, retain 30 days (all configurable).
* utc everywhere.

## out of scope (phase 1)

* moderation tools (only schema hooks).
* backups, sso, cf access, horizontal scaling, metrics/dashboards.
* non-chat features.

## architecture

```
ssh client → cloudflare tunnel (raw tcp) → ssh gateway (pty) → spawn rust tui
                                               │ env exports: fp, keytype, ip
                                               └───────────→ postgres (sqlx + listen/notify)
```

* **ssh gateway (go/wish)**: thin auth+pty shim. computes pubkey sha256 fp; disables shell; launches the tui bound to session pty. supports: ed25519, ecdsa p256/p384, rsa-sha2-256/512, sk-ed25519; rejects dss + rsa-sha1.
* **tui client (rust)**: `ratatui` app. on start, upsert user by fingerprint, join a room (default `lobby`), show scrollback, subscribe to realtime, enforce client-side rate limit.
* **postgres**: source of truth + realtime via `LISTEN/NOTIFY`.

## identity & onboarding

* env from gateway:

  * `BBS_PUBKEY_SHA256` (openssh-style base64 sha256 of pubkey)
  * `BBS_PUBKEY_TYPE` (`ed25519|ecdsa256|ecdsa384|rsa256|rsa512|sk-ed25519`)
  * `REMOTE_ADDR` (ip\:port)
  * `DATABASE_URL`
  * `BBS_DEFAULT_ROOM` (default `lobby`)
* first run:

  * upsert user by fingerprint; if new, assign random ascii handle (adjective-noun-hex; truncated ≤16; retry on collision).
  * ensure default room exists; join it.
* subsequent runs: auto sign-in by fingerprint.

## ui/ux (tui)

* crates: `ratatui`, `crossterm`, `tokio`, `sqlx` (postgres), `serde`, `tracing`, `rand`, `unicode-segmentation`.
* layout:

  * main pane: current room messages (timestamp `[hh:mm:ss]` utc, nick, body).
  * right sidebar: rooms list + unread badges + online estimate.
  * bottom: input + slash hints.
  * statusline: current nick, room, rate bucket state, fp short (sha256:8).
* keybinds: `enter` send, `esc` focus input, `pgup/pgdn` scroll, `tab` switch rooms, `ctrl+c` quit.
* commands:

  * `/nick <name>` → change nickname (ascii only, `[a-z0-9_-]{2,16}`, unique).
  * `/join <room>` → create if missing; room name rules: `[a-z0-9_-]{1,24}`.
  * `/leave [room]` → drop membership (`delete from room_members where room_id=$rid and user_id=$me`) and unfocus if current.
  * `/rooms` → list rooms.
  * `/who [room]` → recent active users.
  * `/me <action>` → emote.
  * `/help`, `/quit`.

## rooms & ownership

* a room’s `created_by` is the user who first created it.
* delete rules (phase 1): only creator can delete; later mods can too.
* deletion is soft (to preserve refs); name becomes unavailable post-delete unless we fully purge (v2).

## rate limits, sizes, retention (defaults; env-configurable)

* per-user send: 10 msgs/min, burst 10.
* msg size: ≤1000 chars; body must be non-empty (trimmed).
* retention: 30 days. (phase 1: app-driven cleanup job; pg cron later.)

## realtime fanout

* single pg `NOTIFY 'room_events'` for all rooms; payload json:

  * `{"t":"msg","room_id":R,"id":M}`
  * (reserve: `{"t":"del","room_id":R,"id":M}`)
* client:

  * one listener task.
  * if joined to `room_id`, `select * from messages where id = $1`.
  * if listener drops, fall back to short polling by `created_at` > last seen.

## postgres schema

```sql
create table users(
  id bigserial primary key,
  fingerprint_sha256 text not null unique,
  pubkey_type text not null,
  handle text not null unique
    check (handle ~ '^[a-z0-9_-]{2,16}$'),
  created_at timestamptz not null default now(),
  last_seen_at timestamptz not null default now()
);

create table rooms(
  id bigserial primary key,
  name text not null unique
    check (name ~ '^[a-z0-9_-]{1,24}$'),
  created_by bigint not null references users(id) on delete restrict,
  is_deleted boolean not null default false,
  created_at timestamptz not null default now(),
  deleted_at timestamptz
);

create table room_members(
  room_id bigint not null references rooms(id) on delete cascade,
  user_id bigint not null references users(id) on delete cascade,
  last_joined_at timestamptz not null default now(),
  primary key(room_id, user_id)
);

create table messages(
  id bigserial primary key,
  room_id bigint not null references rooms(id) on delete cascade,
  user_id bigint not null references users(id) on delete cascade,
  body text not null check (char_length(body) <= 1000),
  created_at timestamptz not null default now(),
  deleted_at timestamptz,
  constraint messages_body_nonempty check (length(btrim(body)) > 0)
);

-- future moderation (v2)
create table bans(
  id bigserial primary key,
  user_id bigint not null references users(id) on delete cascade,
  reason text,
  created_at timestamptz not null default now(),
  expires_at timestamptz
);

create table name_changes(
  id bigserial primary key,
  user_id bigint not null references users(id) on delete cascade,
  old_handle text not null,
  new_handle text not null,
  changed_at timestamptz not null default now()
);

-- indexes
create index on messages(room_id, created_at desc);
create index on messages(user_id, created_at desc);
create index on room_members(user_id);
create index on rooms(created_by);
```

### triggers for realtime

```sql
create function notify_new_message() returns trigger language plpgsql as $$
begin
  perform pg_notify('room_events', json_build_object(
    't','msg','room_id',new.room_id,'id',new.id
  )::text);
  return new;
end $$;

create trigger messages_notify
after insert on messages
for each row execute function notify_new_message();
```

### room deletion (by creator)

```sql
-- soft delete; messages remain; future joins blocked
update rooms
set is_deleted = true, deleted_at = now()
where name = $1 and created_by = $creator_id and is_deleted = false;
```

* joining a deleted room should error in app; listing excludes `is_deleted`.

## server-enforced constraints (app-driven using pg)

* per-user/minute gate before insert:

```sql
with recent as (
  select count(*) c
  from messages
  where user_id=$1 and created_at > now() - interval '1 minute'
)
insert into messages(room_id, user_id, body)
select $room, $user, $body
where (select c from recent) < $limit
returning id;
```

* additionally, client maintains a token bucket to avoid spammy round trips.

## message flow

1. user types → client validates bucket + length.
2. client `insert ... returning id`.
3. `NOTIFY` fires via trigger; all clients get payload; if joined, select by id; render.

## config (env)

```
DATABASE_URL=postgres://bbs:bbs@postgres:5432/bbs
BBS_DEFAULT_ROOM=lobby
BBS_MSG_MAX_LEN=1000
BBS_RATE_PER_MIN=10
BBS_RETENTION_DAYS=30
BBS_HISTORY_LOAD=200
```

## docker-compose (minimal)

```yaml
version: "3.9"
services:
  postgres:
    image: postgres:16
    environment:
      POSTGRES_USER: bbs
      POSTGRES_PASSWORD: bbs
      POSTGRES_DB: bbs
    volumes: [ "pg:/var/lib/postgresql/data" ]
    healthcheck:
      test: ["CMD-SHELL","pg_isready -U bbs"]
      interval: 5s
      timeout: 3s
      retries: 20

  ssh-gateway:
    image: ghcr.io/you/bbs-ssh-gateway:latest
    environment:
      DATABASE_URL: postgres://bbs:bbs@postgres:5432/bbs
      BBS_CLIENT_PATH: /app/bbs-tui
      BBS_DEFAULT_ROOM: lobby
    depends_on: [ postgres ]
    ports: [ "0.0.0.0:2222:2222" ]  # local testing

  cloudflared:
    image: cloudflare/cloudflared:latest
    command: tunnel --no-autoupdate run
    environment:
      TUNNEL_TOKEN: ${TUNNEL_TOKEN}
    depends_on: [ ssh-gateway ]

volumes:
  pg: {}
```

## rust workspace layout

```
crates/
  bbs-tui/
    src/
      main.rs          # boot: env, db pool, ui runtime
      ui.rs            # ratatui widgets/layout
      input.rs         # command parsing + keybinds
      data.rs          # sqlx models + queries
      realtime.rs      # LISTEN/NOTIFY loop
      rate.rs          # token bucket (client-side)
      rooms.rs         # join/leave/history + ownership checks
      nick.rs          # rename validation + audit
      util.rs          # fp shortener, formatting
    migrations/
      0001_init.sql
```

## go ssh gateway sketch (wish)

* accept if key.type ∈ {ed25519, ecdsa256/384, rsa-sha2-256/512, sk-ed25519}; compute fp via `ssh.FingerprintSHA256(pubkey)`.
* disable: passwords, agent/port forwarding, subsystems.
* per-session:

  * allocate pty.
  * exec `BBS_CLIENT_PATH` with env (fp, type, remote addr, db url, default room).
  * wire stdio ⇄ pty; close on disconnect; idle timeout (e.g., 2h).

## security posture

* passwords off; only keys.
* support modern key types; reject legacy dss/rsa-sha1.
* store fingerprint only (no public key blob); do not log message bodies.
* ascii-only nicks to avoid width/render shenanigans; messages allow full unicode (nfkc normalize; strip control chars).
* sanitize rendering to avoid ansi injection (don’t trust message bodies; escape before render).

## failure modes

* db unavailable: tui prints transient error and retries with backoff; exit after \~30s with nonzero code.
* lost notify: client falls back to short polling (`select ... where created_at > last_seen` every 2s).
* name collision: db unique constraint → client shows error; user retries.

## ops notes (phase 1)

* retention job (best-effort, inside tui/background task):

```sql
delete from messages
where created_at < now() - ($RETENTION_DAYS || ' days')::interval;
```

* logs: structured json to stdout via `tracing`.

## testing

* unit: command parsing, nick/room validators, rate bucket.
* integration: migrations apply; upsert user; send/recv across two clients; listen/notify works.
* e2e: tmux script spawning n ssh sessions posting messages; ensure delivery median <200ms.

## roadmap (post-mvp)

* moderation: roles, /mute, /ban, delete message, room-level perms.
* per-room topics, pins, reactions.
* dm threads.
* cf access sso in front of ssh.
* pg cron for retention; metrics (prometheus), logs (loki).
* scaling: optional `bbs-core` service for fanout/rate limiting; or introduce redis pub/sub if rooms/users explode.
* optional unicode nicknames with nfkc + width-aware rendering.

## open choices (you can punt to later)

* room delete semantics beyond creator (mods): add `roles(user_id, room_id, role)`; gate deletes to `role in ('owner','mod')`.
* history load tuning: `BBS_HISTORY_LOAD` default 200; adjust per room or by last activity window.
