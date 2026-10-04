use std::path::PathBuf;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use tentacle::api::create_router;
use tentacle::config::Config;
use tentacle::core::MetricsPoller;
use tentacle::sftp::SftpServer;
use tentacle::state::AppState;

#[derive(Parser, Debug)]
#[command(name = "tentacle")]
#[command(author = "OctopusPanel Team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "High-performance node daemon for OctopusPanel", long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "FILE", help = "Path to config YAML file")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 1. Initialize configuration
    let config = Config::load_or_default(cli.config.as_deref())?;

    // 2. Initialize tracing
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.system.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    info!(
        "Starting Tentacle node daemon v{} [ID: {}]",
        env!("CARGO_PKG_VERSION"),
        config.node.id
    );

    // 3. Initialize Shared Application State
    let state = AppState::new(config.clone())?;

    // Restore any existing servers from persistent storage
    if let Err(e) = state.restore_servers().await {
        tracing::error!("Error restoring servers from persistent storage: {}", e);
    }

    // 4. Start Metrics Poller
    let poll_interval = config.resources.metrics_poll_interval_secs;
    MetricsPoller::start_polling(
        state.docker.client().clone(),
        state.servers.clone(),
        state.metrics_tx.clone(),
        poll_interval,
    );
    info!(
        "Background metrics poller active (interval: {}s)",
        poll_interval
    );

    // 5. Start SFTP Server (if enabled)
    if config.sftp.enabled {
        let sftp_server = SftpServer::new(
            config.sftp.clone(),
            state.servers.clone(),
            config.auth.panel_secret.clone(),
        );

        tokio::spawn(async move {
            if let Err(e) = sftp_server.run().await {
                error!("SFTP server encountered an error: {}", e);
            }
        });
    }

    // 6. Build API Router
    let app = create_router(state);

    // 7. Bind HTTP Listener
    let http_addr = format!("{}:{}", config.node.listen_host, config.node.listen_port);
    let listener = TcpListener::bind(&http_addr).await?;
    info!("Tentacle API gateway listening on http://{}", http_addr);

    // 8. Run Axum Server with Graceful Shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Tentacle daemon shutdown gracefully.");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
        },
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown...");
        },
    }
}
