//! CDP (Chrome DevTools Protocol) backend using chromiumoxide.
//!
//! This backend controls a real Chromium/Chrome browser for desktop
//! and Linux environments.

use crate::action::{WaitCondition, WebAction};
use crate::backend::{WebBackend, WebError};
use crate::evidence::WebEvidence;
use crate::page::PageSnapshot;
use base64::Engine;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;

/// CDP backend wrapping a chromiumoxide browser instance.
pub struct CdpBackend {
    page: Arc<Mutex<Option<Page>>>,
    browser: Arc<Mutex<Option<Browser>>>,
}

impl Default for CdpBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CdpBackend {
    /// Create a new CDP backend. Call `launch()` to start the browser.
    pub fn new() -> Self {
        Self {
            page: Arc::new(Mutex::new(None)),
            browser: Arc::new(Mutex::new(None)),
        }
    }

    /// Launch a headless Chrome instance.
    pub async fn launch(&self) -> Result<(), WebError> {
        let config = BrowserConfig::builder()
            .no_sandbox()
            .window_size(1280, 720)
            .build()
            .map_err(|e| WebError::Other(format!("Browser config error: {}", e)))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| WebError::Other(format!("Browser launch error: {}", e)))?;

        // Spawn the browser event handler
        tokio::spawn(async move { while let Some(_event) = handler.next().await {} });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| WebError::Other(format!("New page error: {}", e)))?;

        *self.browser.lock().await = Some(browser);
        *self.page.lock().await = Some(page);

        Ok(())
    }

    /// Get a reference to the current page, or error if not launched.
    async fn page(&self) -> Result<Page, WebError> {
        self.page.lock().await.clone().ok_or(WebError::NotConnected)
    }
}

#[async_trait::async_trait]
impl WebBackend for CdpBackend {
    async fn execute(&self, action: WebAction) -> Result<WebEvidence, WebError> {
        let page = self.page().await?;

        match action {
            WebAction::Navigate { url } => {
                page.goto(&url)
                    .await
                    .map_err(|e| WebError::NavigationFailed(e.to_string()))?;

                let title = page
                    .get_title()
                    .await
                    .unwrap_or_default()
                    .unwrap_or_default();
                let current_url = page
                    .url()
                    .await
                    .ok()
                    .flatten()
                    .map(|u| u.to_string())
                    .unwrap_or_default();

                Ok(WebEvidence {
                    action_summary: format!("Navigated to {}", url),
                    url: Some(current_url),
                    screenshot: None,
                    text_content: Some(title),
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Click { selector } => {
                page.find_element(&selector)
                    .await
                    .map_err(|_| WebError::ElementNotFound {
                        selector: selector.clone(),
                    })?
                    .click()
                    .await
                    .map_err(|e| WebError::Other(format!("Click failed: {}", e)))?;

                Ok(WebEvidence {
                    action_summary: format!("Clicked element: {}", selector),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: None,
                    text_content: None,
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Type { selector, text } => {
                page.find_element(&selector)
                    .await
                    .map_err(|_| WebError::ElementNotFound {
                        selector: selector.clone(),
                    })?
                    .click()
                    .await
                    .map_err(|e| WebError::Other(format!("Focus failed: {}", e)))?;

                page.find_element(&selector)
                    .await
                    .map_err(|_| WebError::ElementNotFound {
                        selector: selector.clone(),
                    })?
                    .type_str(&text)
                    .await
                    .map_err(|e| WebError::Other(format!("Type failed: {}", e)))?;

                Ok(WebEvidence {
                    action_summary: format!("Typed '{}' into {}", text, selector),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: None,
                    text_content: None,
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Screenshot => {
                let bytes = page
                    .screenshot(
                        chromiumoxide::page::ScreenshotParams::builder()
                            .full_page(false)
                            .build(),
                    )
                    .await
                    .map_err(|e| WebError::Other(format!("Screenshot failed: {}", e)))?;

                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

                Ok(WebEvidence {
                    action_summary: "Captured screenshot".to_string(),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: Some(b64),
                    text_content: None,
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Extract { selector } => {
                let text = if let Some(sel) = selector {
                    let el =
                        page.find_element(&sel)
                            .await
                            .map_err(|_| WebError::ElementNotFound {
                                selector: sel.clone(),
                            })?;
                    el.inner_text().await.ok().flatten().unwrap_or_default()
                } else {
                    // Extract full page text via JS
                    page.evaluate("document.body.innerText")
                        .await
                        .ok()
                        .and_then(|v| v.into_value::<String>().ok())
                        .unwrap_or_default()
                };

                Ok(WebEvidence {
                    action_summary: "Extracted text content".to_string(),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: None,
                    text_content: Some(text),
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Wait { condition } => {
                match condition {
                    WaitCondition::Selector(sel) => {
                        const POLL_INTERVAL_MS: u64 = 200;
                        const TIMEOUT_MS: u64 = 30_000;
                        let deadline = tokio::time::Instant::now()
                            + tokio::time::Duration::from_millis(TIMEOUT_MS);

                        loop {
                            if page.find_element(&sel).await.is_ok() {
                                break;
                            }
                            if tokio::time::Instant::now() >= deadline {
                                return Err(WebError::Timeout { ms: TIMEOUT_MS });
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(
                                POLL_INTERVAL_MS,
                            ))
                            .await;
                        }
                    }
                    WaitCondition::Navigation => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }
                    WaitCondition::Duration(ms) => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
                    }
                }

                Ok(WebEvidence {
                    action_summary: "Wait completed".to_string(),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: None,
                    text_content: None,
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Back => {
                page.evaluate("history.back()").await.ok();
                Ok(WebEvidence {
                    action_summary: "Navigated back".to_string(),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: None,
                    text_content: None,
                    browser_success: true,
                    http_status: None,
                })
            }

            WebAction::Forward => {
                page.evaluate("history.forward()").await.ok();
                Ok(WebEvidence {
                    action_summary: "Navigated forward".to_string(),
                    url: page.url().await.ok().flatten().map(|u| u.to_string()),
                    screenshot: None,
                    text_content: None,
                    browser_success: true,
                    http_status: None,
                })
            }
        }
    }

    async fn snapshot(&self) -> Result<PageSnapshot, WebError> {
        let page = self.page().await?;

        let url = page
            .url()
            .await
            .ok()
            .flatten()
            .map(|u| u.to_string())
            .unwrap_or_default();
        let title = page
            .get_title()
            .await
            .unwrap_or_default()
            .unwrap_or_default();

        // Basic page snapshot — will be enriched with DOM parsing later
        Ok(PageSnapshot {
            url,
            title,
            elements: Vec::new(), // TODO: parse interactive elements from DOM
        })
    }

    async fn is_ready(&self) -> bool {
        self.page.lock().await.is_some()
    }
}
