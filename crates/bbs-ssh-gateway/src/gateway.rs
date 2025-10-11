use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, SocketAddrV6};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::future;
use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use russh::server::{self, Auth, Handle, Session};
use russh::{ChannelId, CryptoVec};
use ssh_key::{Algorithm, EcdsaCurve, HashAlg};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

const IO_BUFFER_SIZE: usize = 16 * 1024;

#[derive(Debug)]
pub struct AppConfig {
    pub client_path: PathBuf,
    pub default_room: String,
    pub database_url: Option<String>,
    pub listen_addr: SocketAddr,
}

pub struct GatewayServer {
    settings: Arc<AppConfig>,
}

impl GatewayServer {
    pub fn new(settings: Arc<AppConfig>) -> Self {
        Self { settings }
    }
}

impl server::Server for GatewayServer {
    type Handler = GatewaySession;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        GatewaySession::new(self.settings.clone(), peer_addr)
    }
}

pub struct GatewaySession {
    settings: Arc<AppConfig>,
    peer_addr: Option<SocketAddr>,
    env: HashMap<String, String>,
    pty: Option<PtyRequest>,
    channel: Option<ChannelId>,
    public_key: Option<PublicKeyInfo>,
    stdin_tx: Option<Sender<Vec<u8>>>,
    master: Option<Box<dyn MasterPty + Send>>,
    child_killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    read_task: Option<JoinHandle<()>>,
    write_task: Option<JoinHandle<()>>,
    exit_task: Option<JoinHandle<()>>,
    shell_started: bool,
}

struct PublicKeyInfo {
    fingerprint: String,
    mapped_kind: &'static str,
}

#[derive(Clone)]
struct PtyRequest {
    term: String,
    cols: u32,
    rows: u32,
    pixel_width: u32,
    pixel_height: u32,
}

type PtyIoHandles = (Sender<Vec<u8>>, JoinHandle<()>, JoinHandle<()>);

struct SpawnedSession {
    stdin_tx: Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    read_task: JoinHandle<()>,
    write_task: JoinHandle<()>,
    exit_task: JoinHandle<()>,
}

impl server::Handler for GatewaySession {
    type Error = anyhow::Error;

    fn auth_publickey(
        &mut self,
        _user: &str,
        public_key: &ssh_key::PublicKey,
    ) -> impl std::future::Future<Output = Result<Auth, Self::Error>> + Send {
        let result = self.process_auth_publickey(public_key);
        future::ready(result)
    }

    fn channel_open_session(
        &mut self,
        channel: russh::Channel<server::Msg>,
        session: &mut Session,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let result = self.process_channel_open_session(channel, session);
        future::ready(result)
    }

    fn env_request(
        &mut self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let result = self.process_env_request(channel, variable_name, variable_value, session);
        future::ready(result)
    }

    fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let request = PtyRequest {
            term: term.to_string(),
            cols: col_width,
            rows: row_height,
            pixel_width: pix_width,
            pixel_height: pix_height,
        };
        let result = self.process_pty_request(channel, request, session);
        future::ready(result)
    }

    fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let result = self.process_window_change(
            channel, col_width, row_height, pix_width, pix_height, session,
        );
        future::ready(result)
    }

    fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let result = self.process_shell_request(channel, session);
        future::ready(result)
    }

    fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let result = self.process_client_data(data);
        future::ready(result)
    }

    fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let result = self.process_channel_close(channel);
        future::ready(result)
    }

    fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        self.stdin_tx.take();
        future::ready(Ok(()))
    }
}

impl GatewaySession {
    fn new(settings: Arc<AppConfig>, peer_addr: Option<SocketAddr>) -> Self {
        Self {
            settings,
            peer_addr,
            env: HashMap::new(),
            pty: None,
            channel: None,
            public_key: None,
            stdin_tx: None,
            master: None,
            child_killer: None,
            read_task: None,
            write_task: None,
            exit_task: None,
            shell_started: false,
        }
    }

    fn process_auth_publickey(&mut self, public_key: &ssh_key::PublicKey) -> Result<Auth> {
        let algorithm = public_key.algorithm();
        let Some(mapped_kind) = map_algorithm(&algorithm) else {
            warn!(algo = ?algorithm, "rejecting unsupported public key algorithm");
            return Ok(Auth::reject());
        };

        let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();
        let short = short_fingerprint(&fingerprint);
        let remote = remote_addr_string(self.peer_addr);
        info!(
            remote = %remote,
            key_type = mapped_kind,
            fingerprint = %short,
            "client public key accepted"
        );

        self.public_key = Some(PublicKeyInfo {
            fingerprint,
            mapped_kind,
        });

        Ok(Auth::Accept)
    }

    fn process_channel_open_session(
        &mut self,
        channel: russh::Channel<server::Msg>,
        session: &mut Session,
    ) -> Result<bool> {
        if self.channel.is_some() {
            warn!("rejecting additional session channel");
            let _ = session.channel_open_failure(
                channel.id(),
                russh::ChannelOpenFailure::AdministrativelyProhibited,
                "only a single session channel is supported",
                "",
            );
            Ok(false)
        } else {
            self.channel = Some(channel.id());
            Ok(true)
        }
    }

    fn process_env_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        value: &str,
        session: &mut Session,
    ) -> Result<()> {
        if is_env_allowed(name) {
            self.env.insert(name.to_string(), value.to_string());
            let _ = session.channel_success(channel);
        } else {
            debug!(name, "blocking forwarded environment variable");
            let _ = session.channel_failure(channel);
        }
        Ok(())
    }

    fn process_pty_request(
        &mut self,
        channel: ChannelId,
        request: PtyRequest,
        session: &mut Session,
    ) -> Result<()> {
        self.pty = Some(request);
        let _ = session.channel_success(channel);
        Ok(())
    }

    fn process_window_change(
        &mut self,
        channel: ChannelId,
        cols: u32,
        rows: u32,
        pixel_width: u32,
        pixel_height: u32,
        session: &mut Session,
    ) -> Result<()> {
        if let Some(master) = self.master.as_ref() {
            if let Err(err) = master.resize(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: pixel_width as u16,
                pixel_height: pixel_height as u16,
            }) {
                warn!(error = %err, "failed to resize pty");
                let _ = session.channel_failure(channel);
                return Ok(());
            }
        }
        let _ = session.channel_success(channel);
        Ok(())
    }

    fn process_shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<()> {
        if self.shell_started {
            let _ = session.channel_failure(channel);
            return Ok(());
        }

        let Some(pty) = self.pty.clone() else {
            warn!("client attempted shell without requesting pty");
            let _ = session.channel_failure(channel);
            return Ok(());
        };

        let handle = session.handle();
        let remote = remote_addr_string(self.peer_addr);

        match self.spawn_gateway_process(&pty, channel, handle, remote.clone()) {
            Ok(spawned) => {
                let _ = session.channel_success(channel);
                info!(
                    remote = %remote,
                    cols = pty.cols,
                    rows = pty.rows,
                    "spawned TUI session"
                );
                let SpawnedSession {
                    stdin_tx,
                    master,
                    killer,
                    read_task,
                    write_task,
                    exit_task,
                } = spawned;
                self.stdin_tx = Some(stdin_tx);
                self.master = Some(master);
                self.child_killer = Some(killer);
                self.read_task = Some(read_task);
                self.write_task = Some(write_task);
                self.exit_task = Some(exit_task);
                self.shell_started = true;
                Ok(())
            }
            Err(error) => {
                let _ = session.channel_failure(channel);
                Err(error)
            }
        }
    }

    fn spawn_gateway_process(
        &self,
        request: &PtyRequest,
        channel: ChannelId,
        handle: Handle,
        remote: String,
    ) -> Result<SpawnedSession> {
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: request.rows as u16,
                cols: request.cols as u16,
                pixel_width: request.pixel_width as u16,
                pixel_height: request.pixel_height as u16,
            })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(self.settings.client_path.clone());
        for (key, value) in &self.env {
            cmd.env(key, value);
        }
        cmd.env("TERM", request.term.clone());
        if let Some(db) = &self.settings.database_url {
            cmd.env("DATABASE_URL", db);
        }
        cmd.env("BBS_DEFAULT_ROOM", self.settings.default_room.clone());
        cmd.env("REMOTE_ADDR", &remote);
        if let Some(info) = &self.public_key {
            cmd.env("BBS_PUBKEY_SHA256", info.fingerprint.clone());
            cmd.env("BBS_PUBKEY_TYPE", info.mapped_kind);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn TUI client")?;
        let killer = child.clone_killer();

        let master = pair.master;
        let (stdin_tx, read_task, write_task) =
            spawn_pty_io(&*master, channel, handle.clone(), remote.clone())?;
        let exit_task = spawn_child_waiter(child, handle, channel, remote);

        Ok(SpawnedSession {
            stdin_tx,
            master,
            killer,
            read_task,
            write_task,
            exit_task,
        })
    }

    fn process_client_data(&mut self, data: &[u8]) -> Result<()> {
        if let Some(tx) = &self.stdin_tx {
            if let Err(err) = tx.send(data.to_vec()) {
                error!(error = %err, "failed to forward client data to pty");
            }
        }
        Ok(())
    }

    fn process_channel_close(&mut self, _channel: ChannelId) -> Result<()> {
        self.stdin_tx.take();
        if let Some(mut killer) = self.child_killer.take() {
            if let Err(err) = killer.kill() {
                debug!(error = %err, "failed to kill child process");
            }
        }
        info!(remote = %remote_addr_string(self.peer_addr), "client disconnected");
        Ok(())
    }
}

fn spawn_child_waiter(
    mut child: Box<dyn Child + Send + Sync>,
    handle: Handle,
    channel: ChannelId,
    remote: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match tokio::task::spawn_blocking(move || child.wait()).await {
            Ok(Ok(status)) => {
                let code = status.exit_code();
                debug!(remote = %remote, exit_code = code, "tui process exited");
                if let Err(err) = handle.exit_status_request(channel, code).await {
                    debug!(error = ?err, "failed to send exit status");
                }
                let _ = handle.close(channel).await;
            }
            Ok(Err(err)) => {
                error!(remote = %remote, error = %err, "error waiting for tui process");
                let _ = handle.close(channel).await;
            }
            Err(join_err) => {
                error!(remote = %remote, error = %join_err, "failed to join tui wait task");
            }
        }
    })
}

fn spawn_pty_io(
    master: &dyn MasterPty,
    channel: ChannelId,
    handle: Handle,
    remote: String,
) -> Result<PtyIoHandles> {
    let mut reader = master
        .try_clone_reader()
        .context("failed to clone pty reader")?;
    let writer = master.take_writer().context("failed to take pty writer")?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let runtime = tokio::runtime::Handle::current();

    let reader_handle = tokio::task::spawn_blocking({
        let handle = handle.clone();
        let reader_remote = remote.clone();
        move || {
            let mut buf = vec![0u8; IO_BUFFER_SIZE];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = runtime.block_on(handle.eof(channel));
                        break;
                    }
                    Ok(n) => {
                        let mut data = CryptoVec::new();
                        data.extend(&buf[..n]);
                        if runtime.block_on(handle.data(channel, data)).is_err() {
                            break;
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(err) => {
                        debug!(remote = %reader_remote, error = %err, "error reading from pty");
                        break;
                    }
                }
            }
        }
    });

    let writer_handle = tokio::task::spawn_blocking(move || {
        let mut writer = writer;
        while let Ok(chunk) = rx.recv() {
            if chunk.is_empty() {
                continue;
            }
            if let Err(err) = writer.write_all(&chunk) {
                debug!(error = %err, "failed to write to pty");
                break;
            }
        }
    });

    Ok((tx, reader_handle, writer_handle))
}

fn map_algorithm(algo: &Algorithm) -> Option<&'static str> {
    match algo {
        Algorithm::Ed25519 => Some("ed25519"),
        Algorithm::Ecdsa { curve } => match curve {
            EcdsaCurve::NistP256 => Some("ecdsa256"),
            EcdsaCurve::NistP384 => Some("ecdsa384"),
            _ => None,
        },
        Algorithm::Rsa { hash } => match hash {
            Some(HashAlg::Sha256) => Some("rsa256"),
            Some(HashAlg::Sha512) => Some("rsa512"),
            _ => None,
        },
        Algorithm::SkEd25519 => Some("sk-ed25519"),
        _ => None,
    }
}

fn short_fingerprint(fingerprint: &str) -> &str {
    fingerprint
        .strip_prefix("SHA256:")
        .and_then(|rest| rest.get(..8))
        .unwrap_or(fingerprint)
}

fn is_env_allowed(name: &str) -> bool {
    matches!(
        name,
        "LANG"
            | "LC_ALL"
            | "LC_CTYPE"
            | "LC_MESSAGES"
            | "LC_TIME"
            | "LC_MONETARY"
            | "LC_NUMERIC"
            | "LC_COLLATE"
            | "COLORTERM"
            | "TERM"
    ) || name.starts_with("LC_")
}

fn remote_addr_string(addr: Option<SocketAddr>) -> String {
    match addr {
        Some(SocketAddr::V4(v4)) => v4.to_string(),
        Some(SocketAddr::V6(v6)) => format_ipv6(v6),
        None => String::from("unknown"),
    }
}

fn format_ipv6(addr: SocketAddrV6) -> String {
    format!("[{}]:{}", addr.ip(), addr.port())
}
