//! Typed intents for agent actions.
//!
//! An intent describes what an agent wants to accomplish at a high level,
//! decoupled from how it's executed. Intents carry structured parameters
//! and can be validated against schemas before execution.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A typed intent describing an agent's desired action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Unique intent type identifier (e.g., "web.navigate", "web.click", "app.open").
    pub kind: String,

    /// Structured parameters for this intent.
    pub params: HashMap<String, serde_json::Value>,

    /// Optional human-readable description of what this intent accomplishes.
    pub description: Option<String>,

    /// Expected artifacts that should be produced by this intent.
    pub expected_artifacts: Vec<String>,
}

impl Intent {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            params: HashMap::new(),
            description: None,
            expected_artifacts: Vec::new(),
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_expected_artifact(mut self, artifact: impl Into<String>) -> Self {
        self.expected_artifacts.push(artifact.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_intent_builder() {
        let intent = Intent::new("web.navigate")
            .with_param("url", json!("https://flights.google.com"))
            .with_description("Navigate to Google Flights")
            .with_expected_artifact("page_loaded");

        assert_eq!(intent.kind, "web.navigate");
        assert_eq!(intent.params["url"], json!("https://flights.google.com"));
        assert_eq!(intent.expected_artifacts, vec!["page_loaded"]);
    }

    #[test]
    fn test_intent_serialization() {
        let intent = Intent::new("web.click")
            .with_param("selector", json!("#search-btn"))
            .with_description("Click search button");

        let json = serde_json::to_string(&intent).unwrap();
        let deserialized: Intent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.kind, "web.click");
    }
}
