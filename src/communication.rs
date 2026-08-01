use async_trait::async_trait;
use futures_util::SinkExt;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::protocol::Message;

#[cfg(test)]
use mockall::{automock, predicate::*};

#[cfg_attr(test, automock)]
#[async_trait]
pub trait PanelClient {
    async fn connect(&mut self, node_key: &str) -> Result<bool, String>;
    async fn send_metrics(&self, metrics: &str) -> Result<(), String>;
}

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct WebSocketClient {
    panel_url: String,
    stream: Arc<Mutex<Option<WsStream>>>,
}

impl WebSocketClient {
    pub fn new(panel_url: &str) -> Self {
        Self {
            panel_url: panel_url.to_string(),
            stream: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl PanelClient for WebSocketClient {
    async fn connect(&mut self, node_key: &str) -> Result<bool, String> {
        let ws_url = if self.panel_url.starts_with("http://") {
            self.panel_url.replace("http://", "ws://")
        } else if self.panel_url.starts_with("https://") {
            self.panel_url.replace("https://", "wss://")
        } else if !self.panel_url.starts_with("ws://") && !self.panel_url.starts_with("wss://") {
            format!("ws://{}", self.panel_url)
        } else {
            self.panel_url.clone()
        };

        let base_url = ws_url.trim_end_matches('/');
        let base_url = if base_url.ends_with("/api") {
            &base_url[..base_url.len() - 4]
        } else {
            base_url
        };
        let url = format!("{}/api/nodes/connect?key={}", base_url, node_key);

        let (ws_stream, response) = connect_async(&url)
            .await
            .map_err(|e| format!("WebSocket connection failed: {}", e))?;

        if response.status().is_informational() {
            let mut lock = self.stream.lock().await;
            *lock = Some(ws_stream);
            Ok(true)
        } else {
            Err(format!("Unauthorized or server error: {}", response.status()))
        }
    }

    async fn send_metrics(&self, metrics: &str) -> Result<(), String> {
        let mut lock = self.stream.lock().await;
        if let Some(stream) = lock.as_mut() {
            if let Err(e) = stream.send(Message::Text(metrics.to_string().into())).await {
                *lock = None;
                return Err(format!("Failed to transmit metrics over WebSocket stream: {}", e));
            }
            println!("🚀 [WebSocket] Successfully streamed metrics to panel: {}", metrics);
            Ok(())
        } else {
            Err("WebSocket is disconnected or not initialized.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_panel_handshake_mock() {
        let mut mock = MockPanelClient::new();
        
        // TDD: Der Mock simuliert eine erfolgreiche Authentifizierung via Node-Key
        mock.expect_connect()
            .with(eq("valid_node_key_123"))
            .times(1)
            .returning(|_| Ok(true));
            
        let result = mock.connect("valid_node_key_123").await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[tokio::test]
    async fn test_panel_handshake_invalid_key_mock() {
        let mut mock = MockPanelClient::new();
        
        mock.expect_connect()
            .with(eq("invalid_key"))
            .times(1)
            .returning(|_| Err("Unauthorized".to_string()));
            
        let result = mock.connect("invalid_key").await;
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Unauthorized");
    }
}
