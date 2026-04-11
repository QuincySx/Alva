//! Bridges MiddlewareStack hooks to ExtensionHost event dispatch.

use std::sync::{Arc, RwLock};
use async_trait::async_trait;
use alva_agent_core::middleware::{Middleware, MiddlewarePriority};
use alva_agent_core::shared::MiddlewareError;
use alva_agent_core::state::AgentState;
use alva_types::{ToolCall, ToolOutput};
use super::host::ExtensionHost;
use super::events::{ExtensionEvent, EventResult};

/// Bridges MiddlewareStack hooks to ExtensionHost event dispatch.
pub struct ExtensionBridgeMiddleware {
    host: Arc<RwLock<ExtensionHost>>,
}

impl ExtensionBridgeMiddleware {
    pub fn new(host: Arc<RwLock<ExtensionHost>>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Middleware for ExtensionBridgeMiddleware {
    fn name(&self) -> &str { "extension_bridge" }
    fn priority(&self) -> i32 { MiddlewarePriority::OBSERVATION + 200 }

    async fn on_agent_start(&self, _state: &mut AgentState) -> Result<(), MiddlewareError> {
        let host = self.host.read().unwrap();
        host.emit(&ExtensionEvent::AgentStart);
        Ok(())
    }

    async fn on_agent_end(&self, _state: &mut AgentState, error: Option<&str>) -> Result<(), MiddlewareError> {
        let host = self.host.read().unwrap();
        host.emit(&ExtensionEvent::AgentEnd { error: error.map(|s| s.to_string()) });
        Ok(())
    }

    async fn before_tool_call(&self, _state: &mut AgentState, tool_call: &ToolCall) -> Result<(), MiddlewareError> {
        let host = self.host.read().unwrap();
        let event = ExtensionEvent::BeforeToolCall {
            tool_name: tool_call.name.clone(),
            tool_call_id: tool_call.id.clone(),
            arguments: tool_call.arguments.clone(),
        };
        match host.emit(&event) {
            EventResult::Block { reason } => Err(MiddlewareError::Blocked { reason }),
            _ => Ok(()),
        }
    }

    async fn after_tool_call(&self, _state: &mut AgentState, tool_call: &ToolCall, result: &mut ToolOutput) -> Result<(), MiddlewareError> {
        let host = self.host.read().unwrap();
        let event = ExtensionEvent::AfterToolCall {
            tool_name: tool_call.name.clone(),
            tool_call_id: tool_call.id.clone(),
            result: result.clone(),
        };
        host.emit(&event);
        Ok(())
    }
}
