mod docker;

pub use docker::*;

use anyhow::Result;

use crate::job::Job;

pub trait JobExecutor {
    async fn execute(&mut self, job: Job) -> Result<()>;
    async fn stop(&mut self, job_id: &str) -> Result<()>;
}

pub enum JobExecutorCommand {
    Execute { job: Job },
    Stop { job_id: String },
}
