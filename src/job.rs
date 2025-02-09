use serde::Deserialize;
use serde_json::Value;

use crate::env::EnvVars;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DockerJob {
    pub id: String,
    pub image: String,
    pub port: u16,
    pub command: Option<Vec<String>>,
    pub body: Value,
    pub env: Option<EnvVars>,
    pub callback_url: String,
    pub always_pull: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
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
                assert_eq!(env, Some(test_env));
                assert_eq!(callback_url, "https://api.example.com/callback");
                assert_eq!(always_pull, true);
            }
            _ => panic!("Invalid job variant"),
        }
    }
}
