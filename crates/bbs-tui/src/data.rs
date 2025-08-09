use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub fingerprint_sha256: String,
    pub pubkey_type: String,
    pub handle: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Room {
    pub id: i64,
    pub name: String,
    pub created_by: i64,
    pub is_deleted: bool,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub room_id: i64,
    pub user_id: i64,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageView {
    pub id: i64,
    pub room_id: i64,
    pub user_id: i64,
    pub user_handle: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

pub async fn upsert_user_by_fp(pool: &PgPool, fp: &str, key_type: &str) -> Result<User> {
    // try select existing first
    if let Some(u) = sqlx::query_as::<_, User>(
        r#"select id, fingerprint_sha256, pubkey_type, handle, created_at, last_seen_at
           from users where fingerprint_sha256 = $1"#,
    )
    .bind(fp)
    .fetch_optional(pool)
    .await?
    {
        // touch last_seen_at
        sqlx::query("update users set last_seen_at = now() where id = $1")
            .bind(u.id)
            .execute(pool)
            .await?;
        return Ok(u);
    }

    // new user: generate handle and insert with collision retries
    let mut tries = 0;
    while tries < 10 {
        let handle = random_handle();
        let rec = sqlx::query_as::<_, User>(
            r#"insert into users(fingerprint_sha256, pubkey_type, handle)
               values($1,$2,$3)
               returning id, fingerprint_sha256, pubkey_type, handle, created_at, last_seen_at"#,
        )
        .bind(fp)
        .bind(key_type)
        .bind(&handle)
        .fetch_one(pool)
        .await;
        match rec {
            Ok(u) => return Ok(u),
            Err(e) => {
                // unique violation → retry with new handle
                let is_unique = e
                    .as_database_error()
                    .and_then(|d| d.code().map(|c| c == "23505"))
                    .unwrap_or(false);
                if is_unique {
                    tries += 1;
                    continue;
                }
                return Err(e.into());
            }
        }
    }
    Err(anyhow!("failed to create unique handle after retries"))
}

pub async fn ensure_room_exists(pool: &PgPool, name: &str, created_by: i64) -> Result<Room> {
    if let Some(r) = sqlx::query_as::<_, Room>(
        r#"select id, name, created_by, is_deleted, created_at, deleted_at
           from rooms where name = $1"#,
    )
    .bind(name)
    .fetch_optional(pool)
    .await?
    {
        return Ok(r);
    }

    let r = sqlx::query_as::<_, Room>(
        r#"insert into rooms(name, created_by) values($1,$2)
           returning id, name, created_by, is_deleted, created_at, deleted_at"#,
    )
    .bind(name)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(r)
}

pub async fn join_room(pool: &PgPool, room_id: i64, user_id: i64) -> Result<()> {
    sqlx::query(
        r#"insert into room_members(room_id, user_id)
           values($1,$2)
           on conflict(room_id, user_id)
           do update set last_joined_at = now()"#,
    )
    .bind(room_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn recent_messages_view(
    pool: &PgPool,
    room_id: i64,
    limit: i64,
) -> Result<Vec<MessageView>> {
    let rows = sqlx::query_as::<_, MessageView>(
        r#"select m.id, m.room_id, m.user_id, u.handle as user_handle, m.body, m.created_at
           from messages m
           join users u on u.id = m.user_id
           where m.room_id = $1 and m.deleted_at is null
           order by m.created_at desc
           limit $2"#,
    )
    .bind(room_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().rev().collect())
}

pub async fn insert_message(
    pool: &PgPool,
    room_id: i64,
    user_id: i64,
    body: &str,
) -> Result<Message> {
    let msg = sqlx::query_as::<_, Message>(
        r#"insert into messages(room_id, user_id, body)
           values($1,$2,$3)
           returning id, room_id, user_id, body, created_at, deleted_at"#,
    )
    .bind(room_id)
    .bind(user_id)
    .bind(body)
    .fetch_one(pool)
    .await?;
    Ok(msg)
}

pub async fn message_view_by_id(pool: &PgPool, id: i64) -> Result<Option<MessageView>> {
    let row = sqlx::query_as::<_, MessageView>(
        r#"select m.id, m.room_id, m.user_id, u.handle as user_handle, m.body, m.created_at
           from messages m
           join users u on u.id = m.user_id
           where m.id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

fn random_handle() -> String {
    // simple: usr-<8hex> from random u32
    let n: u32 = rand::thread_rng().gen();
    let hex = format!("{:08x}", n);
    let s = format!("usr-{}", hex);
    s.chars().take(16).collect()
}
