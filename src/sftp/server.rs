use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use russh::keys::ssh_key::{Algorithm, PrivateKey};
use russh::keys::PublicKey;
use russh::server::{Auth, Config, Handler, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::SftpConfig;
use crate::core::server::Server;
use crate::error::TentacleError;
use crate::sftp::handler::SftpSessionHandler;

#[derive(Clone)]
pub struct SftpServer {
    config: SftpConfig,
    servers: Arc<RwLock<HashMap<String, Arc<Server>>>>,
    panel_secret: String,
}

impl SftpServer {
    pub fn new(
        config: SftpConfig,
        servers: Arc<RwLock<HashMap<String, Arc<Server>>>>,
        panel_secret: String,
    ) -> Self {
        Self {
            config,
            servers,
            panel_secret,
        }
    }

    pub async fn run(self) -> Result<(), TentacleError> {
        let addr: SocketAddr = format!("{}:{}", self.config.listen_host, self.config.listen_port)
            .parse()
            .map_err(|e| TentacleError::Config(format!("Invalid SFTP socket address: {}", e)))?;

        let listener = TcpListener::bind(addr).await?;
        info!("SFTP subsystem listening on {}", addr);

        let private_key = self.load_or_generate_host_key()?;

        let mut russh_config = Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(600)),
            auth_rejection_time: std::time::Duration::from_millis(100),
            ..Default::default()
        };
        russh_config.keys.push(private_key);

        let russh_config = Arc::new(russh_config);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, remote_addr)) => {
                        info!("SFTP incoming connection from {}", remote_addr);
                        let config = russh_config.clone();
                        let handler = SftpClientHandler {
                            servers: self.servers.clone(),
                            panel_secret: self.panel_secret.clone(),
                            authenticated_server: None,
                            channel: None,
                        };

                        tokio::spawn(async move {
                            if let Err(e) = russh::server::run_stream(config, socket, handler).await {
                                warn!("SFTP session closed with error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("SFTP accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    fn load_or_generate_host_key(&self) -> Result<PrivateKey, TentacleError> {
        if self.config.host_key_path.exists() {
            PrivateKey::read_openssh_file(&self.config.host_key_path).map_err(|e| {
                TentacleError::Config(format!(
                    "Failed to read SFTP host key {}: {}",
                    self.config.host_key_path.display(),
                    e
                ))
            })
        } else {
            info!(
                "Generating new Ed25519 host key at {}",
                self.config.host_key_path.display()
            );
            if let Some(parent) = self.config.host_key_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let key = PrivateKey::random(&mut russh::keys::ssh_key::rand_core::OsRng, Algorithm::Ed25519).map_err(
                |e| TentacleError::Internal(format!("Failed to generate Ed25519 host key: {}", e)),
            )?;

            let _ = key.write_openssh_file(
                &self.config.host_key_path,
                russh::keys::ssh_key::LineEnding::LF,
            );
            Ok(key)
        }
    }
}

pub struct SftpClientHandler {
    servers: Arc<RwLock<HashMap<String, Arc<Server>>>>,
    panel_secret: String,
    authenticated_server: Option<Arc<Server>>,
    channel: Option<Channel<Msg>>,
}

#[allow(clippy::manual_async_fn)]
impl Handler for SftpClientHandler {
    type Error = russh::Error;

    fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> impl Future<Output = Result<Auth, Self::Error>> + Send {
        let servers = self.servers.clone();
        let expected_secret = self.panel_secret.clone();
        let user = user.to_string();
        let password = password.to_string();

        async move {
            let server_id = if let Some((_, sid)) = user.split_once('.') {
                sid
            } else {
                user.as_str()
            };

            let password_valid = password == expected_secret;

            if password_valid {
                let map = servers.read().await;
                if let Some(server) = map.get(server_id) {
                    self.authenticated_server = Some(server.clone());
                    info!("SFTP auth successful for server {}", server_id);
                    return Ok(Auth::Accept);
                }
            }

            warn!("SFTP authentication failed for user {}", user);
            Ok(Auth::Reject {
                proceed_with_methods: None,
            })
        }
    }

    fn auth_publickey(
        &mut self,
        _user: &str,
        _public_key: &PublicKey,
    ) -> impl Future<Output = Result<Auth, Self::Error>> + Send {
        async move {
            Ok(Auth::Reject {
                proceed_with_methods: None,
            })
        }
    }

    fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        async move {
            if self.authenticated_server.is_none() {
                return Ok(false);
            }
            self.channel = Some(channel);
            Ok(true)
        }
    }

    fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let is_sftp = name == "sftp" && self.channel.is_some() && self.authenticated_server.is_some();
        let maybe_channel = self.channel.take();
        let maybe_server = self.authenticated_server.clone();

        async move {
            if is_sftp {
                let _ = session.channel_success(channel);
                if let (Some(chan), Some(server)) = (maybe_channel, maybe_server) {
                    let stream = chan.into_stream();
                    let handler = SftpSessionHandler::new(server.fs.clone());
                    tokio::spawn(async move {
                        russh_sftp::server::run(stream, handler).await;
                    });
                }
            } else {
                let _ = session.channel_failure(channel);
            }
            Ok(())
        }
    }
}
