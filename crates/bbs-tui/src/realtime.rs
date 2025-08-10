// LISTEN/NOTIFY loop (to be implemented)
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{postgres::PgListener, PgPool};
use tokio::{
    sync::mpsc,
    time::{sleep, Duration},
};

#[derive(Debug, Clone)]
pub enum Event {
    Message { id: i64, room_id: i64 },
}

#[derive(Debug, Deserialize)]
struct NotifyPayload {
    #[serde(rename = "t")]
    t: String,
    room_id: i64,
    id: i64,
}

pub async fn spawn_listener(pool: PgPool, tx: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let mut backoff_secs = 1u64;
        let mut last_seen: DateTime<Utc> = Utc::now();
        loop {
            match run_once(&pool, &tx).await {
                Ok(_) => {
                    backoff_secs = 1;
                }
                Err(_e) => {
                    // Fallback polling while we back off
                    let d = backoff_secs.min(30);
                    let steps = (d / 2).max(1);
                    for _ in 0..steps {
                        if let Err(_pe) = poll_once(&pool, &tx, &mut last_seen).await {
                            // ignore poll errors
                        }
                        sleep(Duration::from_secs(2)).await;
                    }
                    backoff_secs = (backoff_secs * 2).min(30);
                }
            }
        }
    });
}

async fn run_once(pool: &PgPool, tx: &mpsc::Sender<Event>) -> Result<()> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen("room_events").await?;
    loop {
        let n = listener.recv().await?;
        if let Ok(p) = serde_json::from_str::<NotifyPayload>(n.payload()) {
            if p.t == "msg" {
                let _ = tx
                    .send(Event::Message {
                        id: p.id,
                        room_id: p.room_id,
                    })
                    .await;
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct MinimalMsg {
    id: i64,
    room_id: i64,
    created_at: DateTime<Utc>,
}

async fn poll_once(
    pool: &PgPool,
    tx: &mpsc::Sender<Event>,
    last_seen: &mut DateTime<Utc>,
) -> Result<()> {
    // Fetch new messages since last_seen and emit as events
    let rows: Vec<MinimalMsg> = sqlx::query_as::<_, MinimalMsg>(
        r#"select id, room_id, created_at
           from messages
           where created_at > $1
           order by created_at asc
           limit 100"#,
    )
    .bind(*last_seen)
    .fetch_all(pool)
    .await?;

    for r in rows {
        let _ = tx
            .send(Event::Message {
                id: r.id,
                room_id: r.room_id,
            })
            .await;
        if r.created_at > *last_seen {
            *last_seen = r.created_at;
        }
    }
    Ok(())
}
