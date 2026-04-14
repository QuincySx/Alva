// INPUT:  std::sync::Arc, alva_kernel_abi::*, alva_kernel_core::*, crate::WasmSleeper (wasm only)
// OUTPUT: WasmAgent
// POS:    Minimal wasm-side consumer facade — bundles AgentState + AgentConfig + run_agent into one struct callable from app code.

//! `WasmAgent` — the smallest consumer-facing API for running an alva
//! kernel on wasm32. Mirrors the role of `alva-host-native::AgentRuntime`,
//! but stripped down to the pieces that compile + run in a browser:
//!
//! - `Arc<dyn LanguageModel>` — caller-supplied (typically a wasm-friendly
//!   provider built on `gloo-net` or `web_sys::fetch`)
//! - `Vec<Arc<dyn Tool>>` — caller-supplied; defaults to empty since the
//!   default `alva-agent-tools` set isn't wasm-clean yet
//! - `InMemorySession` — the only kernel session impl that's already
//!   wasm-friendly
//! - **No security middleware** — wasm is sandboxed by the browser
//! - **No CompactionMiddleware** — defer to ContextHooks once a wasm
//!   ContextHandle impl exists
//! - **No CheckpointMiddleware** — wasm has no fs to checkpoint to
//!
//! The struct is `pub` and intentionally tiny so callers can treat it as
//! a starting template, copy it, and add the pieces they actually need.

use std::sync::Arc;
use std::time::Duration;

use alva_kernel_abi::base::cancel::CancellationToken;
use alva_kernel_abi::model::{LanguageModel, ModelConfig};
use alva_kernel_abi::session::InMemorySession;
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::AgentMessage;
use alva_kernel_abi::{AgentError, Sleeper};
#[cfg(not(target_family = "wasm"))]
use alva_kernel_abi::NoopSleeper;
use alva_kernel_core::builtins::ToolTimeoutMiddleware;
use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::run::run_agent;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::AgentEvent;

/// Minimal wasm-side runtime facade.
pub struct WasmAgent {
    state: AgentState,
    config: AgentConfig,
}

impl WasmAgent {
    /// Construct a wasm runtime with the given model and (optional) tools.
    /// Installs `ToolTimeoutMiddleware` with a 120-second budget driven by
    /// the platform's native sleeper (`WasmSleeper` on wasm32,
    /// `NoopSleeper` on native — native paths are test-only and don't
    /// rely on the timeout firing).
    ///
    /// All other knobs use sensible defaults; users that need finer control
    /// should construct `AgentState` + `AgentConfig` directly or grab
    /// `config_mut()` to adjust the middleware stack.
    pub fn new(
        model: Arc<dyn LanguageModel>,
        tools: Vec<Arc<dyn Tool>>,
        system_prompt: impl Into<String>,
    ) -> Self {
        let state = AgentState {
            model,
            tools,
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        };

        let mut middleware = MiddlewareStack::new();
        middleware.push_sorted(Arc::new(ToolTimeoutMiddleware::with_sleeper(
            Duration::from_secs(120),
            Self::default_sleeper(),
        )));

        let config = AgentConfig {
            middleware,
            system_prompt: system_prompt.into(),
            max_iterations: 50,
            model_config: ModelConfig::default(),
            context_window: 0,
            workspace: None,
            bus: None,
            context_system: None,
            context_token_budget: None,
        };
        Self { state, config }
    }

    /// Pick the right `Sleeper` for the current target. On wasm32 this is
    /// the real `WasmSleeper` backed by `gloo-timers`; on native (tests
    /// only) it falls back to `NoopSleeper` which never fires — that's
    /// fine because tests rely on cancellation/completion, not on real
    /// wall-clock timeouts.
    fn default_sleeper() -> Arc<dyn Sleeper> {
        #[cfg(target_family = "wasm")]
        {
            Arc::new(crate::WasmSleeper)
        }
        #[cfg(not(target_family = "wasm"))]
        {
            Arc::new(NoopSleeper)
        }
    }

    /// Run the agent against a single user input and stream `AgentEvent`s
    /// through the provided sender. Returns when `run_agent` finishes
    /// (naturally or via cancellation).
    pub async fn run(
        &mut self,
        prompt: impl Into<String>,
        cancel: CancellationToken,
        event_tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(), AgentError> {
        let input = vec![AgentMessage::Standard(
            alva_kernel_abi::base::message::Message::user(&prompt.into()),
        )];
        run_agent(&mut self.state, &self.config, cancel, input, event_tx).await
    }

    /// One-shot convenience: run the agent against `prompt`, drain events
    /// internally, and return the concatenated text of every assistant
    /// `MessageEnd` produced during the run. This is the 99% wasm use
    /// case — callers who need streaming, tool observation, or custom
    /// event handling should use [`run`] directly.
    ///
    /// Uses a fresh `CancellationToken` internally; if the caller needs
    /// to cancel the run from outside, use [`run`] with their own token.
    pub async fn run_simple(
        &mut self,
        prompt: impl Into<String>,
    ) -> Result<String, AgentError> {
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        self.run(prompt, cancel, tx).await?;

        let mut output = String::new();
        while let Ok(event) = rx.try_recv() {
            if let AgentEvent::MessageEnd { message } = event {
                if let AgentMessage::Standard(msg) = message {
                    let text = msg.text_content();
                    if !text.is_empty() {
                        if !output.is_empty() {
                            output.push('\n');
                        }
                        output.push_str(&text);
                    }
                }
            }
        }
        Ok(output)
    }

    /// Borrow the underlying `AgentState`. Useful for inspecting `session`
    /// after a run, or wiring custom middleware before calling `run`.
    pub fn state(&self) -> &AgentState {
        &self.state
    }

    /// Borrow the underlying `AgentConfig` mutably so callers can mutate
    /// middleware / max_iterations / context_token_budget / etc. before
    /// the next `run` call.
    pub fn config_mut(&mut self) -> &mut AgentConfig {
        &mut self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::base::content::ContentBlock;
    use alva_kernel_abi::base::error::AgentError;
    use alva_kernel_abi::base::message::{Message, MessageRole};
    use alva_kernel_abi::base::stream::StreamEvent;
    use alva_kernel_abi::model::CompletionResponse;
    use async_trait::async_trait;
    use futures_core::Stream;
    use std::pin::Pin;

    /// Tiny model that returns a fixed text response with no tool calls.
    struct EchoOnceModel;
    #[async_trait]
    impl LanguageModel for EchoOnceModel {
        async fn complete(
            &self,
            _: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            Ok(CompletionResponse::from_message(Message {
                id: "r1".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text { text: "echo-ok".into() }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            }))
        }
        fn stream(
            &self,
            _: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            // Use a minimal hand-rolled stream so we don't need futures crate.
            struct Once(Option<StreamEvent>, bool);
            impl Stream for Once {
                type Item = StreamEvent;
                fn poll_next(
                    mut self: Pin<&mut Self>,
                    _cx: &mut std::task::Context<'_>,
                ) -> std::task::Poll<Option<Self::Item>> {
                    if let Some(ev) = self.0.take() {
                        std::task::Poll::Ready(Some(ev))
                    } else if !self.1 {
                        self.1 = true;
                        std::task::Poll::Ready(Some(StreamEvent::Done))
                    } else {
                        std::task::Poll::Ready(None)
                    }
                }
            }
            Box::pin(Once(
                Some(StreamEvent::TextDelta { text: "echo-ok".into() }),
                false,
            ))
        }
        fn model_id(&self) -> &str { "echo-once" }
    }

    #[tokio::test]
    async fn wasm_agent_runs_a_single_turn_on_native() {
        let mut agent = WasmAgent::new(Arc::new(EchoOnceModel), vec![], "");
        let cancel = CancellationToken::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = agent.run("hi", cancel, tx).await;
        assert!(result.is_ok(), "WasmAgent::run should succeed: {:?}", result);
        // Session should now contain at least the user input + the assistant response.
        assert!(
            agent.state().session.len() >= 2,
            "expected session to have user + assistant messages, got {} entries",
            agent.state().session.len()
        );
    }

    #[tokio::test]
    async fn wasm_agent_run_simple_returns_response_text() {
        let mut agent = WasmAgent::new(Arc::new(EchoOnceModel), vec![], "");
        let output = agent
            .run_simple("hello")
            .await
            .expect("run_simple should succeed");
        assert_eq!(
            output, "echo-ok",
            "run_simple should return the assistant response text"
        );
    }
}
