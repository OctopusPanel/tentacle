use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonConfig {
    pub panel_url: Option<String>,
    pub node_key: Option<String>,
}

impl DaemonConfig {
    pub fn load() -> Result<Self, String> {
        let default_config_path = "/etc/tentacle/config.json";
        let config_path = env::var("TENTACLE_CONFIG").unwrap_or_else(|_| default_config_path.to_string());

        let mut config = if Path::new(&config_path).exists() {
            let content = fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse JSON config in {}: {}", config_path, e))?
        } else {
            DaemonConfig {
                panel_url: None,
                node_key: None,
            }
        };

        if let Ok(url) = env::var("PANEL_URL") {
            if !url.trim().is_empty() {
                config.panel_url = Some(url);
            }
        }
        if let Ok(key) = env::var("NODE_KEY") {
            if !key.trim().is_empty() {
                config.node_key = Some(key);
            }
        }

        let args: Vec<String> = env::args().collect();
        let mut i = 1;
        while i < args.len() {
            if (args[i] == "--url" || args[i] == "--panel-url") && i + 1 < args.len() {
                config.panel_url = Some(args[i + 1].clone());
                i += 2;
                continue;
            }
            if (args[i] == "--key" || args[i] == "--node-key") && i + 1 < args.len() {
                config.node_key = Some(args[i + 1].clone());
                i += 2;
                continue;
            }
            i += 1;
        }

        Ok(config)
    }

    pub fn is_complete(&self) -> bool {
        self.panel_url.as_ref().map_or(false, |s| !s.trim().is_empty()) &&
        self.node_key.as_ref().map_or(false, |s| !s.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config_is_incomplete() {
        let config = DaemonConfig {
            panel_url: None,
            node_key: None,
        };
        assert!(!config.is_complete());
    }

    #[test]
    fn test_valid_config() {
        let config = DaemonConfig {
            panel_url: Some("http://octo.panel".to_string()),
            node_key: Some("secret_key".to_string()),
        };
        assert!(config.is_complete());
    }
}
