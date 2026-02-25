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

    /// Launch a Chrome instance.
    ///
    /// Respects environment variables:
    /// - `CHROME_PATH`: explicit path to Chrome/Chromium binary
    /// - `KRUST_HEADLESS`: set to "false" to show browser window (default: true)
    /// - `KRUST_WINDOW_WIDTH`: browser width (default: 1280)
    /// - `KRUST_WINDOW_HEIGHT`: browser height (default: 720)
    pub async fn launch(&self) -> Result<(), WebError> {
        let headless = std::env::var("KRUST_HEADLESS")
            .map(|v| v != "false")
            .unwrap_or(true);
        let width: u32 = std::env::var("KRUST_WINDOW_WIDTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1280);
        let height: u32 = std::env::var("KRUST_WINDOW_HEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(720);

        let mut builder = BrowserConfig::builder()
            .no_sandbox()
            .window_size(width, height);

        // Set Chrome path from env if provided
        if let Ok(chrome_path) = std::env::var("CHROME_PATH") {
            tracing::info!("Using Chrome from CHROME_PATH: {}", chrome_path);
            builder = builder.chrome_executable(chrome_path);
        } else {
            // Try to find Chrome and report helpful error if not found
            let candidates = if cfg!(target_os = "macos") {
                vec![
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                    "/Applications/Chromium.app/Contents/MacOS/Chromium",
                ]
            } else {
                vec![
                    "google-chrome",
                    "google-chrome-stable",
                    "chromium-browser",
                    "chromium",
                    "/usr/bin/google-chrome",
                    "/usr/bin/chromium-browser",
                    "/usr/bin/chromium",
                ]
            };

            let found = candidates
                .iter()
                .find(|c| std::path::Path::new(c).exists() || which::which(c).is_ok());

            match found {
                Some(path) => {
                    tracing::info!("Auto-detected Chrome at: {}", path);
                    builder = builder.chrome_executable(*path);
                }
                None => {
                    let msg = format!(
                        "Chrome/Chromium not found. Searched: {:?}\n\
                         Set the CHROME_PATH environment variable to your Chrome binary.\n\
                         Example: CHROME_PATH=\"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome\"",
                        candidates
                    );
                    tracing::error!("{}", msg);
                    return Err(WebError::Other(msg));
                }
            }
        }

        if !headless {
            builder = builder.with_head();
        }

        let config = builder
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
                        const INITIAL_POLL_MS: u64 = 200;
                        const MAX_POLL_MS: u64 = 2_000;
                        const TIMEOUT_MS: u64 = 30_000;
                        let deadline = tokio::time::Instant::now()
                            + tokio::time::Duration::from_millis(TIMEOUT_MS);
                        let mut poll_ms = INITIAL_POLL_MS;

                        loop {
                            if page.find_element(&sel).await.is_ok() {
                                break;
                            }
                            if tokio::time::Instant::now() >= deadline {
                                return Err(WebError::Timeout { ms: TIMEOUT_MS });
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(poll_ms)).await;
                            poll_ms = (poll_ms * 2).min(MAX_POLL_MS);
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
                page.evaluate("history.back()")
                    .await
                    .map_err(|e| WebError::Other(format!("Back navigation failed: {}", e)))?;
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
                page.evaluate("history.forward()")
                    .await
                    .map_err(|e| WebError::Other(format!("Forward navigation failed: {}", e)))?;
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
