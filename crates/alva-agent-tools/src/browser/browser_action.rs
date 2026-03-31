// INPUT:  alva_types, async_trait, chromiumoxide::cdp, serde, serde_json, super::browser_manager
// OUTPUT: BrowserActionTool
// POS:    Performs page interactions (click/type/press/scroll) via CSS selectors or coordinates.
//! browser_action — page interaction: click, type, press, scroll

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize)]
struct Input {
    /// Action type: "click", "type", "press", "scroll"
    action: String,
    /// CSS selector for click/type actions
    selector: Option<String>,
    /// Text to type (for "type" action)
    text: Option<String>,
    /// Key to press (for "press" action), e.g. "Enter", "Tab", "Escape"
    key: Option<String>,
    /// Scroll direction: "up" or "down" (for "scroll" action)
    direction: Option<String>,
    /// Scroll amount in pixels, default 300
    amount: Option<i64>,
    /// Coordinate X for click (alternative to selector)
    x: Option<f64>,
    /// Coordinate Y for click (alternative to selector)
    y: Option<f64>,
    /// Browser instance ID, default "default"
    id: Option<String>,
}

pub struct BrowserActionTool {
    pub manager: SharedBrowserManager,
}

#[async_trait]
impl Tool for BrowserActionTool {
    fn name(&self) -> &str {
        "browser_action"
    }

    fn description(&self) -> &str {
        "Perform an interaction on the current page. Supports: click (by selector or coordinates), type (text into an element), press (keyboard key), scroll (up/down)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["click", "type", "press", "scroll"],
                    "description": "The action to perform"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector to target (for click/type)"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type (required for 'type' action)"
                },
                "key": {
                    "type": "string",
                    "description": "Key to press (for 'press' action): Enter, Tab, Escape, ArrowDown, Backspace, etc."
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down"],
                    "description": "Scroll direction (for 'scroll' action). Default: 'down'"
                },
                "amount": {
                    "type": "integer",
                    "description": "Scroll amount in pixels. Default: 300"
                },
                "x": {
                    "type": "number",
                    "description": "X coordinate for click (alternative to selector)"
                },
                "y": {
                    "type": "number",
                    "description": "Y coordinate for click (alternative to selector)"
                },
                "id": {
                    "type": "string",
                    "description": "Browser instance ID. Default: 'default'"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "browser_action".into(), message: e.to_string() })?;

        let id = params.id.clone().unwrap_or_else(|| "default".to_string());

        let manager = self.manager.lock().await;
        let page = manager
            .active_page(&id)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "browser_action".into(), message: e })?;

        let result = match params.action.as_str() {
            "click" => execute_click(&page, &params).await,
            "type" => execute_type(&page, &params).await,
            "press" => execute_press(&page, &params).await,
            "scroll" => execute_scroll(&page, &params).await,
            other => Err(format!("Unknown action: '{}'. Use click/type/press/scroll.", other)),
        };

        match result {
            Ok(msg) => Ok(ToolOutput::text(json!({
                "status": "ok",
                "action": params.action,
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
    let direction = params.direction.as_deref().unwrap_or("down");
    let amount = params.amount.unwrap_or(300);

    let delta_y = match direction {
        "down" => amount,
        "up" => -amount,
        other => return Err(format!("Invalid scroll direction: '{}'. Use 'up' or 'down'.", other)),
    };

    let js = format!(
        "window.scrollBy({{ top: {}, behavior: 'smooth' }})",
        delta_y
    );

    page.evaluate(js)
        .await
        .map_err(|e| format!("Scroll failed: {}", e))?;

    Ok(format!("Scrolled {} by {} pixels", direction, amount))
}
