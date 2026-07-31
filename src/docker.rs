use async_trait::async_trait;
use bollard::Docker;
use bollard::query_parameters::ListContainersOptions;

#[cfg(test)]
use mockall::{automock, predicate::*};

#[cfg_attr(test, automock)]
#[async_trait]
pub trait ContainerManager {
    async fn list_containers(&self) -> Result<Vec<String>, String>;
    async fn start_container(&self, image: &str, env_vars: Vec<String>) -> Result<String, String>;
    async fn stop_container(&self, id: &str) -> Result<bool, String>;
}

pub struct BollardContainerManager {
    docker: Docker,
}

impl BollardContainerManager {
    pub fn new() -> Result<Self, String> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| format!("Failed to connect to Docker: {}", e))?;
        Ok(Self { docker })
    }
}

#[async_trait]
impl ContainerManager for BollardContainerManager {
    async fn list_containers(&self) -> Result<Vec<String>, String> {
        let options = Some(ListContainersOptions {
            all: true,
            ..Default::default()
        });

        let containers = self.docker.list_containers(options).await
            .map_err(|e| format!("Failed to list containers: {}", e))?;

        let mut names = Vec::new();
        for container in containers {
            if let Some(mut container_names) = container.names {
                if let Some(name) = container_names.pop() {
                    names.push(name.trim_start_matches('/').to_string());
                }
            }
        }

        Ok(names)
    }

    async fn start_container(&self, image: &str, env_vars: Vec<String>) -> Result<String, String> {
        let config = bollard::models::ContainerCreateBody {
            image: Some(image.to_string()),
            env: Some(env_vars),
            ..Default::default()
        };

        let container = self.docker.create_container(None, config).await
            .map_err(|e| format!("Failed to create container: {}", e))?;

        self.docker.start_container(&container.id, None).await
            .map_err(|e| format!("Failed to start container: {}", e))?;

        Ok(container.id)
    }

    async fn stop_container(&self, id: &str) -> Result<bool, String> {
        self.docker.stop_container(id, None).await
            .map_err(|e| format!("Failed to stop container {}: {}", id, e))?;

        use bollard::query_parameters::RemoveContainerOptions;
        let remove_options = Some(RemoveContainerOptions {
            force: true,
            ..Default::default()
        });

        self.docker.remove_container(id, remove_options).await
            .map_err(|e| format!("Failed to remove container {}: {}", id, e))?;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_containers_mock() {
        let mut mock = MockContainerManager::new();
        
        mock.expect_list_containers()
            .times(1)
            .returning(|| Ok(vec!["container_1".to_string(), "container_2".to_string()]));
            
        let result = mock.list_containers().await;
        
        assert!(result.is_ok());
        let containers = result.unwrap();
        assert_eq!(containers.len(), 2);
        assert_eq!(containers[0], "container_1");
        assert_eq!(containers[1], "container_2");
    }

    #[tokio::test]
    async fn test_start_container_mock() {
        let mut mock = MockContainerManager::new();
        
        mock.expect_start_container()
            .with(eq("strixnodes/minecraft:latest"), eq(vec!["EULA=true".to_string()]))
            .times(1)
            .returning(|_, _| Ok("new_container_id_123".to_string()));
            
        let result = mock.start_container("strixnodes/minecraft:latest", vec!["EULA=true".to_string()]).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "new_container_id_123");
    }

    #[tokio::test]
    async fn test_stop_container_mock() {
        let mut mock = MockContainerManager::new();
        
        mock.expect_stop_container()
            .with(eq("container_id_123"))
            .times(1)
            .returning(|_| Ok(true));
            
        let result = mock.stop_container("container_id_123").await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }
}
