mod env;
mod executors;
mod job;
mod settings;
mod tracking;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock,
    },
    time::Duration,
};

use anyhow::{Ok, Result};

use axum::{
    body::Bytes,
    extract::Path,
    http::{HeaderMap, HeaderValue},
    routing::{get, put},
    Json, Router,
};
use executors::{DockerExecutor, JobExecutor, JobExecutorCommand};
use job::Job;
use log::{debug, error, info};
use reqwest::StatusCode;
use serde_json::json;
use settings::SETTINGS;
use tokio::{
    join,
    sync::mpsc::{self},
};
use tracking::{JobStatus, JobTracker, JobTrackerCommand};

const VERSION: &str = env!("CARGO_PKG_VERSION");
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

    // Load settings
    let settings = &*SETTINGS;

    // Thread-safe boolean which indicates whether we are running.
    // This changes to false when a termination signal is received.
    let running = Arc::new(AtomicBool::new(true));

    // Job executor channel
    let (job_executor_tx, mut job_executor_rx) = mpsc::channel::<JobExecutorCommand>(32);

    // Job tracker channel
    let (job_tracker_tx, mut job_tracker_rx) = mpsc::channel::<JobTrackerCommand>(32);

    // Control server poller
    let running2 = running.clone();
    let job_tracker_tx2 = job_tracker_tx.clone();
    let job_executor_tx2 = job_executor_tx.clone();
    let control_server_poller_task = tokio::spawn(async move {
        // Set default headers
        let mut default_headers = HeaderMap::new();
        if let Some(labels) = &settings.core.labels {
            let labels_string: String = labels.into();
            default_headers.insert(
                "x-foreman-labels",
                labels_string
                    .parse()
                    .expect("Failed to parse labels into header value"),
            );
        }
        // Configure the HTTP client
        let http_client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_millis(settings.core.poll_timeout.into()))
            .user_agent(&*USER_AGENT)
            .default_headers(default_headers)
            .build()
            .unwrap();
        loop {
            if !running2.load(Ordering::SeqCst) {
                info!("Stopping poller task");
                break;
            }

            // If we've reached our maximum concurrent jobs, sleep before polling again
            let running_jobs_count = tracking::count_running_jobs(&job_tracker_tx2)
                .await
                .unwrap_or_default();
            if running_jobs_count as u64 > settings.core.max_concurrent_jobs {
                info!(
                    "Reached maximum concurrent jobs ({}), waiting a bit before polling again",
                    settings.core.max_concurrent_jobs
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(
                    settings.core.poll_frequency.into(),
                ))
                .await;
                continue;
            }

            // Poll control server for jobs
            let jobs_result: anyhow::Result<Vec<Job>> = async {
                let jobs = http_client
                    .get(&settings.core.url)
                    .header("Authorization", format!("Bearer {}", settings.core.token))
                    .send()
                    .await?
                    .json::<Vec<Job>>()
                    .await?;
                Ok(jobs)
            }
            .await;

            match jobs_result {
                anyhow::Result::Ok(jobs) => {
                    for job in jobs {
                        info!("Got job: {:?}", job);
                        job_tracker_tx2
                            .send(JobTrackerCommand::Insert { job: job.clone() })
                            .await
                            .expect("Failed to send job to tracker channel");

                        job_executor_tx2
                            .send(JobExecutorCommand::Execute { job })
                            .await
                            .expect("Failed to send job to executor channel");
                    }
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
    let job_manager_task = tokio::spawn(async move {
        let mut executor = DockerExecutor::new()
            .await
            .expect("Failed to create Docker executor");

        while let Some(command) = job_executor_rx.recv().await {
            match command {
                JobExecutorCommand::Execute { job } => {
                    if let Err(e) = executor.execute(job).await {
                        error!("Error executing job: {}", e)
                    }
                }
                JobExecutorCommand::Stop { job_id } => {
                    if let Err(e) = executor.stop(&job_id).await {
                        error!("Error stopping job: {}", e)
                    }
                }
                JobExecutorCommand::Remove { job_id } => {
                    if let Err(e) = executor.remove(&job_id).await {
                        error!("Error removing job: {}", e)
                    }
                }
            }
        }
    });

    // Job tracking task for managing job state
    let job_tracking_task = tokio::spawn(async move {
        let mut job_tracker = JobTracker::new();
        loop {
            // Process commands received from the job tracker channel
            if let Some(command) = job_tracker_rx.recv().await {
                match command {
                    JobTrackerCommand::Insert { job } => {
                        job_tracker.insert(job);
                    }
                    JobTrackerCommand::GetJob { job_id, resp } => {
                        let result = job_tracker.get_job(&job_id).cloned();
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
                    JobTrackerCommand::GetRunningJobIds { resp } => {
                        let running_job_ids = job_tracker.get_running_job_ids();
                        resp.send(Ok(running_job_ids))
                            .expect("Failed to send running job ids response over channel");
                    }
                    JobTrackerCommand::GetStoppedJobIds { resp } => {
                        let stopped_job_ids = job_tracker.get_stopped_job_ids();
                        resp.send(Ok(stopped_job_ids))
                            .expect("Failed to send stopped job ids response over channel");
                    }
                    JobTrackerCommand::GetCompletedJobIds { resp } => {
                        let completed_job_ids = job_tracker.get_completed_job_ids();
                        resp.send(Ok(completed_job_ids))
                            .expect("Failed to send completed job ids response over channel");
                    }
                    JobTrackerCommand::GetTimedOutJobIds { resp } => {
                        let timed_out_job_ids = job_tracker.get_timed_out_job_ids();
                        resp.send(Ok(timed_out_job_ids))
                            .expect("Failed to send timed out job ids response over channel");
                    }
                    JobTrackerCommand::GetStoppedAndExpiredJobIds { resp } => {
                        let stopped_job_ids = job_tracker.get_stopped_and_expired_job_ids();
                        resp.send(Ok(stopped_job_ids))
                            .expect("Failed to send stopped job ids response over channel");
                    }
                    JobTrackerCommand::CountRunningJobs { resp } => {
                        let count = job_tracker.count_running_jobs();
                        resp.send(Ok(count))
                            .expect("Failed to send running job count response over channel");
                    }
                }
            }
        }
    });

    // Job lifecycle task coordinates between job tracker and job executor
    let running3 = running.clone();
    let job_tracker_tx3 = job_tracker_tx.clone();
    let job_executor_tx3 = job_executor_tx.clone();
    let job_lifecycle_task =
        tokio::spawn(async move {
            loop {
                // Send stop command to the job executor for any completed jobs
                let completed_job_ids = tracking::get_completed_job_ids(&job_tracker_tx3).await;
                if let Some(completed_job_ids) = completed_job_ids {
                    for job_id in completed_job_ids {
                        info!("Sending 'Stop' command for completed job: {}", job_id);
                        let command = JobExecutorCommand::Stop {
                            job_id: job_id.clone(),
                        };
                        job_executor_tx3.send(command).await.expect(
                            "Failed to send stop command to job executor for completed job ",
                        );
                        tracking::update_job_status(
                            &job_id,
                            JobStatus::Stopped,
                            None,
                            &job_tracker_tx3,
                        )
                        .await
                        .expect("Failed to update job status to 'stopped' for completed job");
                    }
                }
                // Send stop command to the job executor for any timed-out jobs
                let timed_out_job_ids = tracking::get_timed_out_job_ids(&job_tracker_tx3).await;
                if let Some(timed_out_job_ids) = timed_out_job_ids {
                    for job_id in timed_out_job_ids {
                        info!("Sending 'Stop' command for timed-out job: {}", job_id);
                        let command = JobExecutorCommand::Stop {
                            job_id: job_id.clone(),
                        };
                        job_executor_tx3.send(command).await.expect(
                            "Failed to send 'stop' command to job executor for timed-out job",
                        );
                        tracking::update_job_status(
                            &job_id,
                            JobStatus::Stopped,
                            None,
                            &job_tracker_tx3,
                        )
                        .await
                        .expect("Failed to update job status to 'stopped' for timed-out job");
                    }
                }
                // Send remove command to the job executor for any stopped and expired jobs
                let stopped_job_ids =
                    tracking::get_stopped_and_expired_job_ids(&job_tracker_tx3).await;
                if let Some(stopped_job_ids) = stopped_job_ids {
                    for job_id in stopped_job_ids {
                        info!("Sending 'remove' command for stopped job: {}", job_id);
                        let command = JobExecutorCommand::Remove {
                            job_id: job_id.clone(),
                        };
                        job_executor_tx3.send(command).await.expect(
                            "Failed to send 'remove' command to job executor for stopped job",
                        );
                        tracking::update_job_status(
                            &job_id,
                            JobStatus::Finished,
                            None,
                            &job_tracker_tx3,
                        )
                        .await
                        .expect("Failed to update job status to 'finished' for stopped job");
                    }
                }

                if !running3.load(Ordering::SeqCst) {
                    // Stop any running jobs
                    let running_job_ids = tracking::get_running_job_ids(&job_tracker_tx3)
                        .await
                        .unwrap_or_default();
                    let running_job_ids_length = running_job_ids.len();
                    for job_id in running_job_ids {
                        info!("Sending 'Stop' command for running job: {}", job_id);
                        let command = JobExecutorCommand::Stop {
                            job_id: job_id.clone(),
                        };
                        job_executor_tx3.send(command).await.expect(
                            "Failed to send 'stop' command to job executor for timed-out job",
                        );
                        tracking::update_job_status(
                            &job_id,
                            JobStatus::Stopped,
                            None,
                            &job_tracker_tx3,
                        )
                        .await
                        .expect("Failed to update job status to 'stopped' for running job");
                    }
                    // Remove any stopped jobs (if allowed by settings)
                    let mut stopped_job_ids_length: usize = 0;
                    if settings.core.remove_stopped_containers_on_terminate {
                        let stopped_job_ids = tracking::get_stopped_job_ids(&job_tracker_tx3)
                            .await
                            .unwrap_or_default();
                        stopped_job_ids_length = stopped_job_ids.len();
                        for job_id in stopped_job_ids {
                            info!("Sending 'remove' command for stopped job: {}", job_id);
                            let command = JobExecutorCommand::Remove {
                                job_id: job_id.clone(),
                            };
                            job_executor_tx3.send(command).await.expect(
                                "Failed to send 'remove' command to job executor for stopped job",
                            );
                            tracking::update_job_status(
                                &job_id,
                                JobStatus::Finished,
                                None,
                                &job_tracker_tx3,
                            )
                            .await
                            .expect("Failed to update job status to 'finished' for stopped job");
                        }
                    }

                    if running_job_ids_length == 0 && stopped_job_ids_length == 0 {
                        info!("Stopping lifecycle task");
                        break;
                    } else {
                        continue;
                    }
                }

                // Sleep for a while before checking again
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

    let job_tracker_tx4 = job_tracker_tx.clone();
    let job_tracker_tx5 = job_tracker_tx.clone();
    let app = Router::new()
        .route(
            "/job/:job_id",
            get(|Path(job_id): Path<String>| async move {
                let job_opt = tracking::get_job(&job_id, &job_tracker_tx4).await;
                if job_opt.is_none() {
                    return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" })));
                }

                let tracked_job = {
                    let tracked_job = job_opt.unwrap();
                    let tracked_job = tracked_job.lock().unwrap();
                    tracked_job.clone() // FIXME: I don't love the clone here :(
                };
                let Job::Docker(docker_job) = tracked_job.inner();

                match *tracked_job.status() {
                    JobStatus::Completed => {
                         return (
                            StatusCode::FORBIDDEN,
                            Json(json!({ "error": "refusing to return job as it's status is 'completed'" })),
                        );
                    },
                    JobStatus::Pending => {
                        if let Err(e) = tracking::update_job_status(
                            &docker_job.id,
                            JobStatus::Running,
                            Some(0.0),
                            &job_tracker_tx4,
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

                    // Get the job object from the JobTracker
                    let job_opt = tracking::get_job(&job_id, &job_tracker_tx5).await;
                    if job_opt.is_none() {
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
                    headers.insert("user-agent", HeaderValue::from_str(&USER_AGENT).unwrap());
                    let resp = http_client
                        .put(callback_url)
                        .headers(headers)
                        .body(Into::<reqwest::Body>::into(body))
                        .send()
                        .await;
                    if let std::result::Result::Ok(resp) = resp {
                        let status_code = resp.status();
                        info!("- Status code {}", status_code);
                    } else {
                        let error_msg = format!("Failed to send PUT request: {:?}", resp);
                        error!("{}", error_msg);
                        return (StatusCode::BAD_REQUEST, error_msg);
                    }

                    // Update the job status in the JobTracker.
                    if let Err(e) = tracking::update_job_status(&job_id, status, Some(progress), &job_tracker_tx5).await {
                        error!("Error updating job status: {}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to update job status".to_string(),
                        );
                    };

                    (StatusCode::OK, "OK".to_string())
                },
            ),
        );

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", settings.core.port)).await?;
    let server = axum::serve(listener, app);

    // Set up a Ctrl-C handler to gracefully shut down
    let running4 = running.clone();
    ctrlc::set_handler(move || {
        println!("Termination signal received, shutting down...");
        running4.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_secs(3));
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let _ = join!(
        control_server_poller_task,
        job_manager_task,
        job_tracking_task,
        job_lifecycle_task,
        server
    );

    Ok(())
}
