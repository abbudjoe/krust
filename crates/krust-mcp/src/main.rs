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
use krust_agent_web::cdp::{detect_chrome_path, CdpBackend};
use krust_protocol_core::artifact::{
    ArtifactContract, Evidence, RequiredEvidenceContract, VerificationResult,
};
use krust_protocol_core::intent::Intent;
use krust_protocol_core::policy::{
    evaluate_policies, AllowAllPolicy, ConfirmPatternPolicy, Policy, PolicyDecision,
};
use krust_protocol_core::state::{apply_transition, AgentState, TransitionEvent};
use rmcp::service::RequestContext;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolRequestParam, CallToolResult, ListToolsResult, PaginatedRequestParam,
        ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

// --- Execution Engine ---

const DEFAULT_MAX_EXECUTION_LOOP_ITERATIONS: usize = 128;
const DEFAULT_MAX_EXECUTION_WALL_CLOCK_BUDGET: Duration = Duration::from_secs(120);

/// Orchestrates tool execution through the protocol-core state machine and policy engine.
struct ExecutionEngine {
    policies: Vec<Box<dyn Policy>>,
    max_loop_iterations: usize,
    max_wall_clock_budget: Duration,
}

#[derive(Debug, Clone)]
struct ToolExecution {
    content: String,
    evidence: Vec<Evidence>,
}

impl ToolExecution {
    fn new(content: impl Into<String>, evidence: Vec<Evidence>) -> Self {
        Self {
            content: content.into(),
            evidence,
        }
    }
}

impl ExecutionEngine {
    fn new() -> Self {
        let policies: Vec<Box<dyn Policy>> = vec![
            Box::new(AllowAllPolicy),
            Box::new(ConfirmPatternPolicy {
                // Keep meaningful patterns configured so this policy is not a no-op.
                // evaluate_policies checks all policies, so this still takes effect even
                // when AllowAllPolicy appears first.
                confirm_prefixes: vec!["payment.".to_string(), "email.send".to_string()],
                deny_prefixes: vec!["danger.".to_string()],
            }),
        ];
        Self::with_policies(policies)
    }

    fn with_policies(policies: Vec<Box<dyn Policy>>) -> Self {
        Self {
            policies,
            max_loop_iterations: DEFAULT_MAX_EXECUTION_LOOP_ITERATIONS,
            max_wall_clock_budget: DEFAULT_MAX_EXECUTION_WALL_CLOCK_BUDGET,
        }
    }

    #[cfg(test)]
    fn with_limits(
        policies: Vec<Box<dyn Policy>>,
        max_loop_iterations: usize,
        max_wall_clock_budget: Duration,
    ) -> Self {
        Self {
            policies,
            max_loop_iterations,
            max_wall_clock_budget,
        }
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
        contract: Option<&dyn ArtifactContract>,
        mut run_tool: F,
    ) -> (String, AgentState)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<ToolExecution, String>>,
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
                let blocked_reason = format!(
                    "Policy requires human confirmation but no approval channel is configured: {}",
                    reason
                );
                return (
                    blocked_reason.clone(),
                    AgentState::Failed {
                        reason: blocked_reason,
                    },
                );
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

        let started_at = Instant::now();
        let mut loop_iterations = 0usize;
        let mut last_content = String::new();

        loop {
            loop_iterations += 1;

            if loop_iterations > self.max_loop_iterations {
                state = AgentState::Failed {
                    reason: format!(
                        "Execution aborted: retry safety budget exceeded ({} iterations)",
                        self.max_loop_iterations
                    ),
                };
                return (last_content, state);
            }

            let elapsed = started_at.elapsed();
            if elapsed > self.max_wall_clock_budget {
                state = AgentState::Failed {
                    reason: format!(
                        "Execution aborted: retry safety wall-clock budget exceeded ({:?})",
                        self.max_wall_clock_budget
                    ),
                };
                return (last_content, state);
            }

            let remaining_budget = self.max_wall_clock_budget.saturating_sub(elapsed);
            if remaining_budget.is_zero() {
                state = AgentState::Failed {
                    reason: format!(
                        "Execution aborted: retry safety wall-clock budget exhausted ({:?})",
                        self.max_wall_clock_budget
                    ),
                };
                return (last_content, state);
            }

            // 3. Execute the tool with per-attempt wall-clock enforcement.
            let execution = match tokio::time::timeout(remaining_budget, run_tool()).await {
                Ok(Ok(execution)) => execution,
                Ok(Err(err)) => {
                    last_content = err;

                    let tool_event = TransitionEvent::ToolCompleted {
                        tool_call_id: tool_call_id.to_string(),
                        success: false,
                    };
                    state = match apply_transition(&state, &tool_event) {
                        Some(s) => s,
                        None => return (last_content.clone(), state),
                    };

                    match &state {
                        AgentState::Retrying {
                            attempt,
                            max_attempts,
                            ..
                        } => {
                            if *attempt >= *max_attempts {
                                let exhausted = TransitionEvent::RetriesExhausted {
                                    reason: format!(
                                        "Tool failed after {} attempts: {}",
                                        *max_attempts + 1,
                                        last_content.clone()
                                    ),
                                };
                                state = apply_transition(&state, &exhausted).unwrap_or(state);
                                return (last_content.clone(), state);
                            }

                            let retry = TransitionEvent::RetryRequested {
                                max_attempts: *max_attempts,
                            };
                            state = match apply_transition(&state, &retry) {
                                Some(s) => s,
                                None => {
                                    let exhausted = TransitionEvent::RetriesExhausted {
                                        reason: format!(
                                            "Tool failed after retries exhausted: {}",
                                            last_content.clone()
                                        ),
                                    };
                                    state = apply_transition(&state, &exhausted).unwrap_or(state);
                                    return (last_content.clone(), state);
                                }
                            };
                            continue;
                        }
                        _ => return (last_content.clone(), state),
                    }
                }
                Err(_) => {
                    let timeout_reason = format!(
                        "Execution aborted: tool attempt timed out after {:?} (remaining wall-clock budget)",
                        remaining_budget
                    );
                    state = AgentState::Failed {
                        reason: timeout_reason.clone(),
                    };
                    return (timeout_reason, state);
                }
            };

            last_content = execution.content.clone();

            // 4. Transition: Executing → Verifying
            let tool_event = TransitionEvent::ToolCompleted {
                tool_call_id: tool_call_id.to_string(),
                success: true,
            };
            state = match apply_transition(&state, &tool_event) {
                Some(s) => s,
                None => return (last_content.clone(), state),
            };

            // 5. Verify artifacts
            let verification_result = if let Some(contract) = contract {
                contract.verify(&execution.evidence)
            } else {
                VerificationResult::Passed {
                    artifacts: vec![execution.content.clone()],
                }
            };

            match verification_result {
                VerificationResult::Passed { artifacts } => {
                    let verify_event = TransitionEvent::VerificationPassed { artifacts };
                    state = apply_transition(&state, &verify_event).unwrap_or(state);
                    return (last_content.clone(), state);
                }
                VerificationResult::Failed { reason } => {
                    let verify_event = TransitionEvent::VerificationFailed {
                        reason: format!("Artifact verification failed: {}", reason),
                    };
                    state = match apply_transition(&state, &verify_event) {
                        Some(s) => s,
                        None => return (last_content.clone(), state),
                    };
                }
                VerificationResult::Insufficient { missing } => {
                    let verify_event = TransitionEvent::VerificationFailed {
                        reason: format!(
                            "Artifact verification insufficient evidence: {:?}",
                            missing
                        ),
                    };
                    state = match apply_transition(&state, &verify_event) {
                        Some(s) => s,
                        None => return (last_content.clone(), state),
                    };
                }
            }

            // 6. Retry loop after verification failure
            match &state {
                AgentState::Retrying {
                    attempt,
                    max_attempts,
                    ..
                } => {
                    if *attempt >= *max_attempts {
                        let exhausted = TransitionEvent::RetriesExhausted {
                            reason: format!(
                                "Verification failed after {} attempts",
                                *max_attempts + 1
                            ),
                        };
                        state = apply_transition(&state, &exhausted).unwrap_or(state);
                        return (last_content.clone(), state);
                    }

                    let retry = TransitionEvent::RetryRequested {
                        max_attempts: *max_attempts,
                    };
                    state = match apply_transition(&state, &retry) {
                        Some(s) => s,
                        None => {
                            let exhausted = TransitionEvent::RetriesExhausted {
                                reason: "Retry transition invalid after verification failure"
                                    .to_string(),
                            };
                            state = apply_transition(&state, &exhausted).unwrap_or(state);
                            return (last_content.clone(), state);
                        }
                    };
                }
                _ => return (last_content.clone(), state),
            }
        }
    }
}

fn required_evidence_contract(kinds: &[&str], description: &str) -> RequiredEvidenceContract {
    RequiredEvidenceContract {
        required_kinds: kinds.iter().map(|k| (*k).to_string()).collect(),
        description: description.to_string(),
    }
}

fn finalize_execution_result(result: String, state: AgentState) -> String {
    match state {
        AgentState::Completed { .. } => result,
        AgentState::Failed { reason } => {
            if result.trim().is_empty() || result == reason {
                format!("Error: {}", reason)
            } else {
                format!("Error: {} (last tool output: {})", reason, result)
            }
        }
        AgentState::Cancelled { reason } => format!("Error: execution cancelled: {}", reason),
        other => {
            if result.trim().is_empty() {
                format!("Error: execution ended in unexpected state: {:?}", other)
            } else {
                format!(
                    "Error: execution ended in unexpected state: {:?} (last tool output: {})",
                    other, result
                )
            }
        }
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
struct PressKeyRequest {
    #[schemars(description = "Key to press: Enter, Tab, Escape, ArrowDown, ArrowUp, Backspace, etc.")]
    key: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ScreenshotRequest {
    #[schemars(description = "Optional file path to save screenshot. Defaults to /tmp/krust-screenshot-<timestamp>.png")]
    output_path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WaitRequest {
    #[schemars(description = "CSS selector to wait for, or milliseconds as a number string")]
    condition: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchRequest {
    #[schemars(description = "Search query")]
    query: String,
    #[schemars(description = "Maximum number of results (default: 5)")]
    count: Option<u32>,
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
        let contract = required_evidence_contract(&["page_loaded"], "Page must be loaded");

        let (result, state) = self
            .engine
            .execute(&intent, &format!("web_navigate_{}", step), step, Some(&contract), || async {
                match backend.execute(WebAction::Navigate { url: url.clone() }).await {
                    Ok(evidence) => Ok(ToolExecution::new(
                        format!(
                            "Navigated to {}. Title: {}",
                            evidence.url.as_deref().unwrap_or(&url),
                            evidence.text_content.as_deref().unwrap_or("(none)")
                        ),
                        vec![Evidence::new(
                            "page_loaded",
                            serde_json::json!({"url": evidence.url, "title": evidence.text_content}),
                        )],
                    )),
                    Err(e) => Err(format!("Navigation failed: {}", e)),
                }
            })
            .await;

        finalize_execution_result(result, state)
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
        let contract = required_evidence_contract(&["element_clicked"], "Click evidence required");

        let (result, state) = self
            .engine
            .execute(
                &intent,
                &format!("web_click_{}", step),
                step,
                Some(&contract),
                || async {
                    match backend
                        .execute(WebAction::Click {
                            selector: selector.clone(),
                        })
                        .await
                    {
                        Ok(evidence) => Ok(ToolExecution::new(
                            format!("Clicked element: {}", selector),
                            vec![Evidence::new(
                                "element_clicked",
                                serde_json::json!({"selector": selector, "url": evidence.url}),
                            )],
                        )),
                        Err(e) => Err(format!("Click failed: {}", e)),
                    }
                },
            )
            .await;

        finalize_execution_result(result, state)
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
        let contract = required_evidence_contract(&["text_typed"], "Type evidence required");

        let (result, state) = self
            .engine
            .execute(
                &intent,
                &format!("web_type_{}", step),
                step,
                Some(&contract),
                || async {
                    match backend
                        .execute(WebAction::Type {
                            selector: selector.clone(),
                            text: text.clone(),
                        })
                        .await
                    {
                        Ok(_evidence) => Ok(ToolExecution::new(
                            format!("Typed '{}' into {}", text, selector),
                            vec![Evidence::new(
                                "text_typed",
                                serde_json::json!({"selector": selector, "text": text}),
                            )],
                        )),
                        Err(e) => Err(format!("Type failed: {}", e)),
                    }
                },
            )
            .await;

        finalize_execution_result(result, state)
    }

    #[tool(description = "Press a keyboard key (Enter, Tab, Escape, ArrowDown, ArrowUp, Backspace, Space, etc.)")]
    async fn web_press_key(&self, Parameters(req): Parameters<PressKeyRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent = Intent::new("web.press_key");
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let key = req.key.clone();
        let contract =
            required_evidence_contract(&["key_pressed"], "Key press evidence required");

        let (result, _state) = self
            .engine
            .execute(
                &intent,
                &format!("web_press_key_{}", step),
                step,
                Some(&contract),
                || async {
                    match backend
                        .execute(WebAction::PressKey { key: key.clone() })
                        .await
                    {
                        Ok(_evidence) => Ok(ToolExecution::new(
                            format!("Pressed key: {}", key),
                            vec![Evidence::new(
                                "key_pressed",
                                serde_json::json!({"key": key}),
                            )],
                        )),
                        Err(e) => Err(format!("Key press failed: {}", e)),
                    }
                },
            )
            .await;

        result
    }

    #[tool(description = "Take a screenshot of the current page. Saves to a file and returns the file path.")]
    async fn web_screenshot(&self, Parameters(req): Parameters<ScreenshotRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let intent = Intent::new("web.screenshot");
        let step = self.next_step().await;
        let backend = self.backend.clone();
        let output_path = req.output_path.clone();
        let contract =
            required_evidence_contract(&["screenshot"], "Screenshot capture evidence required");

        let (result, state) = self
            .engine
            .execute(
                &intent,
                &format!("web_screenshot_{}", step),
                step,
                Some(&contract),
                || async {
                    match backend
                        .execute(WebAction::Screenshot {
                            output_path: output_path.clone(),
                        })
                        .await
                    {
                        Ok(evidence) => {
                            let path = evidence.screenshot.ok_or_else(|| {
                                "Screenshot failed: no file path returned".to_string()
                            })?;

                            Ok(ToolExecution::new(
                                format!("Screenshot saved to: {}", path),
                                vec![Evidence::new(
                                    "screenshot",
                                    serde_json::json!({
                                        "path": path,
                                        "url": evidence.url,
                                    }),
                                )],
                            ))
                        }
                        Err(e) => Err(format!("Screenshot failed: {}", e)),
                    }
                },
            )
            .await;

        finalize_execution_result(result, state)
    }

    #[tool(description = "Search the web using TinyFish AI (with Brave fallback). Returns structured search results without needing browser automation.")]
    async fn web_search(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        let count = req.count.unwrap_or(5);

        // Try TinyFish first, then Brave
        if let Ok(tinyfish_key) = std::env::var("TINYFISH_API_KEY") {
            match tinyfish_search(&tinyfish_key, &req.query, count).await {
                Ok(results) => return results,
                Err(e) => {
                    tracing::warn!("TinyFish search failed, falling back to Brave: {}", e);
                }
            }
        }

        if let Ok(brave_key) = std::env::var("BRAVE_API_KEY") {
            match brave_search(&brave_key, &req.query, count).await {
                Ok(results) => return results,
                Err(e) => return format!("Search failed (both TinyFish and Brave): {}", e),
            }
        }

        "Search unavailable: set TINYFISH_API_KEY or BRAVE_API_KEY environment variable".to_string()
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
        let contract =
            required_evidence_contract(&["text_content"], "Text extraction evidence required");

        let (result, state) = self
            .engine
            .execute(
                &intent,
                &format!("web_extract_{}", step),
                step,
                Some(&contract),
                || async {
                    match backend
                        .execute(WebAction::Extract {
                            selector: selector.clone(),
                        })
                        .await
                    {
                        Ok(evidence) => {
                            let text = evidence.text_content.unwrap_or_default();
                            let content = if text.len() > 10000 {
                                format!(
                                    "{}... [truncated, {} chars total]",
                                    &text[..10000],
                                    text.len()
                                )
                            } else {
                                text.clone()
                            };

                            Ok(ToolExecution::new(
                                content,
                                vec![Evidence::new(
                                    "text_content",
                                    serde_json::json!({"length": text.len(), "url": evidence.url}),
                                )],
                            ))
                        }
                        Err(e) => Err(format!("Extract failed: {}", e)),
                    }
                },
            )
            .await;

        finalize_execution_result(result, state)
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
        let contract =
            required_evidence_contract(&["wait_completed"], "Wait completion evidence required");

        let (result, state) = self
            .engine
            .execute(
                &intent,
                &format!("web_wait_{}", step),
                step,
                Some(&contract),
                || async {
                    let condition = if let Ok(ms) = condition_str.parse::<u64>() {
                        WaitCondition::Duration(ms)
                    } else {
                        WaitCondition::Selector(condition_str.clone())
                    };

                    match backend.execute(WebAction::Wait { condition }).await {
                        Ok(_) => Ok(ToolExecution::new(
                            format!("Wait completed for: {}", condition_str),
                            vec![Evidence::new(
                                "wait_completed",
                                serde_json::json!({"condition": condition_str}),
                            )],
                        )),
                        Err(e) => Err(format!("Wait failed: {}", e)),
                    }
                },
            )
            .await;

        finalize_execution_result(result, state)
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

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self.tool_router.list_all();
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_context =
            rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tool_context).await
    }
}

// --- Search helpers ---

async fn tinyfish_search(api_key: &str, query: &str, count: u32) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://agent.tinyfish.ai/v1/automation/run-sse")
        .header("X-API-Key", api_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "url": format!("https://www.google.com/search?q={}", urlencoding::encode(query)),
            "goal": format!("Extract the top {} search result titles, URLs, and descriptions", count),
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("TinyFish request failed: {}", e))?;

    let text = resp
        .text()
        .await
        .map_err(|e| format!("TinyFish response read failed: {}", e))?;

    Ok(text)
}

async fn brave_search(api_key: &str, query: &str, count: u32) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", &count.to_string())])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Brave search request failed: {}", e))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Brave response parse failed: {}", e))?;

    // Format results
    let mut output = String::new();
    if let Some(results) = data.get("web").and_then(|w| w.get("results")).and_then(|r| r.as_array())
    {
        for (i, result) in results.iter().enumerate() {
            let title = result.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let url = result.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let desc = result
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            output.push_str(&format!("{}. {}\n   {}\n   {}\n\n", i + 1, title, url, desc));
        }
    }

    if output.is_empty() {
        Err("No results found".to_string())
    } else {
        Ok(output)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("krust-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--check") {
        match detect_chrome_path() {
            Ok(path) => {
                println!("✅ Chrome detected: {}", path);
                return Ok(());
            }
            Err(err) => {
                eprintln!("❌ Chrome check failed: {}", err);
                std::process::exit(1);
            }
        }
    }

    // Initialize logging to stderr (MCP uses stdin/stdout for protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Krust MCP server v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!(
        "Headless: {}",
        std::env::var("KRUST_HEADLESS")
            .map(|v| v != "false")
            .unwrap_or(true)
    );

    match detect_chrome_path() {
        Ok(path) => tracing::info!("Detected Chrome executable: {}", path),
        Err(err) => tracing::error!(
            "Chrome/Chromium was not found at startup: {}. \
             Set CHROME_PATH to your Chrome/Chromium binary before first browser tool call.",
            err
        ),
    }

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
    use rmcp::{
        model::{ClientInfo, ErrorCode},
        ClientHandler, ServiceError,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone, Default)]
    struct TestClientHandler;

    impl ClientHandler for TestClientHandler {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
    }

    #[test]
    fn test_engine_policy_allows_web_navigate() {
        let engine = ExecutionEngine::new();
        let intent =
            Intent::new("web.navigate").with_param("url", serde_json::json!("https://example.com"));
        assert_eq!(engine.check_policy(&intent), PolicyDecision::Allow);
    }

    #[test]
    fn test_allow_all_does_not_short_circuit_confirm_policy() {
        let engine = ExecutionEngine::with_policies(vec![
            Box::new(AllowAllPolicy),
            Box::new(ConfirmPatternPolicy {
                confirm_prefixes: vec!["payment.".to_string()],
                deny_prefixes: vec![],
            }),
        ]);

        let intent = Intent::new("payment.submit");
        match engine.check_policy(&intent) {
            PolicyDecision::Confirm { .. } => {}
            other => panic!("Expected Confirm, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_engine_execute_success_lifecycle() {
        let engine = ExecutionEngine::new();
        let intent = Intent::new("web.navigate");
        let contract = required_evidence_contract(&["page_loaded"], "Page load evidence required");

        let (result, state) = engine
            .execute(&intent, "tc_test", 1, Some(&contract), || async {
                Ok(ToolExecution::new(
                    "Navigated to example.com",
                    vec![Evidence::new(
                        "page_loaded",
                        serde_json::json!({"url": "https://example.com"}),
                    )],
                ))
            })
            .await;

        assert_eq!(result, "Navigated to example.com");
        match state {
            AgentState::Completed { artifacts } => {
                assert_eq!(artifacts, vec!["page_loaded"]);
            }
            _ => panic!("Expected Completed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_execute_retries_until_success() {
        let engine = ExecutionEngine::new();
        let intent = Intent::new("web.click");
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let (result, state) = engine
            .execute(&intent, "tc_retry", 1, None, move || {
                let attempts = attempts_for_closure.clone();
                async move {
                    let current = attempts.fetch_add(1, Ordering::SeqCst);
                    if current < 2 {
                        Err(format!("transient failure {}", current + 1))
                    } else {
                        Ok(ToolExecution::new("eventual success", vec![]))
                    }
                }
            })
            .await;

        assert_eq!(result, "eventual success");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        match state {
            AgentState::Completed { .. } => {}
            _ => panic!("Expected Completed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_verification_insufficient_retries_and_fails() {
        let engine = ExecutionEngine::new();
        let intent = Intent::new("web.extract");
        let contract = required_evidence_contract(&["text_content"], "Text evidence required");
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let (result, state) = engine
            .execute(&intent, "tc_verify", 1, Some(&contract), move || {
                let attempts = attempts_for_closure.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Ok(ToolExecution::new("extracted", vec![]))
                }
            })
            .await;

        assert_eq!(result, "extracted");
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
        match state {
            AgentState::Failed { reason } => {
                assert!(reason.contains("Verification failed after"));
            }
            _ => panic!("Expected Failed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_policy_deny_blocks_execution() {
        let engine = ExecutionEngine::with_policies(vec![Box::new(ConfirmPatternPolicy {
            confirm_prefixes: vec![],
            deny_prefixes: vec!["forbidden.".to_string()],
        })]);
        let intent = Intent::new("forbidden.action");

        let (result, state) = engine
            .execute(&intent, "tc_test", 1, None, || async {
                Ok(ToolExecution::new("should not reach", vec![]))
            })
            .await;

        assert!(result.starts_with("Policy denied:"));
        match state {
            AgentState::Failed { .. } => {}
            _ => panic!("Expected Failed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_policy_confirm_blocks_execution_without_human_channel() {
        let engine = ExecutionEngine::with_policies(vec![Box::new(ConfirmPatternPolicy {
            confirm_prefixes: vec!["payment.".to_string()],
            deny_prefixes: vec![],
        })]);
        let intent = Intent::new("payment.charge");
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let (result, state) = engine
            .execute(&intent, "tc_confirm", 1, None, move || {
                let attempts = attempts_for_closure.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Ok(ToolExecution::new("should never run", vec![]))
                }
            })
            .await;

        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert!(result.contains("requires human confirmation"));
        match state {
            AgentState::Failed { reason } => {
                assert!(reason.contains("requires human confirmation"));
            }
            _ => panic!("Expected Failed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_retry_budget_guard_stops_execution_loop() {
        let engine = ExecutionEngine::with_limits(
            vec![Box::new(AllowAllPolicy)],
            1,
            Duration::from_secs(60),
        );
        let intent = Intent::new("web.extract");
        let contract = required_evidence_contract(&["text_content"], "Text evidence required");
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let (_result, state) = engine
            .execute(&intent, "tc_budget", 1, Some(&contract), move || {
                let attempts = attempts_for_closure.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Ok(ToolExecution::new("extracted", vec![]))
                }
            })
            .await;

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        match state {
            AgentState::Failed { reason } => {
                assert!(reason.contains("retry safety budget exceeded"));
            }
            _ => panic!("Expected Failed, got {:?}", state),
        }
    }

    #[tokio::test]
    async fn test_engine_wall_clock_budget_preempts_hung_tool_attempt() {
        let engine = ExecutionEngine::with_limits(
            vec![Box::new(AllowAllPolicy)],
            16,
            Duration::from_millis(50),
        );
        let intent = Intent::new("web.wait");

        let (result, state) = tokio::time::timeout(
            Duration::from_secs(1),
            engine.execute(&intent, "tc_hung", 1, None, || async {
                std::future::pending::<Result<ToolExecution, String>>().await
            }),
        )
        .await
        .expect("hung tool attempt should be preempted by wall-clock timeout");

        assert!(result.contains("tool attempt timed out"));
        match state {
            AgentState::Failed { reason } => {
                assert!(reason.contains("tool attempt timed out"));
            }
            _ => panic!("Expected Failed, got {:?}", state),
        }
    }

    #[test]
    fn test_finalize_execution_result_surfaces_failed_state_details() {
        let rendered = finalize_execution_result(
            "backend failure payload".to_string(),
            AgentState::Failed {
                reason: "verification failed".to_string(),
            },
        );

        assert!(rendered.contains("verification failed"));
        assert!(rendered.contains("backend failure payload"));
    }

    #[tokio::test]
    async fn test_mcp_list_tools_exposes_expected_tools() {
        let (server_transport, client_transport) = tokio::io::duplex(4096);

        let server = KrustServer::new();
        let server_handle = tokio::spawn(async move {
            let running = server
                .serve(server_transport)
                .await
                .expect("server should start");
            running.waiting().await.expect("server should stop cleanly");
        });

        let client = TestClientHandler::default()
            .serve(client_transport)
            .await
            .expect("client should connect");

        let list = client
            .list_tools(Default::default())
            .await
            .expect("list_tools should succeed");

        let mut tool_names: Vec<String> = list
            .tools
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        tool_names.sort();

        assert_eq!(
            tool_names,
            vec![
                "web_click".to_string(),
                "web_extract".to_string(),
                "web_navigate".to_string(),
                "web_screenshot".to_string(),
                "web_type".to_string(),
                "web_wait".to_string(),
            ]
        );

        client.cancel().await.expect("client cancel should succeed");
        server_handle
            .await
            .expect("server task should complete without panic");
    }

    #[tokio::test]
    async fn test_mcp_call_tool_dispatches_and_validates_parameters() {
        let (server_transport, client_transport) = tokio::io::duplex(4096);

        let server = KrustServer::new();
        let server_handle = tokio::spawn(async move {
            let running = server
                .serve(server_transport)
                .await
                .expect("server should start");
            running.waiting().await.expect("server should stop cleanly");
        });

        let client = TestClientHandler::default()
            .serve(client_transport)
            .await
            .expect("client should connect");

        let err = client
            .call_tool(CallToolRequestParam {
                name: "web_navigate".into(),
                arguments: Some(
                    serde_json::json!({})
                        .as_object()
                        .expect("json object")
                        .clone(),
                ),
            })
            .await
            .expect_err("missing url should fail");

        match err {
            ServiceError::McpError(error) => {
                assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
                assert!(error.message.contains("url"));
            }
            other => panic!("Expected MCP error, got {:?}", other),
        }

        client.cancel().await.expect("client cancel should succeed");
        server_handle
            .await
            .expect("server task should complete without panic");
    }

    #[tokio::test]
    async fn test_mcp_unknown_tool_returns_expected_error_shape() {
        let (server_transport, client_transport) = tokio::io::duplex(4096);

        let server = KrustServer::new();
        let server_handle = tokio::spawn(async move {
            let running = server
                .serve(server_transport)
                .await
                .expect("server should start");
            running.waiting().await.expect("server should stop cleanly");
        });

        let client = TestClientHandler::default()
            .serve(client_transport)
            .await
            .expect("client should connect");

        let err = client
            .call_tool(CallToolRequestParam {
                name: "unknown_tool".into(),
                arguments: None,
            })
            .await
            .expect_err("unknown tool should fail");

        match err {
            ServiceError::McpError(error) => {
                assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
                assert!(error
                    .message
                    .to_ascii_lowercase()
                    .contains("tool not found"));
                assert!(error.data.is_none());
            }
            other => panic!("Expected MCP error, got {:?}", other),
        }

        client.cancel().await.expect("client cancel should succeed");
        server_handle
            .await
            .expect("server task should complete without panic");
    }
}
