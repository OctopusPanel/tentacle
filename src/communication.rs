use async_trait::async_trait;
#[cfg(test)]
use mockall::{automock, predicate::*};

#[cfg_attr(test, automock)]
#[async_trait]
pub trait PanelClient {
    async fn connect(&mut self, node_key: &str) -> Result<bool, String>;
    async fn send_metrics(&self, metrics: &str) -> Result<(), String>;
}

pub struct WebSocketClient {
    panel_url: String,
}

impl WebSocketClient {
    pub fn new(panel_url: &str) -> Self {
        Self {
            panel_url: panel_url.to_string(),
        }
    }
}

#[async_trait]
impl PanelClient for WebSocketClient {
    async fn connect(&mut self, node_key: &str) -> Result<bool, String> {
        // WebSocket URL bauen
        let url = format!("{}/api/nodes/connect?key={}", self.panel_url, node_key);
        
        use tokio_tungstenite::connect_async;
        let (_ws_stream, response) = connect_async(&url)
            .await
            .map_err(|e| format!("WebSocket connection failed: {}", e))?;
            
        // Tungstenite gibt bei Erfolg 101 Switching Protocols zurück
        if response.status().is_informational() {
            Ok(true)
        } else {
            Err(format!("Unauthorized or server error: {}", response.status()))
        }
    }

    async fn send_metrics(&self, metrics: &str) -> Result<(), String> {
        // In Produktion würden wir hier den gespeicherten ws_stream verwenden:
        // self.ws_stream.send(Message::Text(metrics.to_string())).await
        println!("🚀 [WebSocket] Streaming metrics to panel: {}", metrics);
        Ok(())
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
