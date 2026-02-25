//! Web tools — implement the agent-tools Tool trait for web actions.
//!
//! These tools wrap agent-web's backend and expose web capabilities
//! to the tool framework (and thus to MCP).

use crate::action::WebAction;
use crate::backend::WebBackend;
use krust_agent_tools::tool::{Tool, ToolCall, ToolResult};
use krust_protocol_core::artifact::Evidence;
use serde_json::json;
use std::sync::Arc;

/// Navigate to a URL.
pub struct WebNavigateTool {
    backend: Arc<dyn WebBackend>,
}

impl WebNavigateTool {
    pub fn new(backend: Arc<dyn WebBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl Tool for WebNavigateTool {
    fn name(&self) -> &str {
        "web_navigate"
    }

    fn description(&self) -> &str {
        "Navigate the browser to a URL. Returns the page title and URL after navigation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to navigate to"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        let url = match call.params.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => return ToolResult::error(&call.id, "Missing required parameter: url"),
        };

        match self
            .backend
            .execute(WebAction::Navigate { url: url.clone() })
            .await
        {
            Ok(evidence) => {
                let content = format!(
                    "Navigated to {}. Title: {}",
                    evidence.url.as_deref().unwrap_or("unknown"),
                    evidence.text_content.as_deref().unwrap_or("(none)")
                );
                ToolResult::success(&call.id, content).with_evidence(Evidence::new(
                    "page_loaded",
                    json!({
                        "url": evidence.url,
                        "title": evidence.text_content,
                    }),
                ))
            }
            Err(e) => ToolResult::error(&call.id, format!("Navigation failed: {}", e)),
        }
    }
}

/// Click an element on the page.
pub struct WebClickTool {
    backend: Arc<dyn WebBackend>,
}

impl WebClickTool {
    pub fn new(backend: Arc<dyn WebBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl Tool for WebClickTool {
    fn name(&self) -> &str {
        "web_click"
    }

    fn description(&self) -> &str {
        "Click an element on the page by CSS selector."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector of the element to click"
                }
            },
            "required": ["selector"]
        })
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        let selector = match call.params.get("selector").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error(&call.id, "Missing required parameter: selector"),
        };

        match self
            .backend
            .execute(WebAction::Click {
                selector: selector.clone(),
            })
            .await
        {
            Ok(evidence) => ToolResult::success(&call.id, format!("Clicked element: {}", selector))
                .with_evidence(Evidence::new(
                    "element_clicked",
                    json!({
                        "selector": selector,
                        "url_after": evidence.url,
                    }),
                )),
            Err(e) => ToolResult::error(&call.id, format!("Click failed: {}", e)),
        }
    }
}

/// Type text into an input element.
pub struct WebTypeTool {
    backend: Arc<dyn WebBackend>,
}

impl WebTypeTool {
    pub fn new(backend: Arc<dyn WebBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl Tool for WebTypeTool {
    fn name(&self) -> &str {
        "web_type"
    }

    fn description(&self) -> &str {
        "Type text into an input element identified by CSS selector."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector of the input element"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type into the element"
                }
            },
            "required": ["selector", "text"]
        })
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        let selector = match call.params.get("selector").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error(&call.id, "Missing required parameter: selector"),
        };
        let text = match call.params.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => return ToolResult::error(&call.id, "Missing required parameter: text"),
        };

        match self
            .backend
            .execute(WebAction::Type {
                selector: selector.clone(),
                text: text.clone(),
            })
            .await
        {
            Ok(_evidence) => {
                ToolResult::success(&call.id, format!("Typed '{}' into {}", text, selector))
                    .with_evidence(Evidence::new(
                        "text_typed",
                        json!({
                            "selector": selector,
                            "text": text,
                        }),
                    ))
            }
            Err(e) => ToolResult::error(&call.id, format!("Type failed: {}", e)),
        }
    }
}

/// Take a screenshot of the current page.
pub struct WebScreenshotTool {
    backend: Arc<dyn WebBackend>,
}

impl WebScreenshotTool {
    pub fn new(backend: Arc<dyn WebBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl Tool for WebScreenshotTool {
    fn name(&self) -> &str {
        "web_screenshot"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the current browser page."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        match self.backend.execute(WebAction::Screenshot).await {
            Ok(evidence) => {
                let content = format!(
                    "Screenshot captured. Current URL: {}",
                    evidence.url.as_deref().unwrap_or("unknown")
                );
                let mut result = ToolResult::success(&call.id, content);
                if let Some(b64) = &evidence.screenshot {
                    result = result.with_evidence(Evidence::new(
                        "screenshot",
                        json!({
                            "format": "png",
                            "base64_length": b64.len(),
                        }),
                    ));
                }
                result
            }
            Err(e) => ToolResult::error(&call.id, format!("Screenshot failed: {}", e)),
        }
    }
}

/// Extract text content from the page or a specific element.
pub struct WebExtractTool {
    backend: Arc<dyn WebBackend>,
}

impl WebExtractTool {
    pub fn new(backend: Arc<dyn WebBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait::async_trait]
impl Tool for WebExtractTool {
    fn name(&self) -> &str {
        "web_extract"
    }

    fn description(&self) -> &str {
        "Extract text content from the page or a specific element by CSS selector."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "Optional CSS selector. If omitted, extracts full page content."
                }
            }
        })
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        let selector = call
            .params
            .get("selector")
            .and_then(|v| v.as_str())
            .map(String::from);

        match self.backend.execute(WebAction::Extract { selector }).await {
            Ok(evidence) => {
                let text = evidence.text_content.unwrap_or_default();
                // Truncate for LLM consumption
                let truncated = if text.len() > 10000 {
                    format!(
                        "{}... [truncated, {} total chars]",
                        &text[..10000],
                        text.len()
                    )
                } else {
                    text.clone()
                };

                ToolResult::success(&call.id, truncated).with_evidence(Evidence::new(
                    "text_content",
                    json!({
                        "length": text.len(),
                        "url": evidence.url,
                    }),
                ))
            }
            Err(e) => ToolResult::error(&call.id, format!("Extract failed: {}", e)),
        }
    }
}

/// Register all web tools with a tool registry.
pub fn register_web_tools(
    registry: &mut krust_agent_tools::ToolRegistry,
    backend: Arc<dyn WebBackend>,
) {
    registry.register(Box::new(WebNavigateTool::new(backend.clone())));
    registry.register(Box::new(WebClickTool::new(backend.clone())));
    registry.register(Box::new(WebTypeTool::new(backend.clone())));
    registry.register(Box::new(WebScreenshotTool::new(backend.clone())));
    registry.register(Box::new(WebExtractTool::new(backend)));
}
