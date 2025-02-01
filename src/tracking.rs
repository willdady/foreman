use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use anyhow::{bail, Ok, Result};
use serde::Deserialize;
use tokio::sync::{mpsc::Sender, oneshot};

use crate::{
    job::{DockerJob, Job},
    settings::SETTINGS,
};

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "UPPERCASE")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Stopped,
    Finished,
}

impl FromStr for JobStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let s_upper = s.to_uppercase();
        let status = match s_upper.as_str() {
            "PENDING" => JobStatus::Pending,
            "RUNNING" => JobStatus::Running,
            "COMPLETED" => JobStatus::Completed,
            "STOPPED" => JobStatus::Stopped,
            "FINISHED" => JobStatus::Finished,
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
    start_time: SystemTime,
    completed_time: Option<SystemTime>,
    stopped_time: Option<SystemTime>,
    finished_time: Option<SystemTime>,
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
            start_time: SystemTime::now(),
            completed_time: None,
            stopped_time: None,
            finished_time: None,
        };
        self.jobs.insert(job_id, Arc::new(Mutex::new(tracked_job)));
    }

    pub fn get_job(&self, id: &str) -> Option<&Arc<Mutex<TrackedJob>>> {
        self.jobs.get(id)
    }

    pub fn update_status(
        &mut self,
        id: &str,
        status: JobStatus,
        progress: Option<f64>,
    ) -> Result<()> {
        // TODO: Prevent transition between certain states e.g., from Completed to Running is invalid
        if let Some(tracked_job) = self.jobs.get(id) {
            let mut tracked_job = tracked_job.lock().unwrap();
            match status {
                JobStatus::Completed => {
                    tracked_job.completed_time = Some(SystemTime::now());
                }
                JobStatus::Stopped => {
                    tracked_job.stopped_time = Some(SystemTime::now());
                }
                JobStatus::Finished => {
                    tracked_job.finished_time = Some(SystemTime::now());
                }
                _ => {}
            }
            tracked_job.status = status;
            if let Some(progress) = progress {
                tracked_job.progress = progress;
            }
            return Ok(());
        }
        bail!("Invalid job id");
    }

    /// Returns a `Vec<String>` containing the IDs of jobs matching status
    fn get_job_ids_by_status(&self, job_status: JobStatus) -> Vec<String> {
        self.jobs
            .iter()
            .filter_map(|(id, tracked_job)| {
                tracked_job.lock().ok().and_then(|locked_job| {
                    if locked_job.status == job_status {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Returns a `Vec<String>` containing the IDs of all completed jobs.
    pub fn get_completed_job_ids(&self) -> Vec<String> {
        self.get_job_ids_by_status(JobStatus::Completed)
    }

    /// Returns a `Vec<String>` containing the IDs of all running jobs.
    pub fn get_running_job_ids(&self) -> Vec<String> {
        self.get_job_ids_by_status(JobStatus::Running)
    }

    /// Returns a `Vec<String>` containing the IDs of all stopped jobs.
    pub fn get_stopped_job_ids(&self) -> Vec<String> {
        self.get_job_ids_by_status(JobStatus::Stopped)
    }

    /// Returns a `Vec<String>` containing the IDs of any running jobs which have timed out.
    pub fn get_timed_out_job_ids(&self) -> Vec<String> {
        let now = SystemTime::now();
        let job_completion_timeout = Duration::from_millis(SETTINGS.core.job_completion_timeout);

        self.jobs
            .iter()
            .filter_map(|(id, tracked_job)| {
                tracked_job.lock().ok().and_then(|locked_job| {
                    let elapsed = now.duration_since(locked_job.start_time).ok()?;

                    if locked_job.status == JobStatus::Running && elapsed > job_completion_timeout {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Returns a `Vec<String>` containing the IDs of all stopped jobs which have been stopped
    /// for longer than the `core.job_removal_timeout` setting.
    pub fn get_stopped_and_expired_job_ids(&self) -> Vec<String> {
        let now = SystemTime::now();
        let stopped_job_cleanup_timeout = Duration::from_millis(SETTINGS.core.job_removal_timeout);

        self.jobs
            .iter()
            .filter_map(|(id, tracked_job)| {
                tracked_job.lock().ok().and_then(|locked_job| {
                    if locked_job.status != JobStatus::Stopped {
                        return None;
                    }

                    let elapsed_since_stopped =
                        now.duration_since(locked_job.stopped_time.unwrap()).ok()?;
                    if elapsed_since_stopped > stopped_job_cleanup_timeout {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
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
        progress: Option<f64>,
        resp: JobTrackerCommandResponder<()>,
    },
    GetRunningJobIds {
        resp: JobTrackerCommandResponder<Vec<String>>,
    },
    GetStoppedJobIds {
        resp: JobTrackerCommandResponder<Vec<String>>,
    },
    GetTimedOutJobIds {
        resp: JobTrackerCommandResponder<Vec<String>>,
    },
    GetCompletedJobIds {
        resp: JobTrackerCommandResponder<Vec<String>>,
    },
    GetStoppedAndExpiredJobIds {
        resp: JobTrackerCommandResponder<Vec<String>>,
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
    progress: Option<f64>,
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

async fn get_job_ids_helper(
    tx: &Sender<JobTrackerCommand>,
    command_factory: impl FnOnce(oneshot::Sender<Result<Vec<String>>>) -> JobTrackerCommand,
) -> Option<Vec<String>> {
    let (resp_tx, resp_rx) = oneshot::channel();
    tx.send(command_factory(resp_tx))
        .await
        .expect("Failed sending command to job tracker");

    resp_rx
        .await
        .expect("Failed getting job ids from channel")
        .ok()
}

pub async fn get_timed_out_job_ids(tx: &Sender<JobTrackerCommand>) -> Option<Vec<String>> {
    get_job_ids_helper(tx, |resp| JobTrackerCommand::GetTimedOutJobIds { resp }).await
}

pub async fn get_running_job_ids(tx: &Sender<JobTrackerCommand>) -> Option<Vec<String>> {
    get_job_ids_helper(tx, |resp| JobTrackerCommand::GetRunningJobIds { resp }).await
}

pub async fn get_stopped_job_ids(tx: &Sender<JobTrackerCommand>) -> Option<Vec<String>> {
    get_job_ids_helper(tx, |resp| JobTrackerCommand::GetStoppedJobIds { resp }).await
}

pub async fn get_completed_job_ids(tx: &Sender<JobTrackerCommand>) -> Option<Vec<String>> {
    get_job_ids_helper(tx, |resp| JobTrackerCommand::GetCompletedJobIds { resp }).await
}

pub async fn get_stopped_and_expired_job_ids(
    tx: &Sender<JobTrackerCommand>,
) -> Option<Vec<String>> {
    get_job_ids_helper(tx, |resp| JobTrackerCommand::GetStoppedAndExpiredJobIds {
        resp,
    })
    .await
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
