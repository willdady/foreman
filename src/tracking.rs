use std::{collections::HashMap, str::FromStr, time::Duration};

use anyhow::{bail, Ok, Result};
use serde::Deserialize;
use tokio::sync::oneshot;

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

struct TrackedJob {
    job: Job,
    status: JobStatus,
    start_time: Duration,
}

pub struct JobTracker {
    jobs: HashMap<String, TrackedJob>,
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
                    start_time: Duration::from_secs(0),
                };
                self.jobs.insert(job_id, tracked_job);
            }
            _ => panic!("Unsupported job type"),
        }
    }

    pub fn has_job(&self, id: &str) -> bool {
        self.jobs.contains_key(id)
    }

    pub fn update_status(&mut self, id: &str, status: JobStatus) -> Result<()> {
        if let Some(tracked_job) = self.jobs.get_mut(id) {
            tracked_job.status = status;
            return Ok(());
        }
        bail!("Invalid job id");
    }
}

pub enum JobTrackerCommand {
    Insert {
        job: Job,
        // resp: JobTrackerCommandResponder<()>,
    },
    HasJob {
        job_id: String,
        resp: JobTrackerCommandResponder<bool>,
    },
    UpdateStatus {
        job_id: String,
        status: JobStatus,
        resp: JobTrackerCommandResponder<()>,
    },
}

pub type JobTrackerCommandResponder<T> = oneshot::Sender<Result<T>>;

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