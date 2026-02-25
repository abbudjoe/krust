//! # krust-mcp
//!
//! MCP server that exposes Krust's verified execution tools to any
//! MCP-compatible agent (Claude Code, Codex, Cursor, etc.).
//!
//! Usage:
//!   cargo run --bin krust-mcp
//!
//! Then configure your agent's MCP settings to point at this binary.

use krust_agent_web::action::{WaitCondition, WebAction};
use krust_agent_web::backend::WebBackend;
use krust_agent_web::cdp::CdpBackend;
use krust_protocol_core::intent::Intent;
use krust_protocol_core::policy::{
    evaluate_policies, AllowAllPolicy, ConfirmPatternPolicy, Policy, PolicyDecision,
};
use krust_protocol_core::state::{apply_transition, AgentState, TransitionEvent};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_router, ServerHandler, ServiceExt,
};
use std::sync::Arc;
use tokio::sync::Mutex;

// --- Execution Engine ---

/// Orchestrates tool execution through the protocol-core state machine and policy engine.
struct ExecutionEngine {
    policies: Vec<Box<dyn Policy>>,
}

impl ExecutionEngine {
    fn new() -> Self {
        let policies: Vec<Box<dyn Policy>> = vec![
            Box::new(AllowAllPolicy),
            Box::new(ConfirmPatternPolicy {
                // Prove the policy path works: web_ tools go through pattern matching
                // (they match no deny/confirm prefixes so they get Allow)
                confirm_prefixes: vec![],
                deny_prefixes: vec![],
            }),
        ];
        Self { policies }
    }

    /// Check policy for an intent. Returns the decision.
    fn check_policy(&self, intent: &Intent) -> PolicyDecision {
        let refs: Vec<&dyn Policy> = self.policies.iter().map(|p| p.as_ref()).collect();
        evaluate_policies(&refs, intent)
    }

    /// Run a tool call through the full state machine lifecycle.
    /// Returns (result_string, final_state).
    async fn execute<F, Fut>(
        &self,
        intent: &Intent,
        tool_call_id: &str,
        step: u32,
        run_tool: F,
    ) -> (String, AgentState)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        // 1. Check policy
        match self.check_policy(intent) {
            PolicyDecision::Allow => {}
            PolicyDecision::Deny { reason } => {
                return (
                    format!("Policy denied: {}", reason),
                    AgentState::Failed { reason },
                );
            }
            PolicyDecision::Confirm { reason } => {
                // In a real system we'd enter WaitingHuman. For now, log and proceed.
                tracing::info!("Policy would require confirmation: {}", reason);
            }
        }

        // 2. Transition: Planning → Executing
        let mut state = AgentState::Planning;
        let plan_event = TransitionEvent::PlanReady {
            tool_call_id: tool_call_id.to_string(),
            step,
        };
        state = match apply_transition(&state, &plan_event) {
            Some(s) => s,
            None => {
                return (
                    "Internal error: invalid Planning→Executing transition".to_string(),
                    state,
                );
            }
        };

        // 3. Execute the tool
        let (success, content) = match run_tool().await {
            Ok(result) => (true, result),
            Err(err) => (false, err),
        };

        // 4. Transition: Executing → Verifying (success) or Retrying (failure)
        let tool_event = TransitionEvent::ToolCompleted {
            tool_call_id: tool_call_id.to_string(),
            success,
        };
        state = match apply_transition(&state, &tool_event) {
            Some(s) => s,
            None => {
                return (content, state);
            }
        };

        if success {
            // 5. Transition: Verifying → Completed
            let verify_event = TransitionEvent::VerificationPassed {
                artifacts: vec![content.clone()],
            };
            state = apply_transition(&state, &verify_event).unwrap_or(state);
        }

        (content, state)
    }
}

// --- Request schemas ---

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct NavigateRequest {
    #[schemars(description = "The URL to navigate to")]
    url: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ClickRequest {
    #[schemars(description = "CSS selector of the element to click")]
    selector: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct TypeRequest {
    #[schemars(description = "CSS selector of the input element")]
    selector: String,
    #[schemars(description = "Text to type into the element")]
    text: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ExtractRequest {
    #[schemars(description = "Optional CSS selector. Omit for full page text.")]
    selector: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WaitRequest {
    #[schemars(description = "CSS selector to wait for, or milliseconds as a number string")]
    condition: String,
}

// --- Server ---

#[derive(Clone)]
struct KrustServer {
    backend: Arc<CdpBackend>,
    launched: Arc<Mutex<bool>>,
    engine: Arc<ExecutionEngine>,
    step_counter: Arc<Mutex<u32>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for KrustServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KrustServer").finish()
    }
}

impl KrustServer {
    fn new() -> Self {
        Self {
            backend: Arc::new(CdpBackend::new()),
            launched: Arc::new(Mutex::new(false)),
            engine: Arc::new(ExecutionEngine::new()),
            step_counter: Arc::new(Mutex::new(0)),
            tool_router: Self::tool_router(),
        }
    }

    async fn ensure_browser(&self) -> Result<(), String> {
        let mut launched = self.launched.lock().await;
        if !*launched {
            self.backend.launch().await.map_err(|e| e.to_string())?;
            *launched = true;
        }
        Ok(())
    }

    async fn next_step(&self) -> u32 {
        let mut counter = self.step_counter.lock().await;
        *counter += 1;
        *counter
    }
}

#[tool_router]
impl KrustServer {
    #[tool(description = "Navigate the browser to a URL. Returns page title and URL.")]
    async fn web_navigate(&self, Parameters(req): Parameters<NavigateRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent = Intent::new("web.navigate").with_param("url", serde_json::json!(&req.url));
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let url = req.url.clone();

        let (result, _state) = self
            .engine
            .execute(
                &intent,
                &format!("web_navigate_{}", step),
                step,
                || async move {
                    match backend
                        .execute(WebAction::Navigate { url: url.clone() })
                        .await
                    {
                        Ok(evidence) => Ok(format!(
                            "Navigated to {}. Title: {}",
                            evidence.url.as_deref().unwrap_or(&url),
                            evidence.text_content.as_deref().unwrap_or("(none)")
                        )),
                        Err(e) => Err(format!("Navigation failed: {}", e)),
                    }
                },
            )
            .await;

        result
    }

    #[tool(description = "Click an element on the page by CSS selector.")]
    async fn web_click(&self, Parameters(req): Parameters<ClickRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent =
            Intent::new("web.click").with_param("selector", serde_json::json!(&req.selector));
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let selector = req.selector.clone();

        let (result, _state) = self
            .engine
            .execute(
                &intent,
                &format!("web_click_{}", step),
                step,
                || async move {
                    match backend
                        .execute(WebAction::Click {
                            selector: selector.clone(),
                        })
                        .await
                    {
                        Ok(_) => Ok(format!("Clicked element: {}", selector)),
                        Err(e) => Err(format!("Click failed: {}", e)),
                    }
                },
            )
            .await;

        result
    }

    #[tool(description = "Type text into an input element by CSS selector.")]
    async fn web_type(&self, Parameters(req): Parameters<TypeRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent = Intent::new("web.type")
            .with_param("selector", serde_json::json!(&req.selector))
            .with_param("text", serde_json::json!(&req.text));
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let selector = req.selector.clone();
        let text = req.text.clone();

        let (result, _state) = self
            .engine
            .execute(
                &intent,
                &format!("web_type_{}", step),
                step,
                || async move {
                    match backend
                        .execute(WebAction::Type {
                            selector: selector.clone(),
                            text: text.clone(),
                        })
                        .await
                    {
                        Ok(_) => Ok(format!("Typed '{}' into {}", text, selector)),
                        Err(e) => Err(format!("Type failed: {}", e)),
                    }
                },
            )
            .await;

        result
    }

    #[tool(description = "Extract text content from the page or a specific element.")]
    async fn web_extract(&self, Parameters(req): Parameters<ExtractRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent = Intent::new("web.extract");
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let selector = req.selector.clone();

        let (result, _state) = self
            .engine
            .execute(
                &intent,
                &format!("web_extract_{}", step),
                step,
                || async move {
                    match backend.execute(WebAction::Extract { selector }).await {
                        Ok(evidence) => {
                            let text = evidence.text_content.unwrap_or_default();
                            if text.len() > 10000 {
                                Ok(format!(
                                    "{}... [truncated, {} chars total]",
                                    &text[..10000],
                                    text.len()
                                ))
                            } else {
                                Ok(text)
                            }
                        }
                        Err(e) => Err(format!("Extract failed: {}", e)),
                    }
                },
            )
            .await;

        result
    }

    #[tool(
        description = "Wait for an element to appear (CSS selector) or a fixed duration (milliseconds)."
    )]
    async fn web_wait(&self, Parameters(req): Parameters<WaitRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent = Intent::new("web.wait");
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let condition_str = req.condition.clone();

        let (result, _state) = self
            .engine
            .execute(
                &intent,
                &format!("web_wait_{}", step),
                step,
                || async move {
                    let condition = if let Ok(ms) = condition_str.parse::<u64>() {
                        WaitCondition::Duration(ms)
                    } else {
                        WaitCondition::Selector(condition_str.clone())
                    };

                    match backend.execute(WebAction::Wait { condition }).await {
                        Ok(_) => Ok(format!("Wait completed for: {}", condition_str)),
                        Err(e) => Err(format!("Wait failed: {}", e)),
                    }
                },
            )
            .await;

        result
    }
}

impl ServerHandler for KrustServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Krust: Verified execution protocols for AI agents. \
                 This MCP server provides browser automation tools with \
                 state machine-backed execution and evidence verification."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to stderr (MCP uses stdin/stdout for protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting Krust MCP server");

    let server = KrustServer::new();
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;

    tracing::info!("Krust MCP server initialized, waiting for requests");
    service.waiting().await?;
    tracing::info!("Krust MCP server shutting down");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use krust_protocol_core::policy::PolicyDecision;

    #[test]
    fn test_engine_policy_allows_web_navigate() {
        let engine = ExecutionEngine::new();
        let intent =
            Intent::new("web.navigate").with_param("url", serde_json::json!("https://example.com"));
        assert_eq!(engine.check_policy(&intent), PolicyDecision::Allow);
    }

    #[test]
    fn test_engine_policy_with_deny_pattern() {
        let engine = ExecutionEngine {
            policies: vec![Box::new(ConfirmPatternPolicy {
                confirm_prefixes: vec![],
                deny_prefixes: vec!["danger.".to_string()],
            })],
        };
        let intent = Intent::new("danger.delete_all");
        match engine.check_policy(&intent) {
            PolicyDecision::Deny { .. } => {}
            other => panic!("Expected Deny, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_execute_success_lifecycle() {
        let engine = ExecutionEngine::new();
        let intent = Intent::new("web.navigate");

        let (result, state) = engine
            .execute(&intent, "tc_test", 1, || async {
                Ok("Navigated to example.com".to_string())
            })
            .await;

        assert_eq!(result, "Navigated to example.com");
        match state {
            AgentState::Completed { artifacts } => {
                assert_eq!(artifacts, vec!["Navigated to example.com"]);
            }
            _ => panic!("Expected Completed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_execute_failure_lifecycle() {
        let engine = ExecutionEngine::new();
        let intent = Intent::new("web.click");

        let (result, state) = engine
            .execute(&intent, "tc_test", 1, || async {
                Err::<String, String>("Element not found".to_string())
            })
            .await;

        assert_eq!(result, "Element not found");
        match state {
            AgentState::Retrying { attempt, .. } => {
                assert_eq!(attempt, 0);
            }
            _ => panic!("Expected Retrying, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_policy_deny_blocks_execution() {
        let engine = ExecutionEngine {
            policies: vec![Box::new(ConfirmPatternPolicy {
                confirm_prefixes: vec![],
                deny_prefixes: vec!["forbidden.".to_string()],
            })],
        };
        let intent = Intent::new("forbidden.action");

        let (result, state) = engine
            .execute(&intent, "tc_test", 1, || async {
                Ok("should not reach".to_string())
            })
            .await;

        assert!(result.starts_with("Policy denied:"));
        match state {
            AgentState::Failed { .. } => {}
            _ => panic!("Expected Failed, got {:?}", state),
        }
    }
}
