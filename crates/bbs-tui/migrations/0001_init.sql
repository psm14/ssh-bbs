-- Schema per TDD.md
create table if not exists users(
  id bigserial primary key,
  fingerprint_sha256 text not null unique,
  pubkey_type text not null,
  handle text not null unique
    check (handle ~ '^[a-z0-9_-]{2,16}$'),
  created_at timestamptz not null default now(),
  last_seen_at timestamptz not null default now()
);

create table if not exists rooms(
  id bigserial primary key,
  name text not null unique
    check (name ~ '^[a-z0-9_-]{1,24}$'),
  created_by bigint not null references users(id) on delete restrict,
  is_deleted boolean not null default false,
  created_at timestamptz not null default now(),
  deleted_at timestamptz
);

create table if not exists room_members(
  room_id bigint not null references rooms(id) on delete cascade,
  user_id bigint not null references users(id) on delete cascade,
  last_joined_at timestamptz not null default now(),
  primary key(room_id, user_id)
);

create table if not exists messages(
  id bigserial primary key,
  room_id bigint not null references rooms(id) on delete cascade,
  user_id bigint not null references users(id) on delete cascade,
  body text not null check (char_length(body) <= 1000),
  created_at timestamptz not null default now(),
  deleted_at timestamptz,
  constraint messages_body_nonempty check (length(btrim(body)) > 0)
);

create table if not exists bans(
  id bigserial primary key,
  user_id bigint not null references users(id) on delete cascade,
  reason text,
  created_at timestamptz not null default now(),
  expires_at timestamptz
);

create table if not exists name_changes(
  id bigserial primary key,
  user_id bigint not null references users(id) on delete cascade,
  old_handle text not null,
  new_handle text not null,
  changed_at timestamptz not null default now()
);

-- indexes
create index if not exists messages_room_created_idx on messages(room_id, created_at desc);
create index if not exists messages_user_created_idx on messages(user_id, created_at desc);
create index if not exists room_members_user_idx on room_members(user_id);
create index if not exists rooms_created_by_idx on rooms(created_by);

-- realtime trigger
create or replace function notify_new_message() returns trigger language plpgsql as $$
begin
  perform pg_notify('room_events', json_build_object(
    't','msg','room_id',new.room_id,'id',new.id
  )::text);
  return new;
end $$;

drop trigger if exists messages_notify on messages;
create trigger messages_notify
after insert on messages
for each row execute function notify_new_message();

