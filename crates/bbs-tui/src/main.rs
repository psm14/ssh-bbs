mod ui;
mod input;
mod data;
mod realtime;
mod rate;
mod rooms;
mod nick;
mod util;

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cfg = Config::from_env()?;
    info!(default_room = %cfg.default_room, "booting bbs-tui");

    // Connect DB and run migrations
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .context("connect postgres")?;
    sqlx::migrate!().run(&pool).await.context("run migrations")?;

    // TODO: upsert user by fingerprint and seed default room

    // TODO: start UI runtime
    ui::run_placeholder().await?;

    Ok(())
}

fn init_tracing() {
    let env = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("info".parse().unwrap_or_default());
    tracing_subscriber::fmt()
        .with_env_filter(env)
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .compact()
        .init();
}

struct Config {
    pub database_url: String,
    pub default_room: String,
    pub pubkey_sha256: Option<String>,
    pub pubkey_type: Option<String>,
    pub remote_addr: Option<String>,
    pub msg_max_len: usize,
    pub rate_per_min: u32,
    pub retention_days: u32,
    pub history_load: u32,
}

impl Config {
    fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL is required")?;
        let default_room = std::env::var("BBS_DEFAULT_ROOM").unwrap_or_else(|_| "lobby".to_string());
        let pubkey_sha256 = std::env::var("BBS_PUBKEY_SHA256").ok();
        let pubkey_type = std::env::var("BBS_PUBKEY_TYPE").ok();
        let remote_addr = std::env::var("REMOTE_ADDR").ok();
        let msg_max_len = std::env::var("BBS_MSG_MAX_LEN").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
        let rate_per_min = std::env::var("BBS_RATE_PER_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        let retention_days = std::env::var("BBS_RETENTION_DAYS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
        let history_load = std::env::var("BBS_HISTORY_LOAD").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
        Ok(Self { database_url, default_room, pubkey_sha256, pubkey_type, remote_addr, msg_max_len, rate_per_min, retention_days, history_load })
    }
}

