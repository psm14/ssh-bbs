-- Invite codes for gatekeeping access
create table if not exists invites (
  code text primary key,
  created_by bigint references users(id) on delete set null,
  created_at timestamptz not null default now()
);

create index if not exists invites_created_at_idx on invites(created_at desc);

