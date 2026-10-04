use std::collections::HashMap;
use std::path::Path;
use bollard::container::{
    Config as BollardContainerConfig, CreateContainerOptions, KillContainerOptions,
    ListContainersOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions, StopContainerOptions,
    WaitContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, PortBinding};
use bollard::Docker;
use futures_util::{StreamExt, TryStreamExt};
use tracing::{error, info, warn};

use crate::config::DockerConfig;
use crate::core::environment::EnvironmentInterpolator;
use crate::core::server::{InstallConfig, ServerConfig, ServerStatus};
use crate::error::TentacleError;

pub struct ContainerEngine {
    client: Docker,
    network_name: String,
}

impl ContainerEngine {
    pub fn new(config: &DockerConfig) -> Result<Self, TentacleError> {
        let client = Self::connect_docker(&config.socket_path, config.connection_timeout_secs)?;
        Ok(Self {
            client,
            network_name: config.network.clone(),
        })
    }

    pub fn client(&self) -> &Docker {
        &self.client
    }

    fn connect_docker(socket_path: &str, timeout_secs: u64) -> Result<Docker, TentacleError> {
        #[cfg(unix)]
        {
            Docker::connect_with_socket(socket_path, timeout_secs, bollard::API_DEFAULT_VERSION)
                .map_err(|e| TentacleError::Config(format!("Docker socket connection failed: {}", e)))
        }

        #[cfg(windows)]
        {
            if socket_path.starts_with("//./pipe/") {
                Docker::connect_with_named_pipe(
                    socket_path,
                    timeout_secs,
                    bollard::API_DEFAULT_VERSION,
                )
                .map_err(|e| {
                    TentacleError::Config(format!("Docker named pipe connection failed: {}", e))
                })
            } else {
                Docker::connect_with_socket(socket_path, timeout_secs, bollard::API_DEFAULT_VERSION)
                    .map_err(|e| {
                        TentacleError::Config(format!("Docker connection failed: {}", e))
                    })
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            Docker::connect_with_socket(socket_path, timeout_secs, bollard::API_DEFAULT_VERSION)
                .map_err(|e| TentacleError::Config(format!("Docker connection failed: {}", e)))
        }
    }

    pub async fn ensure_image(&self, image: &str) -> Result<(), TentacleError> {
        let image = image.trim();
        if image.is_empty() {
            return Err(TentacleError::Config("Docker image name cannot be empty".to_string()));
        }

        if self.client.inspect_image(image).await.is_ok() {
            return Ok(());
        }

        info!("Image '{}' not found locally. Pulling image via Docker API...", image);
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });

        let mut stream = self.client.create_image(options, None, None);
        while let Some(msg) = stream.try_next().await? {
            if let Some(status) = msg.status {
                tracing::debug!("[Docker Pull] {}", status);
            }
        }
        info!("Successfully pulled Docker image '{}'", image);
        Ok(())
    }

    pub async fn create_game_container(
        &self,
        config: &ServerConfig,
        volume_path: &Path,
    ) -> Result<String, TentacleError> {
        self.ensure_image(&config.docker_image).await?;

        let container_name = format!("octopus-{}", config.id);

        if let Ok(existing) = self.client.inspect_container(&container_name, None).await {
            if let Some(existing_id) = existing.id {
                info!(
                    "Found existing container '{}' ({}), removing stale instance before recreate",
                    container_name, existing_id
                );
                let _ = self
                    .client
                    .remove_container(
                        &existing_id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
            }
        }

        let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
        let mut exposed_ports = HashMap::new();

        for alloc in &config.allocations {
            let key = format!("{}/{}", alloc.container_port, alloc.protocol.to_lowercase());
            exposed_ports.insert(key.clone(), HashMap::new());

            let binding = PortBinding {
                host_ip: Some(alloc.host_ip.clone()),
                host_port: Some(alloc.host_port.to_string()),
            };

            port_bindings
                .entry(key)
                .or_insert_with(|| Some(Vec::new()))
                .as_mut()
                .unwrap()
                .push(binding);
        }

        let volume_bind = format!("{}:/home/container:rw", volume_path.to_string_lossy());
        let binds = vec![volume_bind];

        let host_config = HostConfig {
            binds: Some(binds),
            port_bindings: Some(port_bindings),
            memory: config.memory_limit_bytes,
            memory_swap: config.swap_limit_bytes,
            cpu_quota: config.cpu_quota,
            cpu_period: config.cpu_period,
            network_mode: Some(self.network_name.clone()),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            dns: Some(vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()]),
            ..Default::default()
        };

        let interpolated_cmd = EnvironmentInterpolator::interpolate(
            &config.startup_command,
            &config.environment,
        );

        let mut env = EnvironmentInterpolator::to_docker_env(&config.environment);
        if !env.iter().any(|e| e.starts_with("STARTUP=")) {
            env.push(format!("STARTUP={}", interpolated_cmd));
        }

        let mut labels = HashMap::new();
        labels.insert("octopus.server.id".to_string(), config.id.clone());
        labels.insert("octopus.managed".to_string(), "true".to_string());

        let container_config = BollardContainerConfig {
            image: Some(config.docker_image.clone()),
            cmd: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                interpolated_cmd,
            ]),
            working_dir: Some("/home/container".to_string()),
            env: Some(env),
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            labels: Some(labels),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(true),
            stdin_once: Some(false),
            tty: Some(true),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container_name.as_str(),
            platform: None,
        };

        let response = match self
            .client
            .create_container(Some(options.clone()), container_config.clone())
            .await
        {
            Ok(res) => res,
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 409, .. }) => {
                warn!(
                    "Container conflict for '{}', force removing stale container and retrying...",
                    container_name
                );
                let _ = self
                    .client
                    .remove_container(
                        &container_name,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                self.client
                    .create_container(Some(options), container_config)
                    .await?
            }
            Err(e) => return Err(e.into()),
        };

        Ok(response.id)
    }

    pub async fn start_container(&self, container_id: &str) -> Result<(), TentacleError> {
        self.client
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await?;
        Ok(())
    }

    pub async fn stop_container_graceful(
        &self,
        container_id: &str,
        timeout_secs: u64,
    ) -> Result<(), TentacleError> {
        let stop_options = StopContainerOptions {
            t: timeout_secs as i64,
        };

        match self.client.stop_container(container_id, Some(stop_options)).await {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!(
                    "Graceful stop for container {} timed out or failed: {}. Forcing SIGKILL.",
                    container_id, e
                );
                self.kill_container(container_id).await
            }
        }
    }

    pub async fn kill_container(&self, container_id: &str) -> Result<(), TentacleError> {
        let kill_options = KillContainerOptions {
            signal: "SIGKILL".to_string(),
        };
        self.client
            .kill_container(container_id, Some(kill_options))
            .await?;
        Ok(())
    }

    pub async fn restart_container(
        &self,
        container_id: &str,
        timeout_secs: u64,
    ) -> Result<(), TentacleError> {
        self.stop_container_graceful(container_id, timeout_secs).await?;
        self.start_container(container_id).await?;
        Ok(())
    }

    pub async fn remove_container(&self, container_id: &str) -> Result<(), TentacleError> {
        let options = RemoveContainerOptions {
            force: true,
            v: false,
            link: false,
        };
        self.client
            .remove_container(container_id, Some(options))
            .await?;
        Ok(())
    }

    pub async fn run_install_pipeline(
        &self,
        server_id: &str,
        install_config: &InstallConfig,
        volume_path: &Path,
        environment: &HashMap<String, String>,
    ) -> Result<(), TentacleError> {
        info!("Running installation pipeline for server {}", server_id);
        self.ensure_image(&install_config.image).await?;

        let install_name = format!("octopus-install-{}", server_id);

        // Remove any stale install container if one already exists
        if let Ok(existing) = self.client.inspect_container(&install_name, None).await {
            if let Some(existing_id) = existing.id {
                let _ = self
                    .client
                    .remove_container(
                        &existing_id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
            }
        }

        let volume_bind = format!("{}:/mnt/server:rw", volume_path.to_string_lossy());
        let host_config = HostConfig {
            binds: Some(vec![volume_bind]),
            network_mode: Some("bridge".to_string()),
            dns: Some(vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()]),
            ..Default::default()
        };

        let clean_script = install_config.script.replace("\r\n", "\n").replace('\r', "\n");
        let shell = install_config
            .entrypoint
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("/bin/sh")
            .to_string();

        let docker_env = EnvironmentInterpolator::to_docker_env(environment);

        let container_config = BollardContainerConfig {
            image: Some(install_config.image.clone()),
            entrypoint: Some(vec![shell]),
            cmd: Some(vec!["-c".to_string(), clean_script]),
            env: Some(docker_env),
            working_dir: Some("/mnt/server".to_string()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: install_name.as_str(),
            platform: None,
        };

        let response = self
            .client
            .create_container(Some(options), container_config)
            .await?;

        let install_id = response.id;

        self.client
            .start_container(&install_id, None::<StartContainerOptions<String>>)
            .await?;

        let mut wait_stream = self.client.wait_container(
            &install_id,
            Some(WaitContainerOptions {
                condition: "not-running".to_string(),
            }),
        );

        let mut exit_code = 1i64;
        while let Some(msg) = wait_stream.next().await {
            match msg {
                Ok(response) => {
                    exit_code = response.status_code;
                }
                Err(e) => {
                    error!("Error while awaiting install container: {}", e);
                }
            }
        }

        // Fetch logs and log them to tracing
        let mut log_stream = self.client.logs(
            &install_id,
            Some(LogsOptions::<String> {
                stdout: true,
                stderr: true,
                tail: "100".to_string(),
                ..Default::default()
            }),
        );

        while let Some(log_result) = log_stream.next().await {
            if let Ok(log_output) = log_result {
                let text = log_output.to_string();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    info!("[Installer {}] {}", server_id, trimmed);
                }
            }
        }

        // Cleanup install container
        let _ = self.remove_container(&install_id).await;

        if exit_code == 0 {
            info!("Installation pipeline completed successfully for {}", server_id);
            Ok(())
        } else {
            Err(TentacleError::Internal(format!(
                "Install container exited with non-zero code: {}",
                exit_code
            )))
        }
    }

    pub async fn reconcile_servers(
        &self,
        server_ids: &[String],
    ) -> Result<HashMap<String, (String, ServerStatus)>, TentacleError> {
        let mut filters = HashMap::new();
        filters.insert("label", vec!["octopus.managed=true"]);

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.client.list_containers(Some(options)).await?;
        let mut status_map = HashMap::new();

        for container in containers {
            if let Some(labels) = container.labels {
                if let Some(server_id) = labels.get("octopus.server.id") {
                    if server_ids.contains(server_id) {
                        let cid = container.id.unwrap_or_default();
                        let state = container.state.unwrap_or_default().to_lowercase();
                        let status = match state.as_str() {
                            "running" => ServerStatus::Running,
                            "restarting" => ServerStatus::Starting,
                            _ => ServerStatus::Offline,
                        };
                        status_map.insert(server_id.clone(), (cid, status));
                    }
                }
            }
        }

        Ok(status_map)
    }
}
