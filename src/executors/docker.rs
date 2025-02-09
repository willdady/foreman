use std::collections::HashMap;

use crate::{
    env::EnvVars,
    job::{DockerJob, Job},
    network::PortManager,
    settings::SETTINGS,
};
use futures::{future, stream::StreamExt};
use log::info;

use super::JobExecutor;

use anyhow::{bail, Result};
use bollard::{
    container::{Config, CreateContainerOptions, StartContainerOptions, StopContainerOptions},
    image::{CreateImageOptions, ListImagesOptions},
    network::CreateNetworkOptions,
    secret::{ContainerCreateResponse, ContainerInspectResponse, PortBinding},
    Docker,
};

#[derive(Debug)]
pub struct DockerExecutor {
    docker: Docker,
    port_manager: PortManager,
}

impl DockerExecutor {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;

        let port_manager = PortManager::new(None, None)?;

        let _self = DockerExecutor {
            docker,
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

        let network_name = &SETTINGS.core.network_name;

        let network_exists = networks
            .iter()
            .any(|n| n.name == Some(network_name.to_string()));
        if !network_exists {
            let network_config = CreateNetworkOptions::<&str> {
                name: network_name,
                driver: "bridge",
                enable_ipv6: false,
                ..Default::default()
            };

            self.docker.create_network(network_config).await?;
            info!("Created network: {}", network_name);
        }
        Ok(())
    }

    async fn create_container(
        &self,
        id: &str,
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

        // Merge the default agent environment variables with the job's environment variables
        let mut resolved_env = env.unwrap_or_default();
        if let Some(default_env) = SETTINGS.core.env.as_ref() {
            resolved_env = resolved_env.merge_clone(default_env);
        }

        // Convert env from HashMap to Vec<&str>
        let mut env_strings: Vec<String> = resolved_env.into();
        env_strings.push(format!(
            "FOREMAN_GET_JOB_ENDPOINT=http://{}:{}/job/{}",
            SETTINGS.core.hostname, SETTINGS.core.port, id
        ));
        env_strings.push(format!(
            "FOREMAN_PUT_JOB_ENDPOINT=http://{}:{}/job/{}",
            SETTINGS.core.hostname, SETTINGS.core.port, id
        ));
        let env_strings: Vec<&str> = env_strings.iter().map(|s| s.as_str()).collect();

        // Container labels
        let mut labels = HashMap::new();
        labels.insert("managed-by", "foreman");

        // Extra hosts
        let extra_hosts = SETTINGS.core.extra_hosts.clone();

        let config = Config {
            image: Some(image),
            cmd,
            exposed_ports: Some(exposed_ports),
            host_config: Some(bollard::service::HostConfig {
                port_bindings: Some(port_bindings),
                network_mode: Some(SETTINGS.core.network_name.clone()),
                extra_hosts,
                ..Default::default()
            }),
            env: Some(env_strings),
            labels: Some(labels),
            ..Default::default()
        };

        info!("Created Docker container with name: {}", container_name);
        let container_create_response = self.docker.create_container(options, config).await?;
        Ok(container_create_response)
    }

    async fn stop_container(&self, container_name: &str) -> Result<()> {
        info!("Stopping container {}", container_name);
        self.docker
            .stop_container(container_name, Some(StopContainerOptions { t: 0 }))
            .await?;
        Ok(())
    }

    async fn remove_container(&self, container_name: &str) -> Result<()> {
        info!("Removing container {}", container_name);
        self.docker.remove_container(container_name, None).await?;
        Ok(())
    }

    async fn start_container(&self, container_name: &str) -> Result<()> {
        info!("Starting container: {}", container_name);
        self.docker
            .start_container(container_name, None::<StartContainerOptions<String>>)
            .await?;
        Ok(())
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

    async fn run(&mut self, docker_job: &DockerJob) -> Result<()> {
        let DockerJob {
            id,
            image,
            always_pull,
            port,
            env,
            command,
            ..
        } = docker_job;

        let container_name = format!("job-{}", id);
        // Pull image?
        if *always_pull {
            self.pull(image).await?;
        } else {
            let image_exists = self.image_exists(image).await?;
            if !image_exists {
                info!("Image {} does not exist, pulling...", image);
                self.pull(image).await?;
            } else {
                info!("Image {} exists, skipping pull...", image)
            }
        }
        // Create container
        let host_port = self.port_manager.reserve_port()?;
        self.create_container(
            id,
            &container_name,
            image,
            *port,
            host_port,
            command.as_ref(),
            env.clone(),
        )
        .await?;
        // Start container
        self.start_container(&container_name).await?;
        Ok(())
    }
}

impl JobExecutor for DockerExecutor {
    // Allowing irrefutable_let_patterns as currently there is only one Job variant.
    // Remove if/when other variants are added.
    #[allow(irrefutable_let_patterns)]
    async fn execute(&mut self, job: Job) -> Result<()> {
        if let Job::Docker(docker_job) = job {
            self.run(&docker_job).await?;
        } else {
            bail!("Expected docker job");
        }
        Ok(())
    }

    async fn stop(&mut self, job_id: &str) -> Result<()> {
        let container_name = format!("job-{}", job_id);
        self.stop_container(&container_name).await?;
        Ok(())
    }

    async fn remove(&mut self, job_id: &str) -> Result<()> {
        let container_name = format!("job-{}", job_id);
        self.remove_container(&container_name).await?;
        Ok(())
    }
}
