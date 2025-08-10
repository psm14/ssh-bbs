mod data;
mod input;
mod nick;
mod rate;
mod realtime;
mod rooms;
mod ui;
mod util;

use anyhow::{anyhow, Context, Result};
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
    sqlx::migrate!()
        .run(&pool)
        .await
        .context("run migrations")?;

    // Upsert user by fingerprint and seed default room
    let fp = cfg
        .pubkey_sha256
        .clone()
        .unwrap_or_else(|| "dev-local".into());
    let key_type = cfg.pubkey_type.clone().unwrap_or_else(|| "dev".into());
    let user = match data::upsert_user_by_fp(&pool, &fp, &key_type).await {
        Ok(u) => u,
        Err(e) => {
            error!(error=%e, "failed upsert user");
            return Err(e);
        }
    };
    let room = data::ensure_room_exists(&pool, &cfg.default_room, user.id).await?;
    data::join_room(&pool, room.id, user.id).await?;

    // start UI runtime (interactive)
    let fp_short = cfg
        .pubkey_sha256
        .as_deref()
        .map(crate::util::fp_short)
        .unwrap_or_else(|| "".into());
    let opts = ui::UiOpts {
        history_load: cfg.history_load,
        msg_max_len: cfg.msg_max_len,
        rate_per_min: cfg.rate_per_min,
        fp_short,
    };
    ui::run(pool.clone(), user, room, opts).await?;

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
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        let default_room =
            std::env::var("BBS_DEFAULT_ROOM").unwrap_or_else(|_| "lobby".to_string());
        let pubkey_sha256 = std::env::var("BBS_PUBKEY_SHA256").ok();
        let pubkey_type = std::env::var("BBS_PUBKEY_TYPE").ok();
        let remote_addr = std::env::var("REMOTE_ADDR").ok();
        let msg_max_len = std::env::var("BBS_MSG_MAX_LEN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);
        let rate_per_min = std::env::var("BBS_RATE_PER_MIN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let retention_days = std::env::var("BBS_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let history_load = std::env::var("BBS_HISTORY_LOAD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        Ok(Self {
            database_url,
            default_room,
            pubkey_sha256,
            pubkey_type,
            remote_addr,
            msg_max_len,
            rate_per_min,
            retention_days,
            history_load,
        })
    }
}
