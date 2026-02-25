//! Web action primitives.

use serde::{Deserialize, Serialize};

/// A web action the agent wants to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum WebAction {
    Navigate { url: String },
    Click { selector: String },
    Type { selector: String, text: String },
    PressKey { key: String },
    Extract { selector: Option<String> },
    Screenshot { output_path: Option<String> },
    Wait { condition: WaitCondition },
    Back,
    Forward,
}

/// Condition to wait for before proceeding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WaitCondition {
    /// Wait for a specific selector to appear.
    Selector(String),
    /// Wait for navigation to complete.
    Navigation,
    /// Wait for a fixed duration (milliseconds).
    Duration(u64),
}
