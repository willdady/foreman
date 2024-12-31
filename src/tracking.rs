use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{bail, Ok, Result};
use serde::Deserialize;
use tokio::sync::{mpsc::Sender, oneshot};

use crate::job::{DockerJob, Job};

#[derive(Debug, Deserialize, PartialEq)]
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

#[derive(Debug)]
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
        match job {
            Job::Docker(DockerJob { ref id, .. }) => {
                let job_id = id.to_owned();
                let tracked_job = TrackedJob {
                    job,
                    status: JobStatus::Pending,
                    progress: 0.0,
                    start_time: Duration::from_secs(0),
                };

                self.jobs.insert(job_id, Arc::new(Mutex::new(tracked_job)));
            }
            _ => panic!("Unsupported job type"),
        }
    }

    pub fn has_job(&self, id: &str) -> bool {
        self.jobs.contains_key(id)
    }

    pub fn get_job(&self, id: &str) -> Option<&Arc<Mutex<TrackedJob>>> {
        self.jobs.get(id)
    }

    pub fn update_status(&mut self, id: &str, status: JobStatus, progress: f64) -> Result<()> {
        if let Some(tracked_job) = self.jobs.get(id) {
            let mut tracked_job = tracked_job.lock().unwrap();
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
    tx: Sender<JobTrackerCommand>,
) -> Option<Arc<Mutex<TrackedJob>>> {
    let (resp_tx, resp_rx) = oneshot::channel();
    tx.send(JobTrackerCommand::GetJob {
        job_id: job_id.to_owned(),
        resp: resp_tx,
    })
    .await
    .expect("Failed sending GetJob command");

    let job_opt = resp_rx
        .await
        .expect("Failed to get job from channel")
        .ok()
        .flatten();
    job_opt
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
