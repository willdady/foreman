use std::collections::HashMap;
use std::sync::LazyLock;
use std::{env, path::Path};

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use urlencoding::encode;

const CONFIG_FILE_NAME: &str = "foreman.toml";

#[derive(Debug, Deserialize)]
pub struct LabelMap(HashMap<String, String>);

impl LabelMap {
    pub fn new() -> Self {
        LabelMap(HashMap::new())
    }
}

impl From<&LabelMap> for String {
    /// Convert a `LabelMap` to a string in the format "key=value,key=value".
    /// Both keys and values are URL-encoded.
    fn from(label_map: &LabelMap) -> Self {
        label_map
            .0
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<String>>()
            .join(",")
    }
}

impl Default for LabelMap {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Core {
    pub url: String,
    pub hostname: String,
    pub port: u16,
    pub network_name: String,
    pub token: String,
    pub poll_frequency: u16,
    pub poll_timeout: u16,
    pub extra_hosts: Option<Vec<String>>,
    pub labels: Option<LabelMap>,
    pub job_completion_timeout: u64,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Docker {
    pub url: Option<String>,
    pub start_port: u16,
    pub end_port: u16,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub core: Core,
    pub docker: Docker,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let path_exists =
            |path: Option<String>| path.filter(|_path| Path::new(_path.as_str()).exists());

        let config_path_string = if let Ok(val) = env::var("FOREMAN_CONFIG") {
            path_exists(Some(val)).unwrap_or_else(|| {
                eprintln!(
                    "ERROR: File path defined in FOREMAN_CONFIG environment variable does not exist"
                );
                std::process::exit(1);
            })
        } else {
            path_exists(Some(CONFIG_FILE_NAME.to_string()))
                .or_else(|| {
                    let home_dir = dirs::home_dir().expect("Failed to get home directory");
                    let home_config_file_path = home_dir.join(".foreman").join(CONFIG_FILE_NAME);
                    let home_config_file_path_string =
                        format!("{}", home_config_file_path.display());
                    path_exists(Some(home_config_file_path_string))
                })
                .unwrap_or_else(|| {
                    eprintln!("ERROR: Unable to find {}", CONFIG_FILE_NAME);
                    std::process::exit(1);
                })
        };

        let s = Config::builder()
            .set_default("core.poll_frequency", 5_000)?
            .set_default("core.poll_timeout", 30_000)?
            .set_default("core.port", 3000)?
            .set_default("core.network_name", "foreman")?
            .set_default("core.job_completion_timeout", 10_000)?
            .set_default("docker.start_port", 49152)?
            .set_default("docker.end_port", 65535)?
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

pub static SETTINGS: LazyLock<Settings> =
    LazyLock::new(|| Settings::new().expect("Failed to load settings"));
