use std::sync::LazyLock;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Docker {
    pub url: Option<String>,
    pub start_port: u16,
    pub end_port: u16,
    pub container_timeout: u16,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub docker: Docker,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .set_default("docker.start_port", 49152)?
            .set_default("docker.end_port", 65535)?
            .set_default("docker.container_timeout", 10000)?
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

pub static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    let settings = Settings::new().expect("Failed to load settings");
    settings
});
