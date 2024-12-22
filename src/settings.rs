use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(unused)]
struct Docker {
    socket: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    docker: Docker,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("config.toml"))
            .add_source(
                Environment::with_prefix("vs")
                    .prefix_separator("_")
                    .separator("_"),
            )
            .build()?;

        s.try_deserialize()
    }
}
