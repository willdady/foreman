use std::sync::LazyLock;
use std::{env, path::Path};

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

const CONFIG_FILE_NAME: &'static str = "foreman.toml";

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Core {
    pub url: String,
    pub token: String,
    pub poll_frequency: u16,
    pub poll_timeout: u16,
}

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
    pub core: Core,
    pub docker: Docker,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let path_exists = |path: Option<String>| {
            if let Some(_path) = path {
                if Path::new(_path.as_str()).exists() {
                    Some(_path)
                } else {
                    None
                }
            } else {
                None
            }
        };

        let config_path_string = if let Ok(val) = env::var("FOREMAN_CONFIG") {
            let p = path_exists(Some(val));
            p.expect("File path defined in FOREMAN_CONFIG environment variable does not exist")
        } else {
            path_exists(Some(CONFIG_FILE_NAME.to_string()))
                .or_else(|| {
                    let home_dir = dirs::home_dir().expect("Failed to get home directory");
                    let home_config_file_path = home_dir.join(".foreman").join(CONFIG_FILE_NAME);
                    let home_config_file_path_string =
                        format!("{}", home_config_file_path.display());
                    path_exists(Some(home_config_file_path_string))
                })
                .expect("Could config file not found")
        };

        let s = Config::builder()
            .set_default("core.poll_frequency", 5000)?
            .set_default("core.poll_timeout", 30000)?
            .set_default("docker.start_port", 49152)?
            .set_default("docker.end_port", 65535)?
            .set_default("docker.container_timeout", 10000)?
            .add_source(File::with_name(&config_path_string).required(false))
            .add_source(
                Environment::with_prefix("foreman")
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
