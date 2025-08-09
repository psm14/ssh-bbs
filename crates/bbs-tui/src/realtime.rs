// LISTEN/NOTIFY loop (to be implemented)
use anyhow::Result;
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

pub async fn spawn_listener(pool: PgPool, room_id: i64, tx: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let mut backoff_secs = 1u64;
        loop {
            match run_once(&pool, room_id, &tx).await {
                Ok(_) => {
                    backoff_secs = 1;
                }
                Err(_e) => {
                    let d = backoff_secs.min(30);
                    sleep(Duration::from_secs(d)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                }
            }
        }
    });
}

async fn run_once(pool: &PgPool, room: i64, tx: &mpsc::Sender<Event>) -> Result<()> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen("room_events").await?;
    loop {
        let n = listener.recv().await?;
        if let Ok(p) = serde_json::from_str::<NotifyPayload>(&n.payload()) {
            if p.t == "msg" && p.room_id == room {
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
