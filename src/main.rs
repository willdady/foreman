mod executors;
mod job;
mod network;
mod settings;
mod tracking;

use std::time::Duration;

use anyhow::{Ok, Result};

use axum::{
    body::Bytes, extract::Path, http::HeaderMap, response::IntoResponse, routing::put, Router,
};
use executors::{DockerExecutor, Executor};
use job::Job;
use log::{debug, info};
use reqwest::StatusCode;
use settings::SETTINGS;
use simplelog;
use tokio::{
    join,
    sync::{mpsc, oneshot},
};
use tracking::{JobStatus, JobTracker, JobTrackerCommand};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger.
    simplelog::SimpleLogger::init(simplelog::LevelFilter::Info, simplelog::Config::default())?;

    let settings = &*SETTINGS;
    println!("{:?}", settings);

    // Job executor channel
    let (job_executor_tx, mut job_executor_rx) = mpsc::channel::<Job>(32);

    // Job tracker channel
    let (job_tracker_tx, mut job_tracker_rx) = mpsc::channel::<JobTrackerCommand>(32);

    // Control server poller
    let user_agent = format!(
        "foreman/{} ({}, {})",
        VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let job_tracker_tx2 = job_tracker_tx.clone();
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
                anyhow::Result::Ok(job) => {
                    job_tracker_tx2
                        .send(JobTrackerCommand::Insert { job: job.clone() })
                        .await
                        .expect("Failed to send job to tracker channel");

                    job_executor_tx
                        .send(job)
                        .await
                        .expect("Failed to send job to executor channel");
                }
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

    // Manager task with exclusive access to Docker
    let job_manager = tokio::spawn(async move {
        let mut docker_executor = DockerExecutor::new().await.unwrap();

        while let Some(job) = job_executor_rx.recv().await {
            match docker_executor.execute(job).await {
                Err(e) => eprintln!("Error executing job: {}", e),
                _ => {}
            }
        }
    });

    let job_tracking = tokio::spawn(async move {
        let mut job_tracker = JobTracker::new();
        while let Some(command) = job_tracker_rx.recv().await {
            match command {
                JobTrackerCommand::Insert { job } => {
                    job_tracker.insert(job);
                    // resp.send(Ok(()))
                    //     .expect("Failed to send insert job response over channel");
                }
                JobTrackerCommand::HasJob { job_id, resp } => {
                    let result = job_tracker.has_job(&job_id);
                    resp.send(Ok(result))
                        .expect("Failed to send has job response over channel");
                }
                JobTrackerCommand::UpdateStatus {
                    job_id,
                    status,
                    resp,
                } => {
                    let result = job_tracker.update_status(&job_id, status);
                    resp.send(result)
                        .expect("Failed to send update status response over channel");
                }
            }
        }
    });

    let job_tracker_tx3 = job_tracker_tx.clone();
    let app = Router::new().route(
        "/job/:job_id",
        put(
            |Path(job_id): Path<String>, headers: HeaderMap, body: Bytes| async move {
                // TODO:
                // - Check if the job exists in memory/storage. If not, return an error.
                // - Update the job state in memory/storage.

                info!("Received PUT request for job ID: {}", job_id);
                debug!("Headers: {:?}", headers);

                let job_status: JobStatus = headers
                    .get("x-job-status")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .parse()
                    .unwrap();

                let job_progress: f64 = headers
                    .get("x-job-progress")
                    .and_then(|hv| hv.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                // TODO: Delete foreman specific headers before forwarding to callback url

                let (resp_tx, resp_rx) = oneshot::channel();
                job_tracker_tx3
                    .send(JobTrackerCommand::UpdateStatus {
                        job_id,
                        status: JobStatus::Completed, // FIXME: Parse from request header
                        resp: resp_tx,
                    })
                    .await
                    .unwrap();

                if let Err(e) = resp_rx.await {
                    eprintln!("Error updating job status: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to update job status",
                    )
                        .into_response();
                }

                // - Get the Job's callback URL.
                // - Forward the request payload to the Job's callback URL.
                let callback_url = "https://httpbin.org/put"; // FIXME: Temp!
                info!("Sending PUT request to callback URL {}", callback_url);
                let http_client = reqwest::Client::new();
                let resp = http_client
                    .put(callback_url)
                    .headers(headers)
                    .body(Into::<reqwest::Body>::into(body))
                    .send()
                    .await;

                return (StatusCode::OK, "OK").into_response();
            },
        ),
    );

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", settings.core.port))
        .await
        .unwrap(); // FIXME: Remove unwrap
    let server = axum::serve(listener, app);

    let _ = join!(control_server_poller, job_manager, job_tracking, server);

    Ok(())
}
