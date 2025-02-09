use std::collections::HashMap;
use std::sync::LazyLock;
use std::{env, path::Path};

use config::{
    Config, ConfigError, Environment, File, FileFormat, FileSourceFile, FileSourceString,
};
use serde::Deserialize;
use urlencoding::encode;

use crate::env::EnvVars;

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

/// Resolves the configuration file by checking the following locations in order:
///
/// 1. The path specified by the `FOREMAN_CONFIG` environment variable
/// 2. ./foreman.toml
/// 3. /etc/foreman/foreman.toml
/// 4. $HOME/.foreman/foreman.toml
fn get_config_file() -> Option<File<FileSourceFile, FileFormat>> {
    // If FOREMAN_CONFIG environment variable is set and it points to a valid file, use that.
    // Otherwise panic!
    if let Ok(val) = env::var("FOREMAN_CONFIG") {
        if Path::new(&val).exists() {
            return Some(File::with_name(&val));
        } else {
            panic!("File path defined in FOREMAN_CONFIG environment variable does not exist");
        }
    }

    // If file exists in current directory, use that.
    if Path::new("foreman.toml").exists() {
        return Some(File::with_name("foreman.toml"));
    }

    // If file exists at path /etc/foreman/foreman.toml, use that.
    if Path::new("/etc/foreman/foreman.toml").exists() {
        return Some(File::with_name("/etc/foreman/foreman.toml"));
    }

    // If file exists at path ~/.foreman/foreman.toml, use that.
    if let Some(home_dir) = dirs::home_dir() {
        let p = &home_dir.join(".foreman/foreman.toml");
        if Path::new(p).exists() {
            return Some(File::with_name(p.to_string_lossy().to_string().as_str()));
        }
    }

    None
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
    pub job_removal_timeout: u64,
    pub remove_stopped_containers_on_terminate: bool,
    pub max_concurrent_jobs: u64,
    pub env: Option<EnvVars>,
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
        // Create config builder and set defaults
        let mut config_builder = Config::builder()
            .set_default("core.poll_frequency", 5_000)?
            .set_default("core.poll_timeout", 30_000)?
            .set_default("core.port", 3000)?
            .set_default("core.network_name", "foreman")?
            .set_default("core.job_completion_timeout", 10_000)?
            .set_default("core.job_removal_timeout", 5_000)?
            .set_default("core.remove_stopped_containers_on_terminate", true)?
            .set_default("core.max_concurrent_jobs", 12)?
            .set_default("docker.start_port", 49152)?
            .set_default("docker.end_port", 65535)?;

        // Resolve the path to our `foreman.toml` config file (if it exists) and add it
        // to the config builder.
        if let Some(config_file) = get_config_file() {
            config_builder = config_builder.add_source(config_file.required(false));
        }

        let config = config_builder
            // Add environment variables source to the config builder
            .add_source(
                Environment::with_prefix("foreman")
                    .prefix_separator("_")
                    .separator("_"),
            )
            .build()?;

        // Deserialize the config into our Settings struct
        config.try_deserialize()
    }
}

pub static SETTINGS: LazyLock<Settings> =
    LazyLock::new(|| Settings::new().expect("Failed to load foreman settings"));
