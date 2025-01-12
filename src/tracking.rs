use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{bail, Ok, Result};
use log::info;
use serde::Deserialize;
use tokio::sync::{mpsc::Sender, oneshot};

use crate::job::{DockerJob, Job};

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "UPPERCASE")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
}

impl FromStr for JobStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let s_upper = s.to_uppercase();
        let status = match s_upper.as_str() {
            "PENDING" => JobStatus::Pending,
            "RUNNING" => JobStatus::Running,
            "COMPLETED" => JobStatus::Completed,
            _ => bail!("Unknown job status"),
        };
        Ok(status)
    }
}

#[derive(Debug, Clone)]
pub struct TrackedJob {
    job: Job,
    status: JobStatus,
    progress: f64,
    start_time: Duration,
}

impl TrackedJob {
    pub fn inner(&self) -> &Job {
        &self.job
    }

    pub fn status(&self) -> &JobStatus {
        &self.status
    }
}

pub struct JobTracker {
    jobs: HashMap<String, Arc<Mutex<TrackedJob>>>,
}

impl JobTracker {
    pub fn new() -> Self {
        JobTracker {
            jobs: HashMap::new(),
        }
    }

    pub fn insert(&mut self, job: Job) {
        let Job::Docker(DockerJob { ref id, .. }) = job;
        let job_id = id.to_owned();
        let tracked_job = TrackedJob {
            job,
            status: JobStatus::Pending,
            progress: 0.0,
            start_time: Duration::from_secs(0),
        };
        self.jobs.insert(job_id, Arc::new(Mutex::new(tracked_job)));
    }

    pub fn get_job(&self, id: &str) -> Option<&Arc<Mutex<TrackedJob>>> {
        self.jobs.get(id)
    }

    pub fn update_status(&mut self, id: &str, status: JobStatus, progress: f64) -> Result<()> {
        if let Some(tracked_job) = self.jobs.get(id) {
            let mut tracked_job = tracked_job.lock().unwrap();
            info!(
                "Updating job {} with to status {:?} and progress {:.2}",
                id, status, progress
            );
            tracked_job.status = status;
            tracked_job.progress = progress;
            return Ok(());
        }
        bail!("Invalid job id");
    }
}

pub enum JobTrackerCommand {
    Insert {
        job: Job,
    },
    GetJob {
        job_id: String,
        resp: JobTrackerCommandResponder<Option<Arc<Mutex<TrackedJob>>>>,
    },
    UpdateStatus {
        job_id: String,
        status: JobStatus,
        progress: f64,
        resp: JobTrackerCommandResponder<()>,
    },
}

pub type JobTrackerCommandResponder<T> = oneshot::Sender<Result<T>>;

pub async fn get_job(
    job_id: &str,
    tx: &Sender<JobTrackerCommand>,
) -> Option<Arc<Mutex<TrackedJob>>> {
    let (resp_tx, resp_rx) = oneshot::channel();
    tx.send(JobTrackerCommand::GetJob {
        job_id: job_id.to_owned(),
        resp: resp_tx,
    })
    .await
    .expect("Failed sending GetJob command");

    resp_rx
        .await
        .expect("Failed to get job from channel")
        .ok()
        .flatten()
}

pub async fn update_job_status(
    job_id: &str,
    status: JobStatus,
    progress: f64,
    tx: &Sender<JobTrackerCommand>,
) -> Result<()> {
    let (resp_tx, resp_rx) = oneshot::channel();
    tx.send(JobTrackerCommand::UpdateStatus {
        job_id: job_id.to_owned(),
        status,
        progress,
        resp: resp_tx,
    })
    .await
    .expect("Failed sending UpdateStatus command");

    if let Err(e) = resp_rx.await {
        bail!("Error updating job status: {}", e);
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_job_status() {
        let j: JobStatus = "pending".parse().expect("Failed to parse job status");
        assert_eq!(j, JobStatus::Pending);

        let j: JobStatus = "running".parse().expect("Failed to parse job status");
        assert_eq!(j, JobStatus::Running);

        let j: JobStatus = "completed".parse().expect("Failed to parse job status");
        assert_eq!(j, JobStatus::Completed);
    }
}
