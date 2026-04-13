// INPUT:  alva_types, async_trait, chromiumoxide::cdp, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserActionTool
// POS:    Performs page interactions (click/type/press/scroll) via CSS selectors or coordinates.
//! browser_action — page interaction: click, type, press, scroll

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::browser_manager::SharedBrowserManager;

/// The kind of page interaction to perform.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ActionKind {
    Click,
    Type,
    Press,
    Scroll,
}

/// Scroll direction.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The action to perform.
    action: ActionKind,
    /// CSS selector to target (for click/type).
    #[serde(default)]
    selector: Option<String>,
    /// Text to type (required for 'type' action).
    #[serde(default)]
    text: Option<String>,
    /// Key to press (for 'press' action): Enter, Tab, Escape, ArrowDown, Backspace, etc.
    #[serde(default)]
    key: Option<String>,
    /// Scroll direction (for 'scroll' action). Default: 'down'.
    #[serde(default)]
    direction: Option<ScrollDirection>,
    /// Scroll amount in pixels. Default: 300.
    #[serde(default)]
    amount: Option<i64>,
    /// X coordinate for click (alternative to selector).
    #[serde(default)]
    x: Option<f64>,
    /// Y coordinate for click (alternative to selector).
    #[serde(default)]
    y: Option<f64>,
    /// Browser instance ID. Default: 'default'.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_action",
    description = "Perform an interaction on the current page. Supports: click (by selector or coordinates), type (text into an element), press (keyboard key), scroll (up/down).",
    input = Input,
)]
pub struct BrowserActionTool {
    pub manager: SharedBrowserManager,
}

impl BrowserActionTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let id = params.id.clone().unwrap_or_else(|| "default".to_string());

        let action_label = match params.action {
            ActionKind::Click => "click",
            ActionKind::Type => "type",
            ActionKind::Press => "press",
            ActionKind::Scroll => "scroll",
        };

        let manager = self.manager.lock().await;
        let page = manager
            .active_page(&id)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "browser_action".into(), message: e })?;

        let result = match params.action {
            ActionKind::Click => execute_click(&page, &params).await,
            ActionKind::Type => execute_type(&page, &params).await,
            ActionKind::Press => execute_press(&page, &params).await,
            ActionKind::Scroll => execute_scroll(&page, &params).await,
        };

        match result {
            Ok(msg) => Ok(ToolOutput::text(json!({
                "status": "ok",
                "action": action_label,
                "detail": msg,
            })
            .to_string())),
            Err(e) => Ok(ToolOutput::error(json!({ "error": e }).to_string())),
        }
    }
}

async fn execute_click(
    page: &chromiumoxide::page::Page,
    params: &Input,
) -> Result<String, String> {
    if let Some(ref selector) = params.selector {
        // Click by CSS selector
        let element = page
            .find_element(selector)
            .await
            .map_err(|e| format!("Element '{}' not found: {}", selector, e))?;

        element
            .click()
            .await
            .map_err(|e| format!("Click failed on '{}': {}", selector, e))?;

        Ok(format!("Clicked element '{}'", selector))
    } else if let (Some(x), Some(y)) = (params.x, params.y) {
        // Click by coordinates using CDP mouse event
        page.execute(
            DispatchMouseEventParams::builder()
                .x(x)
                .y(y)
                .r#type(DispatchMouseEventType::MousePressed)
                .button(chromiumoxide::cdp::browser_protocol::input::MouseButton::Left)
                .click_count(1)
                .build()
                .map_err(|e| format!("Failed to build mouse press event: {}", e))?,
        )
        .await
        .map_err(|e| format!("Mouse press failed: {}", e))?;

        page.execute(
            DispatchMouseEventParams::builder()
                .x(x)
                .y(y)
                .r#type(DispatchMouseEventType::MouseReleased)
                .button(chromiumoxide::cdp::browser_protocol::input::MouseButton::Left)
                .click_count(1)
                .build()
                .map_err(|e| format!("Failed to build mouse release event: {}", e))?,
        )
        .await
        .map_err(|e| format!("Mouse release failed: {}", e))?;

        Ok(format!("Clicked at coordinates ({}, {})", x, y))
    } else {
        Err("Click requires either 'selector' or both 'x' and 'y' coordinates".to_string())
    }
}

async fn execute_type(
    page: &chromiumoxide::page::Page,
    params: &Input,
) -> Result<String, String> {
    let text = params
        .text
        .as_deref()
        .ok_or_else(|| "'text' is required for type action".to_string())?;

    if let Some(ref selector) = params.selector {
        let element = page
            .find_element(selector)
            .await
            .map_err(|e| format!("Element '{}' not found: {}", selector, e))?;

        element
            .click()
            .await
            .map_err(|e| format!("Focus click on '{}' failed: {}", selector, e))?;

        element
            .type_str(text)
            .await
            .map_err(|e| format!("Type failed on '{}': {}", selector, e))?;

        Ok(format!("Typed '{}' into '{}'", text, selector))
    } else {
        // Type without a specific selector — sends keys to the focused element
        for ch in text.chars() {
            page.execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyDown)
                    .text(ch.to_string())
                    .build()
                    .map_err(|e| format!("Failed to build key event: {}", e))?,
            )
            .await
            .map_err(|e| format!("Key down failed: {}", e))?;

            page.execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyUp)
                    .text(ch.to_string())
                    .build()
                    .map_err(|e| format!("Failed to build key up event: {}", e))?,
            )
            .await
            .map_err(|e| format!("Key up failed: {}", e))?;
        }
        Ok(format!("Typed '{}' into focused element", text))
    }
}

async fn execute_press(
    page: &chromiumoxide::page::Page,
    params: &Input,
) -> Result<String, String> {
    let key = params
        .key
        .as_deref()
        .ok_or_else(|| "'key' is required for press action".to_string())?;

    page.execute(
        DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::KeyDown)
            .key(key)
            .build()
            .map_err(|e| format!("Failed to build key down event: {}", e))?,
    )
    .await
    .map_err(|e| format!("Key press (down) failed: {}", e))?;

    page.execute(
        DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::KeyUp)
            .key(key)
            .build()
            .map_err(|e| format!("Failed to build key up event: {}", e))?,
    )
    .await
    .map_err(|e| format!("Key press (up) failed: {}", e))?;

    Ok(format!("Pressed key '{}'", key))
}

async fn execute_scroll(
    page: &chromiumoxide::page::Page,
    params: &Input,
) -> Result<String, String> {
    let direction = params.direction.as_ref().unwrap_or(&ScrollDirection::Down);
    let direction_str = match direction {
        ScrollDirection::Down => "down",
        ScrollDirection::Up => "up",
    };
    let amount = params.amount.unwrap_or(300);

    let delta_y = match direction {
        ScrollDirection::Down => amount,
        ScrollDirection::Up => -amount,
    };

    let js = format!(
        "window.scrollBy({{ top: {}, behavior: 'smooth' }})",
        delta_y
    );

    page.evaluate(js)
        .await
        .map_err(|e| format!("Scroll failed: {}", e))?;

    Ok(format!("Scrolled {} by {} pixels", direction_str, amount))
}
