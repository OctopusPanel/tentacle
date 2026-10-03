use std::collections::VecDeque;
use std::sync::Arc;
use bollard::container::{AttachContainerOptions, AttachContainerResults};
use bollard::Docker;
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{error, info};

use crate::core::server::{Server, ServerStatus};
use crate::error::TentacleError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMessage {
    pub stream: String, // "stdout", "stderr", "stdin", "status"
    pub data: String,
    pub timestamp: u64,
}

pub struct CircularBuffer {
    capacity: usize,
    buffer: VecDeque<String>,
}

impl CircularBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push_line(&mut self, line: String) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(line);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.buffer.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

pub struct LogScanner {
    start_regex: Option<Regex>,
    crash_regex: Option<Regex>,
}

impl LogScanner {
    pub fn new(start_pattern: Option<&str>, crash_pattern: Option<&str>) -> Self {
        let start_regex = start_pattern.and_then(|p| Regex::new(p).ok());
        let crash_regex = crash_pattern.and_then(|p| Regex::new(p).ok());
        Self {
            start_regex,
            crash_regex,
        }
    }

    pub fn scan_line(&self, line: &str) -> Option<LogEvent> {
        if let Some(re) = &self.crash_regex {
            if re.is_match(line) {
                return Some(LogEvent::CrashDetected(line.to_string()));
            }
        }

        if let Some(re) = &self.start_regex {
            if re.is_match(line) {
                return Some(LogEvent::ServerStarted);
            }
        }

        None
    }
}

pub enum LogEvent {
    ServerStarted,
    CrashDetected(String),
}

pub struct StreamSession {
    pub buffer: Arc<RwLock<CircularBuffer>>,
    pub broadcast_tx: broadcast::Sender<StreamMessage>,
    pub stdin_tx: mpsc::Sender<String>,
}

impl StreamSession {
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<String>) {
        let buffer = Arc::new(RwLock::new(CircularBuffer::new(capacity)));
        let (broadcast_tx, _) = broadcast::channel(512);
        let (stdin_tx, stdin_rx) = mpsc::channel(128);

        (
            Self {
                buffer,
                broadcast_tx,
                stdin_tx,
            },
            stdin_rx,
        )
    }

    pub async fn attach_and_run(
        docker: &Docker,
        container_id: &str,
        server: Arc<Server>,
        session: Arc<StreamSession>,
        mut stdin_rx: mpsc::Receiver<String>,
    ) -> Result<(), TentacleError> {
        let options = AttachContainerOptions::<String> {
            stdin: Some(true),
            stdout: Some(true),
            stderr: Some(true),
            stream: Some(true),
            logs: Some(false),
            detach_keys: None,
        };

        let AttachContainerResults {
            mut output,
            mut input,
        } = docker.attach_container(container_id, Some(options)).await?;

        // Stdin forwarding task
        tokio::spawn(async move {
            while let Some(command) = stdin_rx.recv().await {
                let cmd_with_newline = if command.ends_with('\n') {
                    command
                } else {
                    format!("{}\n", command)
                };
                if let Err(e) = input.write_all(cmd_with_newline.as_bytes()).await {
                    error!("Failed to write to container stdin: {}", e);
                    break;
                }
                let _ = input.flush().await;
            }
        });

        // Stdout/Stderr receiving task
        let buffer = session.buffer.clone();
        let broadcast_tx = session.broadcast_tx.clone();

        let (start_pat, crash_pat) = {
            let cfg = server.config.read().await;
            (cfg.start_detection_regex.clone(), cfg.crash_detection_regex.clone())
        };

        let scanner = LogScanner::new(start_pat.as_deref(), crash_pat.as_deref());

        tokio::spawn(async move {
            while let Some(chunk_result) = output.next().await {
                match chunk_result {
                    Ok(log_output) => {
                        let text = log_output.to_string();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        for line in text.lines() {
                            let line_str = line.trim_end().to_string();

                            // 1. Buffer line
                            {
                                let mut buf = buffer.write().await;
                                buf.push_line(line_str.clone());
                            }

                            // 2. Scan line
                            if let Some(event) = scanner.scan_line(&line_str) {
                                match event {
                                    LogEvent::ServerStarted => {
                                        info!("Server started event recognized by log scanner");
                                        server.set_status(ServerStatus::Running).await;
                                    }
                                    LogEvent::CrashDetected(reason) => {
                                        error!("Server crash event detected: {}", reason);
                                        server.set_status(ServerStatus::Error).await;
                                    }
                                }
                            }

                            // 3. Broadcast to active clients
                            let msg = StreamMessage {
                                stream: "stdout".to_string(),
                                data: line_str,
                                timestamp: now,
                            };
                            let _ = broadcast_tx.send(msg);
                        }
                    }
                    Err(e) => {
                        error!("Docker output stream error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}
