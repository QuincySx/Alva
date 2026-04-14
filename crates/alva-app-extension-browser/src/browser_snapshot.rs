// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserSnapshotTool
// POS:    Extracts page content in text, HTML, or readability (article-extraction) mode.
//! browser_snapshot — extract page content (text, HTML, readability)

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::browser_manager::SharedBrowserManager;

/// Content extraction mode.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SnapshotMode {
    /// Visible text only.
    Text,
    /// Raw HTML.
    Html,
    /// Article-like clean text (strips nav/ads/sidebars).
    Readability,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Extraction mode. Default: 'text'.
    #[serde(default)]
    mode: Option<SnapshotMode>,
    /// CSS selector to scope extraction to a specific element (e.g. 'article', '#content', '.main').
    #[serde(default)]
    selector: Option<String>,
    /// Browser instance ID. Default: 'default'.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_snapshot",
    description = "Extract content from the current page. Modes: 'text' (visible text, default), 'html' (raw HTML), 'readability' (article extraction — strips nav/ads/sidebars, returns clean text like Reader Mode).",
    input = Input,
    read_only,
)]
pub struct BrowserSnapshotTool {
    pub manager: SharedBrowserManager,
}

impl BrowserSnapshotTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let id = params.id.unwrap_or_else(|| "default".to_string());
        let (mode_str, mode) = match params.mode {
            Some(SnapshotMode::Html) => ("html", SnapshotMode::Html),
            Some(SnapshotMode::Readability) => ("readability", SnapshotMode::Readability),
            Some(SnapshotMode::Text) | None => ("text", SnapshotMode::Text),
        };

        let manager = self.manager.lock().await;
        let page = manager
            .active_page(&id)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "browser_snapshot".into(), message: e })?;

        let result = match mode {
            SnapshotMode::Text => extract_text(&page, params.selector.as_deref()).await,
            SnapshotMode::Html => extract_html(&page, params.selector.as_deref()).await,
            SnapshotMode::Readability => extract_readability(&page).await,
        };

        match result {
            Ok(content) => {
                // Get current URL and title for context
                let url = page
                    .url()
                    .await
                    .ok()
                    .flatten()
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                let title = page.get_title().await.ok().flatten().unwrap_or_default();

                Ok(ToolOutput::text(json!({
                    "url": url,
                    "title": title,
                    "mode": mode_str,
                    "content": content,
                    "length": content.len(),
                })
                .to_string()))
            }
            Err(e) => Ok(ToolOutput::error(json!({ "error": e }).to_string())),
        }
    }
}

/// Extract visible text from the page or a specific element
async fn extract_text(
    page: &chromiumoxide::page::Page,
    selector: Option<&str>,
) -> Result<String, String> {
    let js = if let Some(sel) = selector {
        format!(
            r#"(() => {{
                const el = document.querySelector('{}');
                return el ? el.innerText : 'Element not found: {}';
            }})()"#,
            sel, sel
        )
    } else {
        "document.body.innerText".to_string()
    };

    let result: String = page
        .evaluate(js)
        .await
        .map_err(|e| format!("Text extraction failed: {}", e))?
        .into_value()
        .map_err(|e| format!("Failed to parse text result: {}", e))?;

    Ok(result)
}

/// Extract raw HTML from the page or a specific element
async fn extract_html(
    page: &chromiumoxide::page::Page,
    selector: Option<&str>,
) -> Result<String, String> {
    let js = if let Some(sel) = selector {
        format!(
            r#"(() => {{
                const el = document.querySelector('{}');
                return el ? el.outerHTML : 'Element not found: {}';
            }})()"#,
            sel, sel
        )
    } else {
        "document.documentElement.outerHTML".to_string()
    };

    let result: String = page
        .evaluate(js)
        .await
        .map_err(|e| format!("HTML extraction failed: {}", e))?
        .into_value()
        .map_err(|e| format!("Failed to parse HTML result: {}", e))?;

    Ok(result)
}

/// Readability-style extraction — strips navigation, ads, sidebars, returns clean article text.
/// This is a simplified Readability implementation that runs in the browser.
async fn extract_readability(page: &chromiumoxide::page::Page) -> Result<String, String> {
    // Inject a minimal readability-like extraction script.
    // This is inspired by Mozilla's Readability.js but simplified for CDP injection.
    let js = r#"
    (() => {
        // Remove obviously non-content elements
        const removeTags = ['script', 'style', 'nav', 'footer', 'header', 'aside', 'noscript', 'iframe'];
        const clone = document.cloneNode(true);

        removeTags.forEach(tag => {
            clone.querySelectorAll(tag).forEach(el => el.remove());
        });

        // Remove elements with common non-content class/id patterns
        const noisePatterns = /sidebar|menu|footer|header|nav|banner|ad|popup|modal|cookie|social|share|comment/i;
        clone.querySelectorAll('[class], [id]').forEach(el => {
            const cls = el.className || '';
            const id = el.id || '';
            if (noisePatterns.test(cls) || noisePatterns.test(id)) {
                el.remove();
            }
        });

        // Try to find the main content area
        const candidates = ['article', 'main', '[role="main"]', '.post-content', '.article-content', '.entry-content', '#content', '.content'];
        for (const sel of candidates) {
            const el = clone.querySelector(sel);
            if (el && el.innerText.trim().length > 200) {
                return el.innerText.trim();
            }
        }

        // Fallback: return body text after cleanup
        return clone.body ? clone.body.innerText.trim() : '';
    })()
    "#;

    let result: String = page
        .evaluate(js)
        .await
        .map_err(|e| format!("Readability extraction failed: {}", e))?
        .into_value()
        .map_err(|e| format!("Failed to parse readability result: {}", e))?;

    Ok(result)
}
