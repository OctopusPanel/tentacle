mod config;
mod communication;
mod docker;
mod metrics;

use communication::{PanelClient, WebSocketClient};
use docker::{BollardContainerManager, ContainerManager};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    println!("🐙 Tentacle Daemon booting up...");

    let config = loop {
        match config::DaemonConfig::load() {
            Ok(cfg) if cfg.is_complete() => {
                println!("📋 Configuration loaded successfully.");
                break cfg;
            }
            Ok(_) => {
                eprintln!("⚠️ Configuration incomplete: PANEL_URL or NODE_KEY missing. Waiting for valid configuration in /etc/tentacle/config.json or ENV...");
            }
            Err(e) => {
                eprintln!("⚠️ Configuration load error: {}. Will retry in 10 seconds...", e);
            }
        }
        sleep(Duration::from_secs(10)).await;
    };

    let panel_url = config.panel_url.as_ref().unwrap();
    let node_key = config.node_key.as_ref().unwrap();
    let mut client = WebSocketClient::new(panel_url);

    println!("🔌 Attempting to connect to Octopus Panel at {}...", panel_url);

    loop {
        match client.connect(node_key).await {
            Ok(_) => {
                println!("✅ Connected and authenticated with Octopus Panel!");
                break;
            }
            Err(e) => {
                eprintln!("⚠️ Panel handshake failed: {}. Retrying in 5 seconds...", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }

    println!("🐳 Initializing Docker connection...");

    let container_manager = loop {
        match BollardContainerManager::new() {
            Ok(mgr) => {
                println!("✅ Docker engine connection established!");
                break mgr;
            }
            Err(e) => {
                eprintln!("⚠️ Docker connection failed: {}. Ensure docker is installed and running. Retrying in 10 seconds...", e);
                sleep(Duration::from_secs(10)).await;
            }
        }
    };

    println!("🚀 Daemon operational. Starting continuous runtime loop...");

    loop {
        match container_manager.list_containers().await {
            Ok(containers) => {
                let status_payload = serde_json::json!({
                    "status": "ONLINE",
                    "active_containers": containers.len(),
                    "timestamp": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                });

                if let Err(e) = client.send_metrics(&status_payload.to_string()).await {
                    eprintln!("⚠️ Error streaming metrics (connection might be dropped): {}. Reconnecting...", e);
                    if let Err(recon_err) = client.connect(node_key).await {
                        eprintln!("⚠️ Reconnection attempt failed: {}", recon_err);
                    } else {
                        println!("🔄 Successfully reconnected to Octopus Panel!");
                        let _ = client.send_metrics(&status_payload.to_string()).await;
                    }
                }
            }
            Err(e) => {
                eprintln!("⚠️ Failed to poll Docker containers: {}. Will retry next cycle...", e);
            }
        }
        sleep(Duration::from_secs(10)).await;
    }
}
