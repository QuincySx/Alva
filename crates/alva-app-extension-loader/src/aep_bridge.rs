// INPUT:  proxy::{RemoteExtensionProxy, AepEvent, AepDispatchResult},
//         alva_kernel_core::middleware::Middleware, alva_kernel_abi::{ToolCall, ToolOutput}
// OUTPUT: AepBridgeMiddleware
// POS:    Phase 3 — routes real Middleware hooks into the loaded AEP plugins.

//! `AepBridgeMiddleware` — the single `Middleware` that fans agent
//! lifecycle / tool hooks out to every loaded subprocess plugin.
//!
//! This replaces the old event-layer path (one `HostAPI::on_as`
//! handler per plugin per subscription, driven by
//! `ExtensionHost::emit`). The loader now registers exactly one
//! middleware that owns `Arc` handles to every plugin and translates
//! each `Middleware` hook into a loader-local
//! [`AepEvent`](crate::proxy::AepEvent):
//!
//! | Middleware hook         | AEP subscription(s) dispatched         |
//! |-------------------------|----------------------------------------|
//! | `on_agent_start`        | `on_agent_start` **and** `on_user_message` |
//! | `on_agent_end`          | `on_agent_end`                         |
//! | `before_tool_call`      | `before_tool_call` (can **block**)     |
//! | `after_tool_call`       | `after_tool_call`                      |
//!
//! There is no longer a dedicated "input" event in the kernel, so
//! `on_user_message` is reconstructed inside `on_agent_start` by
//! reading the latest user message text off the session.
//!
//! Each hook iterates plugins sequentially; `dispatch_event_sync`
//! internally short-circuits plugins that did not subscribe to the
//! event, so a plugin only pays the JSON-RPC round-trip for events it
//! asked for. The first plugin that returns `Block` from
//! `before_tool_call` wins — same semantics the old `emit` loop had.

use std::sync::Arc;

use async_trait::async_trait;

use alva_kernel_abi::{AgentMessage, MessageRole, ToolCall, ToolOutput};
use alva_kernel_core::middleware::{Middleware, MiddlewareError, MiddlewarePriority};
use alva_kernel_core::state::AgentState;

use crate::loader::aep_to_core_event_type;
use crate::proxy::{AepDispatchResult, AepEvent, RemoteExtensionProxy};

/// The one `Middleware` the loader registers to route host hooks into
/// all loaded subprocess plugins.
pub struct AepBridgeMiddleware {
    plugins: Vec<Arc<RemoteExtensionProxy>>,
}

impl AepBridgeMiddleware {
    /// Build a bridge over the already-loaded plugins.
    ///
    /// Construction logs (but does not reject) any subscription name
    /// the host does not understand — mirroring the old
    /// `register_plugin_handlers` warning so an unknown subscription
    /// is visible without breaking the whole loader.
    pub fn new(plugins: Vec<Arc<RemoteExtensionProxy>>) -> Self {
        for plugin in &plugins {
            for aep_name in &plugin.init_result().event_subscriptions {
                if aep_to_core_event_type(aep_name).is_none() {
                    tracing::warn!(
                        plugin = %plugin.name(),
                        event = %aep_name,
                        "plugin subscribed to unknown AEP event; it will never fire"
                    );
                }
            }
        }
        Self { plugins }
    }

    /// Dispatch one event to every plugin, returning the first `Block`
    /// as a `MiddlewareError::Blocked`. Plugins that did not subscribe
    /// are skipped cheaply inside `dispatch_event_sync`.
    fn dispatch(&self, event: &AepEvent<'_>) -> Result<(), MiddlewareError> {
        for plugin in &self.plugins {
            if let AepDispatchResult::Block { reason } = plugin.dispatch_event_sync(event) {
                return Err(MiddlewareError::Blocked { reason });
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Middleware for AepBridgeMiddleware {
    fn name(&self) -> &str {
        "aep-bridge"
    }

    /// Run in the security tier so a plugin `Block` on
    /// `before_tool_call` rejects the tool before later middleware
    /// (logging, retries) observes it.
    fn priority(&self) -> i32 {
        MiddlewarePriority::SECURITY
    }

    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        if self.plugins.is_empty() {
            return Ok(());
        }
        // `on_agent_start` subscribers first.
        self.dispatch(&AepEvent::AgentStart)?;

        // `on_user_message` is reconstructed from session state — the
        // kernel no longer emits a dedicated input event.
        if let Some(text) = latest_user_message_text(state).await {
            self.dispatch(&AepEvent::UserMessage { text: &text })?;
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        _state: &mut AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        if self.plugins.is_empty() {
            return Ok(());
        }
        self.dispatch(&AepEvent::AgentEnd { error })
    }

    async fn before_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if self.plugins.is_empty() {
            return Ok(());
        }
        self.dispatch(&AepEvent::BeforeToolCall {
            tool_name: &tool_call.name,
            tool_call_id: &tool_call.id,
            arguments: &tool_call.arguments,
        })
    }

    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        if self.plugins.is_empty() {
            return Ok(());
        }
        // after_tool_call is observational — a Block here has no tool
        // to reject, so we ignore the (already-honoured-as-Continue)
        // result and never fail the hook.
        let _ = self.dispatch(&AepEvent::AfterToolCall {
            tool_name: &tool_call.name,
            tool_call_id: &tool_call.id,
            result,
        });
        Ok(())
    }
}

/// Pull the text of the most recent user message off the session, or
/// `None` if there is no user message yet.
async fn latest_user_message_text(state: &AgentState) -> Option<String> {
    state
        .session
        .messages()
        .await
        .into_iter()
        .rev()
        .find_map(|m| match m {
            AgentMessage::Standard(msg) | AgentMessage::Steering(msg)
                if msg.role == MessageRole::User =>
            {
                Some(msg.text_content())
            }
            _ => None,
        })
}
