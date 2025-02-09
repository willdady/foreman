use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
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

    /// Merge the given EnvVars into this one, overriding any existing keys
    pub fn merge(&mut self, other: EnvVars) {
        self.0.extend(other.0);
    }

    /// Combine the given EnvVars with this one, overriding any existing keys.
    /// Returns a new EnvVars instance.
    pub fn merge_clone(&self, other: &EnvVars) -> Self {
        let mut new_map = HashMap::with_capacity(self.0.len() + other.0.len());
        new_map.extend(self.0.iter().map(|(k, v)| (k.clone(), v.clone())));
        new_map.extend(other.0.iter().map(|(k, v)| (k.clone(), v.clone())));
        EnvVars(new_map)
    }
}

impl From<EnvVars> for Vec<String> {
    /// Convert EnvVars to Vec<String> where each string is formatted as "Key=Value"
    fn from(env_vars: EnvVars) -> Self {
        env_vars
            .inner()
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }
}
