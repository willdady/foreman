mod executors;
mod job;
mod network;
mod settings;
mod tracking;

use std::{sync::LazyLock, time::Duration};

use anyhow::{Ok, Result};

use axum::{
    body::Bytes,
    extract::Path,
    http::{HeaderMap, HeaderValue},
    routing::{get, put},
    Json, Router,
};
use executors::{DockerExecutor, Executor};
use job::Job;
use log::{debug, error, info};
use reqwest::StatusCode;
use serde_json::json;
use settings::SETTINGS;
use simplelog;
use tokio::{
    join,
    sync::{
        mpsc::{self, Sender},
        oneshot,
    },
};
use tracking::{get_job, update_job_status, JobStatus, JobTracker, JobTrackerCommand};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");
static USER_AGENT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "foreman/{} ({}, {})",
        VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
});

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
    let job_tracker_tx2 = job_tracker_tx.clone();
    let control_server_poller = tokio::spawn(async move {
        let http_client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_millis(settings.core.poll_timeout.into()))
            .user_agent(&*USER_AGENT)
            .build()
            .unwrap();
        loop {
            let job_result: anyhow::Result<Job> = async {
                let job = http_client
                    .get(&settings.core.url)
                    .header("Authorization", format!("Bearer {}", settings.core.token))
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
                    error!("Error fetching job from control server: {}", e)
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
                Err(e) => error!("Error executing job: {}", e),
                _ => {}
            }
        }
    });

    // Job tracking task for managing job state
    let job_tracking = tokio::spawn(async move {
        let mut job_tracker = JobTracker::new();
        while let Some(command) = job_tracker_rx.recv().await {
            match command {
                JobTrackerCommand::Insert { job } => {
                    job_tracker.insert(job);
                }
                JobTrackerCommand::GetJob { job_id, resp } => {
                    let result = job_tracker.get_job(&job_id).and_then(|j| Some(j.clone()));
                    resp.send(Ok(result))
                        .expect("Failed to send has job response over channel");
                }
                JobTrackerCommand::UpdateStatus {
                    job_id,
                    status,
                    progress,
                    resp,
                } => {
                    let result = job_tracker.update_status(&job_id, status, progress);
                    resp.send(result)
                        .expect("Failed to send update status response over channel");
                }
            }
        }
    });

    let job_tracker_tx3 = job_tracker_tx.clone();
    let job_tracker_tx4 = job_tracker_tx.clone();
    let app = Router::new()
        .route(
            "/job/:job_id",
            get(|Path(job_id): Path<String>| async move {
                let job_opt = get_job(&job_id, &job_tracker_tx3).await;
                if let None = job_opt {
                    return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" })));
                }

                let tracked_job = {
                    let tracked_job = job_opt.unwrap();
                    let tracked_job = tracked_job.lock().unwrap();
                    tracked_job.clone() // I don't love the clone here :(
                };
                let Job::Docker(docker_job) = tracked_job.inner();

                match tracked_job.status() {
                    &JobStatus::Completed => {
                         return (
                            StatusCode::FORBIDDEN,
                            Json(json!({ "error": "refusing to return job as it's status is 'completed'" })),
                        );
                    },
                    &JobStatus::Pending => {
                        if let Err(e) = update_job_status(
                            &docker_job.id,
                            JobStatus::Running,
                            0.0,
                            &job_tracker_tx3,
                        )
                        .await {
                            error!("Failed to update job status: {}", e);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(json!({ "error": "failed to update job status" })),
                            );
                        };
                    }
                    _ => {}
                }

                (StatusCode::OK, Json(json!({ "id": docker_job.id, "body": docker_job.body })))
            }),
        )
        .route(
            "/job/:job_id",
            put(
                |Path(job_id): Path<String>, headers: HeaderMap, body: Bytes| async move {
                    info!("Received PUT request for job ID: {}", job_id);
                    debug!("Headers: {:?}", headers);
                    let status: JobStatus = match headers.get("x-foreman-job-status") {
                        Some(hv) => match hv.to_str() {
                            std::result::Result::Ok(s) => match s.parse() {
                                std::result::Result::Ok(js) => js,
                                Err(e) => {
                                    let error_msg =
                                        format!("Invalid header x-foreman-job-status: {}", e);
                                    error!("{}", error_msg);
                                    return (StatusCode::BAD_REQUEST, error_msg);
                                }
                            },
                            Err(e) => {
                                let error_msg =
                                    format!("Failed to parse x-foreman-job-status header: {}", e);
                                error!("{}", error_msg);
                                return (StatusCode::BAD_REQUEST, error_msg);
                            }
                        },
                        None => {
                            let error_msg = "Missing x-foreman-job-status header";
                            error!("{}", error_msg);
                            return (StatusCode::BAD_REQUEST, error_msg.to_string());
                        }
                    };

                    let progress: f64 = headers
                        .get("x-foreman-job-progress")
                        .and_then(|hv| hv.to_str().ok())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);

                    // Update the job status in the JobTracker.
                    if let Err(e) =
                        update_job_status(&job_id, status, progress, &job_tracker_tx4).await
                    {
                        error!("Error updating job status: {}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to update job status".to_string(),
                        );
                    };

                    // Get the job object from the JobTracker
                    let job_opt = get_job(&job_id, &job_tracker_tx4).await;
                    if let None = job_opt {
                        return (StatusCode::NOT_FOUND, "Job not found".to_string());
                    }
                    let callback_url = {
                        let tracked_job = job_opt.unwrap();
                        let tracked_job = tracked_job.lock().unwrap();
                        let Job::Docker(docker_job) = &tracked_job.inner();
                        docker_job.callback_url.clone()
                    };

                    // Send a PUT request to the callback URL
                    info!("Sending PUT request to callback URL {}", callback_url);
                    let http_client = reqwest::Client::new();
                    let mut headers = headers.clone();
                    headers.insert("user-agent", HeaderValue::from_str(&*USER_AGENT).unwrap());
                    let resp = http_client
                        .put(callback_url)
                        .headers(headers)
                        .body(Into::<reqwest::Body>::into(body))
                        .send()
                        .await;
                    if let std::result::Result::Ok(resp) = resp {
                        let status_code = resp.status();
                        info!("Status code {}", status_code);
                    } else {
                        let error_msg = format!("Failed to send PUT request: {:?}", resp);
                        error!("{}", error_msg);
                        return (StatusCode::BAD_REQUEST, error_msg);
                    }

                    return (StatusCode::OK, "OK".to_string());
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
