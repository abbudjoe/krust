//! Evidence types for web interactions.

use serde::{Deserialize, Serialize};

/// Evidence produced by a web action execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebEvidence {
    /// What action was performed.
    pub action_summary: String,

    /// The resulting page URL after the action.
    pub url: Option<String>,

    /// Screenshot (base64-encoded PNG) if captured.
    pub screenshot: Option<String>,

    /// Text content extracted from the page or element.
    pub text_content: Option<String>,

    /// Whether the action appeared to succeed from the browser's perspective.
    pub browser_success: bool,

    /// HTTP status code if a navigation occurred.
    pub http_status: Option<u16>,
}
