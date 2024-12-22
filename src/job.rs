use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum DockerJobHTTPMethod {
    #[serde(alias = "post")]
    POST,
    #[serde(alias = "put")]
    PUT,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct EnvVars(HashMap<String, String>);

impl EnvVars {
    pub fn new() -> Self {
        EnvVars(HashMap::new())
    }

    pub fn inner(&self) -> &HashMap<String, String> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.0
    }
}

impl From<EnvVars> for Vec<String> {
    /// Convert EnvVars to Vec<String> where each string is formatted as "Key=Value"
    fn from(env_vars: EnvVars) -> Self {
        env_vars
            .inner()
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub struct DockerJob {
    pub id: String,
    pub image: String,
    pub port: u16,
    pub command: Option<Vec<String>>,
    pub body: Value,
    pub method: DockerJobHTTPMethod,
    pub env: Option<EnvVars>,
    pub callback_url: String,
    pub always_pull: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all_fields = "camelCase")]
pub enum Job {
    #[serde(rename = "docker")]
    Docker(DockerJob),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_deserialize_docker_job() {
        let json = r#"{
            "type": "docker",
            "id": "123abc",
            "image": "alpine:latest",
            "port": 8080,
            "command": ["echo", "Hello world!"],
            "body": {
                "foo": "bar",
                "eggs": "spam"
            },
            "env": {
                "NODE_ENV": "development"
            },
            "method": "POST",
            "callbackUrl": "https://api.example.com/callback",
            "alwaysPull": true
        }"#;

        let job: Job = serde_json::from_str(json).unwrap();

        match job {
            Job::Docker(DockerJob {
                id,
                image,
                port,
                command,
                body,
                method,
                env,
                callback_url,
                always_pull,
            }) => {
                let mut test_env = EnvVars::new();
                test_env
                    .inner_mut()
                    .insert("NODE_ENV".to_string(), "development".to_string());

                assert_eq!(id, "123abc");
                assert_eq!(image, "alpine:latest");
                assert_eq!(port, 8080);
                assert_eq!(
                    command,
                    Some(vec!["echo".to_string(), "Hello world!".to_string()])
                );
                assert_eq!(body, json!({ "foo": "bar", "eggs": "spam" }));
                assert_eq!(method, DockerJobHTTPMethod::POST);
                assert_eq!(env, Some(test_env));
                assert_eq!(callback_url, "https://api.example.com/callback");
                assert_eq!(always_pull, true);
            }
            _ => panic!("Invalid job variant"),
        }
    }
}
