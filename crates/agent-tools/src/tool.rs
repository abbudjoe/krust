//! Core tool trait and types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use krust_protocol_core::artifact::Evidence;

/// A call to a specific tool with parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this call.
    pub id: String,
    /// Tool name (e.g., "web_navigate", "web_click").
    pub name: String,
    /// Parameters for the tool.
    pub params: HashMap<String, serde_json::Value>,
}

/// Result from a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The tool call this result is for.
    pub call_id: String,
    /// Whether the tool reports success.
    pub success: bool,
    /// Human-readable output/content.
    pub content: String,
    /// Evidence collected during execution (for artifact verification).
    pub evidence: Vec<Evidence>,
    /// Whether this result is an error.
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            success: true,
            content: content.into(),
            evidence: Vec::new(),
            is_error: false,
        }
    }

    pub fn error(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            success: false,
            content: content.into(),
            evidence: Vec::new(),
            is_error: true,
        }
    }

    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }
}

/// Trait that all tools implement.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Tool name as exposed to the agent.
    fn name(&self) -> &str;

    /// Human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given parameters.
    async fn execute(&self, call: &ToolCall) -> ToolResult;
}
