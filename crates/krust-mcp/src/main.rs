//! # krust-mcp
//!
//! MCP server that exposes Krust's verified execution tools to any
//! MCP-compatible agent (Claude Code, Codex, Cursor, etc.).
//!
//! Usage:
//!   cargo run --bin krust-mcp
//!
//! Then configure your agent's MCP settings to point at this binary.

use std::sync::Arc;
use tokio::sync::Mutex;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_router,
};
use krust_agent_web::cdp::CdpBackend;
use krust_agent_web::backend::WebBackend;
use krust_agent_web::action::{WebAction, WaitCondition};

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
}

#[tool_router]
impl KrustServer {
    #[tool(description = "Navigate the browser to a URL. Returns page title and URL.")]
    async fn web_navigate(&self, Parameters(req): Parameters<NavigateRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        match self.backend.execute(WebAction::Navigate { url: req.url.clone() }).await {
            Ok(evidence) => format!(
                "Navigated to {}. Title: {}",
                evidence.url.as_deref().unwrap_or(&req.url),
                evidence.text_content.as_deref().unwrap_or("(none)")
            ),
            Err(e) => format!("Navigation failed: {}", e),
        }
    }

    #[tool(description = "Click an element on the page by CSS selector.")]
    async fn web_click(&self, Parameters(req): Parameters<ClickRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        match self.backend.execute(WebAction::Click { selector: req.selector.clone() }).await {
            Ok(_) => format!("Clicked element: {}", req.selector),
            Err(e) => format!("Click failed: {}", e),
        }
    }

    #[tool(description = "Type text into an input element by CSS selector.")]
    async fn web_type(&self, Parameters(req): Parameters<TypeRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        match self.backend.execute(WebAction::Type {
            selector: req.selector.clone(),
            text: req.text.clone(),
        }).await {
            Ok(_) => format!("Typed '{}' into {}", req.text, req.selector),
            Err(e) => format!("Type failed: {}", e),
        }
    }

    #[tool(description = "Extract text content from the page or a specific element.")]
    async fn web_extract(&self, Parameters(req): Parameters<ExtractRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        match self.backend.execute(WebAction::Extract { selector: req.selector }).await {
            Ok(evidence) => {
                let text = evidence.text_content.unwrap_or_default();
                if text.len() > 10000 {
                    format!("{}... [truncated, {} chars total]", &text[..10000], text.len())
                } else {
                    text
                }
            }
            Err(e) => format!("Extract failed: {}", e),
        }
    }

    #[tool(description = "Wait for an element to appear (CSS selector) or a fixed duration (milliseconds).")]
    async fn web_wait(&self, Parameters(req): Parameters<WaitRequest>) -> String {
        if let Err(e) = self.ensure_browser().await {
            return format!("Error: Browser launch failed: {}", e);
        }

        let condition = if let Ok(ms) = req.condition.parse::<u64>() {
            WaitCondition::Duration(ms)
        } else {
            WaitCondition::Selector(req.condition.clone())
        };

        match self.backend.execute(WebAction::Wait { condition }).await {
            Ok(_) => format!("Wait completed for: {}", req.condition),
            Err(e) => format!("Wait failed: {}", e),
        }
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
