mod executors;
mod job;
mod network;
mod settings;

use anyhow::Result;

use executors::{DockerExecutor, Executor};
use job::{DockerJob, DockerJobHTTPMethod, Job};
use serde_json::json;
use settings::Settings;
use simplelog;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger.
    simplelog::SimpleLogger::init(simplelog::LevelFilter::Info, simplelog::Config::default())?;

    let settings = Settings::new()?;

    let mut docker_executor = DockerExecutor::new().await?;
    docker_executor
        .execute(Job::Docker(DockerJob {
            id: String::from("12345"),
            image: String::from("vs-test-image:latest"),
            port: 8080,
            callback_url: String::from("http://localhost:8888"),
            command: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo Yo && sleep 30 && echo Foo && False".to_string(),
            ]),
            body: json!({"foo": "bar"}),
            method: DockerJobHTTPMethod::POST,
            env: None,
            always_pull: false,
        }))
        .await?;

    Ok(())
}
