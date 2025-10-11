mod gateway;
mod hardening;
mod hostkey;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use gateway::{AppConfig, GatewayServer};
use russh::server::{self, Server as _};
use ssh_key::HashAlg;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    hardening::apply();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let listen_addr = std::env::var("BBS_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:2222".into());
    let listen_addr = listen_addr
        .parse()
        .with_context(|| format!("invalid BBS_LISTEN_ADDR value: {listen_addr}"))?;

    let client_path =
        PathBuf::from(std::env::var("BBS_CLIENT_PATH").unwrap_or_else(|_| "/app/bbs-tui".into()));
    let default_room = std::env::var("BBS_DEFAULT_ROOM").unwrap_or_else(|_| String::from("lobby"));
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|val| !val.is_empty());
    let host_key_path = PathBuf::from(
        std::env::var("BBS_HOSTKEY_PATH").unwrap_or_else(|_| "/app/host-keys/hostkey.pem".into()),
    );

    let host_key =
        hostkey::load_or_generate(&host_key_path).context("failed to load host key material")?;
    let host_fingerprint = host_key
        .public_key()
        .fingerprint(HashAlg::Sha256)
        .to_string();

    let mut ssh_config = server::Config {
        inactivity_timeout: Some(Duration::from_secs(2 * 60 * 60)),
        keepalive_interval: Some(Duration::from_secs(60)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    };
    ssh_config.keys.push(host_key);
    let ssh_config = Arc::new(ssh_config);

    let app_config = Arc::new(AppConfig {
        client_path,
        default_room,
        database_url,
        listen_addr,
    });

    info!(
        fingerprint = %host_fingerprint,
        "host key ready"
    );
    info!(
        addr = %app_config.listen_addr,
        client = %app_config.client_path.display(),
        room = %app_config.default_room,
        "ssh gateway listening"
    );

    let mut server_impl = GatewayServer::new(app_config.clone());
    server_impl
        .run_on_address(ssh_config, app_config.listen_addr)
        .await
        .context("ssh server failed")?;

    Ok(())
}
