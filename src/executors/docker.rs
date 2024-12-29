use std::{collections::HashMap, time::Duration};

use crate::{
    job::{DockerJob, EnvVars, Job},
    network::PortManager,
    settings::SETTINGS,
};
use futures::{future, stream::StreamExt};
use log::info;
use serde_json::Value;

use super::Executor;

use anyhow::{bail, Result};
use bollard::{
    container::{
        AttachContainerOptions, AttachContainerResults, Config, CreateContainerOptions,
        StartContainerOptions, StopContainerOptions, WaitContainerOptions,
    },
    image::{CreateImageOptions, ListImagesOptions},
    network::CreateNetworkOptions,
    secret::{ContainerCreateResponse, ContainerInspectResponse, HealthStatusEnum, PortBinding},
    Docker,
};

#[derive(Debug)]
pub struct DockerExecutor {
    docker: Docker,
    network_name: String,
    port_manager: PortManager,
}

impl DockerExecutor {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        // TODO: Ping docker

        let port_manager = PortManager::new(None, None)?;

        let network_name = "viewscreen";
        let _self = DockerExecutor {
            docker,
            network_name: network_name.to_string(),
            port_manager,
        };
        _self.create_network().await?;
        Ok(_self)
    }

    async fn pull(&self, image: &str) -> Result<()> {
        // println!("Pulling image {}", image);
        info!("Pulling image {}", image);

        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });
        self.docker
            .create_image(options, None, None)
            .for_each(|p| {
                if let Ok(info) = p {
                    println!("{:?}", info);
                }
                future::ready(())
            })
            .await;
        Ok(())
    }

    async fn create_network(&self) -> Result<()> {
        let networks = self.docker.list_networks::<String>(None).await?;
        let network_exists = networks
            .iter()
            .any(|n| n.name == Some(self.network_name.to_string()));
        if !network_exists {
            let network_config = CreateNetworkOptions::<&str> {
                name: &self.network_name,
                driver: "bridge",
                enable_ipv6: false,
                ..Default::default()
            };

            self.docker.create_network(network_config).await?;
            info!("Created network: {}", self.network_name);
        }
        Ok(())
    }

    async fn create_container(
        &self,
        container_name: &str,
        image: &str,
        port: u16,
        host_port: u16,
        command: Option<&Vec<String>>,
        env: Option<EnvVars>,
    ) -> Result<ContainerCreateResponse> {
        let cmd = command.map(|vec| vec.iter().map(|s| s.as_str()).collect());

        let options = Some(CreateContainerOptions {
            name: container_name,
            platform: None,
        });

        // FIXME: Port bindings should be configurable i.e. not needed if this program is also running inside Docker
        // Create port bindings
        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            format!("{}/tcp", port),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(host_port.to_string()),
            }]),
        );

        // Create exposed ports
        let _port = format!("{}/tcp", port);
        let empty_object: HashMap<(), ()> = HashMap::new();
        let mut exposed_ports = HashMap::new();
        exposed_ports.insert(_port.as_str(), empty_object);

        // Convert env from HashMap to Vec<&str>
        let env_strings: Option<Vec<String>> = env.map(|_env| _env.into());
        let env_vec: Option<Vec<&str>> = env_strings
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());

        // Container labels
        let mut labels = HashMap::new();
        labels.insert("managed-by", "foreman");

        let config = Config {
            image: Some(image),
            cmd,
            exposed_ports: Some(exposed_ports),
            host_config: Some(bollard::service::HostConfig {
                port_bindings: Some(port_bindings),
                network_mode: Some(self.network_name.clone()),
                ..Default::default()
            }),
            env: env_vec,
            labels: Some(labels),
            ..Default::default()
        };

        info!("Created Docker container with name: {}", container_name);
        let container_create_response = self.docker.create_container(options, config).await?;
        Ok(container_create_response)
    }

    async fn stop_container(&self, container_name: &str) -> Result<()> {
        info!("Stopping container {}", container_name);
        let stop_container_response = self
            .docker
            .stop_container(container_name, Some(StopContainerOptions { t: 0 }))
            .await?;
        Ok(stop_container_response)
    }

    async fn remove_container(&self, container_name: &str) -> Result<()> {
        info!("Removing container {}", container_name);
        let remove_container_response = self.docker.remove_container(container_name, None).await?;
        Ok(remove_container_response)
    }

    async fn stop_and_remove_container(&self, container_name: &str) -> Result<()> {
        self.stop_container(container_name).await?;
        self.remove_container(container_name).await?;
        Ok(())
    }

    async fn attach_container(&self, container_name: &str) -> Result<AttachContainerResults> {
        let options = AttachContainerOptions::<String> {
            stdout: Some(true),
            stderr: Some(true),
            stream: Some(true),
            ..Default::default()
        };

        let attach_container_results = self
            .docker
            .attach_container(container_name, Some(options))
            .await?;
        Ok(attach_container_results)
    }

    async fn start_container(&self, container_name: &str) -> Result<()> {
        info!("Starting container: {}", container_name);
        let start_container_result = self
            .docker
            .start_container(container_name, None::<StartContainerOptions<String>>)
            .await?;
        Ok(start_container_result)
    }

    async fn wait_container(&self, container_name: &str) -> Result<i64> {
        let wait_container_response = self
            .docker
            .wait_container(container_name, None::<WaitContainerOptions<String>>)
            .collect::<Vec<_>>()
            .await;
        if wait_container_response.len() != 1 {
            bail!("Unexpected wait response length");
        }

        match &wait_container_response[0] {
            Ok(x) => Ok(x.status_code),
            Err(e) => match e {
                bollard::errors::Error::DockerContainerWaitError { code, .. } => Ok(*code),
                _ => bail!("{}", e),
            },
        }
    }

    async fn inspect_container(&self, container_name: &str) -> Result<ContainerInspectResponse> {
        let inspect_container_response =
            self.docker.inspect_container(container_name, None).await?;
        Ok(inspect_container_response)
    }

    async fn image_exists(&self, image: &str) -> Result<bool> {
        let options = ListImagesOptions::<String> {
            all: true,
            ..Default::default()
        };
        let image_list = self.docker.list_images(Some(options)).await?;
        let exists = image_list
            .iter()
            .any(|image_summary| image_summary.repo_tags.contains(&image.to_string()));
        Ok(exists)
    }

    async fn run(
        &mut self,
        id: String,
        image: String,
        port: u16,
        command: Option<&Vec<String>>,
        body: Value,
        env: Option<EnvVars>,
        callback_url: String,
        always_pull: bool,
    ) -> Result<()> {
        let container_name = format!("job-{}", id);
        // Pull image?
        if always_pull {
            self.pull(&image).await?;
        } else {
            let image_exists = self.image_exists(&image).await?;
            if !image_exists {
                info!("Image {} does not exist, pulling...", image);
                self.pull(&image).await?;
            } else {
                info!("Image {} exists, skipping pull...", image)
            }
        }
        // Create container
        let host_port = self.port_manager.reserve_port()?;
        self.create_container(&container_name, &image, port, host_port, command, env)
            .await?;
        // Start container
        self.start_container(&container_name).await?;
        // Wait for container to become healthy
        let container_timeout = (&*SETTINGS).docker.container_timeout;
        let mut ms_ellapsed = 0;
        let health_status: HealthStatusEnum = loop {
            let container_inspect_response = self.inspect_container(&container_name).await?;
            let health_status = container_inspect_response
                .state
                .and_then(|state| state.health)
                .and_then(|health| health.status);
            if health_status.is_none() {
                break HealthStatusEnum::NONE;
            }

            let health_status = health_status.unwrap();
            if health_status == HealthStatusEnum::STARTING {
                info!("Waiting for container {} to start...", container_name);
                tokio::time::sleep(Duration::from_millis(500)).await;
                ms_ellapsed += 500;
                if ms_ellapsed >= container_timeout {
                    break health_status;
                }
            } else {
                break health_status;
            }
        };
        // Conditionally proceed based on health status
        match health_status {
            HealthStatusEnum::HEALTHY => {
                info!("Container {} is healthy!", container_name);
            }
            HealthStatusEnum::STARTING => {
                self.stop_and_remove_container(&container_name).await?;
                self.port_manager.release_port(host_port)?;
                bail!(
                    "Timeout waiting for container {} to pass health check",
                    container_name
                );
            }
            HealthStatusEnum::UNHEALTHY => {
                self.stop_and_remove_container(&container_name).await?;
                self.port_manager.release_port(host_port)?;
                bail!("Container {} is unhealthy", container_name);
            }
            HealthStatusEnum::NONE | HealthStatusEnum::EMPTY => {
                self.stop_and_remove_container(&container_name).await?;
                self.port_manager.release_port(host_port)?;
                bail!("Container {} does not have a health status", container_name);
            }
        }
        // Remove container
        self.stop_and_remove_container(&container_name).await?;
        self.port_manager.release_port(host_port)?;
        Ok(())
    }
}

impl Executor for DockerExecutor {
    // Allowing irrefutable_let_patterns as currently there is only one Job variant.
    // Remove if/when other variants are added.
    #[allow(irrefutable_let_patterns)]
    async fn execute(&mut self, job: Job) -> Result<()> {
        if let Job::Docker(DockerJob {
            id,
            image,
            port,
            command,
            body,
            env,
            callback_url,
            always_pull,
            ..
        }) = job
        {
            self.run(
                id,
                image,
                port,
                command.as_ref(),
                body,
                env,
                callback_url,
                always_pull,
            )
            .await?;
        } else {
            bail!("Expected docker job");
        }
        Ok(())
    }
}
