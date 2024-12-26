mod executors;
mod job;
mod network;
mod settings;

use std::time::Duration;

use anyhow::{Ok, Result};

use executors::{DockerExecutor, Executor};
use job::{
    //DockerJob,
    //DockerJobHTTPMethod,
    Job,
};
// use serde_json::json;
use settings::SETTINGS;
use simplelog;
use tokio::{join, sync::mpsc};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger.
    simplelog::SimpleLogger::init(simplelog::LevelFilter::Info, simplelog::Config::default())?;

    let settings = &*SETTINGS;
    println!("{:?}", settings);

    // Jobs channel
    let (tx, mut rx) = mpsc::channel(32);

    // Control server poller
    let user_agent = format!(
        "foreman/{} ({}, {})",
        VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let control_server_poller = tokio::spawn(async move {
        let http_client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_millis(settings.core.poll_timeout.into()))
            .user_agent(user_agent)
            .build()
            .unwrap();
        loop {
            let job_result: anyhow::Result<Job> = async {
                let job = http_client
                    .get(&settings.core.url)
                    .send()
                    .await?
                    .json::<Job>()
                    .await?;
                Ok(job)
            }
            .await;

            match job_result {
                anyhow::Result::Ok(job) => match tx.send(job).await {
                    Err(e) => eprintln!("Failed to send job to channel: {}", e),
                    _ => {}
                },
                anyhow::Result::Err(e) => {
                    eprintln!("Error fetching job from control server: {}", e)
                }
            };

            tokio::time::sleep(tokio::time::Duration::from_millis(
                settings.core.poll_frequency.into(),
            ))
            .await;
        }
    });

    // Spawn a manager task with exclusive access to Docker
    let job_manager = tokio::spawn(async move {
        let mut docker_executor = DockerExecutor::new().await.unwrap();

        while let Some(job) = rx.recv().await {
            match docker_executor.execute(job).await {
                Err(e) => eprintln!("Error executing job: {}", e),
                _ => {}
            }
        }
    });

    // let test_job = Job::Docker(DockerJob {
    //     id: String::from("12345"),
    //     image: String::from("vs-test-image:latest"),
    //     port: 8080,
    //     callback_url: String::from("http://localhost:8888"),
    //     command: Some(vec![
    //         "/bin/sh".to_string(),
    //         "-c".to_string(),
    //         "echo Yo && sleep 30 && echo Foo && False".to_string(),
    //     ]),
    //     body: json!({"foo": "bar"}),
    //     method: DockerJobHTTPMethod::POST,
    //     env: None,
    //     always_pull: false,
    // });

    // let test_job2 = Job::Docker(DockerJob {
    //     id: String::from("6789"),
    //     image: String::from("vs-test-image:latest"),
    //     port: 8080,
    //     callback_url: String::from("http://localhost:8888"),
    //     command: Some(vec![
    //         "/bin/sh".to_string(),
    //         "-c".to_string(),
    //         "echo Yo && sleep 180 && echo I am job 2 ".to_string(),
    //     ]),
    //     body: json!({"foo": "bar"}),
    //     method: DockerJobHTTPMethod::POST,
    //     env: None,
    //     always_pull: false,
    // });

    // println!("--- Sending job 12345 ---");
    // tx.send(test_job).await?;
    // println!("--- Sending job 6789 ---");
    // tx.send(test_job2).await?;

    let _ = join!(control_server_poller, job_manager);

    Ok(())
}
