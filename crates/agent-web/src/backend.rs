//! Backend trait for web interaction.
//!
//! Each platform implements this trait to provide browser control.

use crate::action::WebAction;
use crate::evidence::WebEvidence;
use crate::page::PageSnapshot;

/// Trait that browser backends implement.
///
/// The agent-web crate is backend-agnostic. Implementors provide
/// the actual browser control for their platform:
/// - CDP (chromiumoxide) for desktop/Linux
/// - Accessibility service callbacks for Android/iOS
/// - Native browser APIs for future platforms
#[async_trait::async_trait]
pub trait WebBackend: Send + Sync {
    /// Execute a web action and return evidence of what happened.
    async fn execute(&self, action: WebAction) -> Result<WebEvidence, WebError>;

    /// Get a snapshot of the current page state.
    async fn snapshot(&self) -> Result<PageSnapshot, WebError>;

    /// Check if the backend is connected and ready.
    async fn is_ready(&self) -> bool;
}

/// Errors from web backend operations.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("Navigation failed: {0}")]
    NavigationFailed(String),

    #[error("Element not found: {selector}")]
    ElementNotFound { selector: String },

    #[error("Action timed out after {ms}ms")]
    Timeout { ms: u64 },

    #[error("Backend not connected")]
    NotConnected,

    #[error("Backend error: {0}")]
    Other(String),
}
