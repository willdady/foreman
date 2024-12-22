mod docker;

pub use docker::*;

use anyhow::Result;

use crate::job::Job;

pub trait Executor {
    async fn execute(&mut self, job: Job) -> Result<()>;
}
