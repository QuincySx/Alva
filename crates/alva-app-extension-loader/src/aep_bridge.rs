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
//! reading the latest user message off the session. `on_agent_start`
//! can fire multiple times within one user turn (each tool loop), so
//! the bridge deduplicates by the latest user message's id and only
//! re-dispatches `on_user_message` when the message actually changes —
//! preserving the old `input` event's "fires once per new input"
//! semantics.
//!
//! Each hook iterates plugins sequentially; `dispatch_event_sync`
//! internally short-circuits plugins that did not subscribe to the
//! event, so a plugin only pays the JSON-RPC round-trip for events it
//! asked for. The first plugin that returns `Block` from
//! `before_tool_call` wins — same semantics the old `emit` loop had.
//!
//! ## Trust model — fail-open, non-authoritative
//!
//! AEP plugins are third-party subprocesses and are treated as an
//! *enhancement* layer, **not** an authoritative security gate:
//!
//! - **Priority.** This middleware runs at
//!   [`MiddlewarePriority::HOOKS`] (1500) — the same tier as other
//!   user-installed extensions (e.g. `HooksPlugin`). That is
//!   deliberately **after** the built-in SECURITY tier (1000: auth /
//!   permission / sandbox), so first-party security stays
//!   authoritative and runs first. It is still **before**
//!   GUARDRAIL (2000) / CONTEXT (3000) / OBSERVATION (5000) / RETRY
//!   (6000), so a plugin `Block` on `before_tool_call` still pre-empts
//!   logging and retries.
//! - **Fail-open.** If a plugin's RPC errors or times out, the dispatch
//!   is treated as `Continue` (the tool is allowed) — see
//!   [`dispatch_outcome`](crate::proxy) and its test. A wedged or
//!   crashed plugin therefore never blocks a tool. The threat model
//!   assumes AEP plugins may misbehave; relying on one to *block*
//!   something is unsupported — use built-in SECURITY-tier middleware
//!   for hard gates.

use std::sync::{Arc, Mutex};

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
    /// Id of the most recently dispatched `on_user_message`, used to
    /// avoid re-firing the event for the same user message across the
    /// multiple `on_agent_start` calls that happen within one turn.
    /// See the module-level "fires once per new input" note.
    last_user_message_id: Mutex<Option<String>>,
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
        Self {
            plugins,
            last_user_message_id: Mutex::new(None),
        }
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

    /// Run in the user-hook tier (1500), not the built-in SECURITY
    /// tier (1000). Third-party AEP plugins are an enhancement layer,
    /// not an authoritative security gate — they must run *after*
    /// first-party security (sandbox / permissions / PlanMode), but
    /// still *before* GUARDRAIL/CONTEXT/OBSERVATION/RETRY so a plugin
    /// `Block` on `before_tool_call` pre-empts logging and retries.
    /// See the module-level "Trust model" note.
    fn priority(&self) -> i32 {
        MiddlewarePriority::HOOKS
    }

    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        if self.plugins.is_empty() {
            return Ok(());
        }
        // `on_agent_start` subscribers first.
        self.dispatch(&AepEvent::AgentStart)?;

        // `on_user_message` is reconstructed from session state — the
        // kernel no longer emits a dedicated input event. Dedup by id
        // so the same user message isn't re-sent on every tool-loop
        // iteration's `on_agent_start`.
        if let Some((id, text)) = latest_user_message(state).await {
            let changed = {
                let mut guard = self.last_user_message_id.lock().unwrap();
                if guard.as_deref() == Some(id.as_str()) {
                    false
                } else {
                    *guard = Some(id);
                    true
                }
            };
            if changed {
                self.dispatch(&AepEvent::UserMessage { text: &text })?;
            }
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

/// Pull the `(id, text)` of the most recent user message off the
/// session, or `None` if there is no user message yet.
///
/// `id` is the `Message::id` (a stable per-message uuid) and is used by
/// `on_agent_start` to dedup repeated dispatch of the same user
/// message across a turn's tool loop.
async fn latest_user_message(state: &AgentState) -> Option<(String, String)> {
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
                Some((msg.id.clone(), msg.text_content()))
            }
            _ => None,
        })
}
