// INPUT:  alva_kernel_core::{run_agent, AgentState, AgentConfig, MiddlewareStack, Extensions}, alva_kernel_abi::*
// OUTPUT: _wasm_smoke_probe (private dead-code function, exists only to type-check the kernel surface for wasm)
// POS:    Compile-time probe — never called, exists to force `cargo check --target wasm32` to exercise run_agent's full type surface.

//! Compile-time probe for the kernel API surface on wasm32.
//!
//! This module exists for **one** reason: to make `cargo check --target
//! wasm32-unknown-unknown -p alva-host-wasm` actually exercise the
//! `alva-kernel-core` types — `run_agent`, `AgentState`, `AgentConfig`,
//! `LanguageModel`, `Tool`, etc. — instead of just compiling an empty lib.
//!
//! Without this probe, the wasm32 cargo check on alva-host-wasm could pass
//! while still missing some import that is wasm-incompatible. The probe
//! constructs a minimal stub agent and references `run_agent`, forcing the
//! compiler to check the full kernel API surface for wasm-friendliness.
//!
//! The function is `#[allow(dead_code)]` and never called — its purpose is
//! purely to fail compilation if a kernel commit accidentally introduces a
//! wasm-blocking dep through a type used by a downstream wasm host.

#![allow(dead_code)]

use std::pin::Pin;
use std::sync::Arc;

use alva_kernel_abi::base::cancel::CancellationToken;
use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::{Message, MessageRole};
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::session::InMemorySession;
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::AgentMessage;
use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::run::run_agent;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use async_trait::async_trait;
use futures_core::Stream;

/// Stub model — type-checks `LanguageModel` but never executes.
struct WasmStubModel;

#[async_trait]
impl LanguageModel for WasmStubModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse::from_message(Message {
            id: "stub".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "stub".to_string() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        // We never poll this in production — just construct an empty boxed
        // stream so the type checker is satisfied.
        Box::pin(EmptyStream)
    }

    fn model_id(&self) -> &str {
        "wasm-stub"
    }
}

/// Trivial empty stream that ends immediately. Exists to satisfy the
/// `LanguageModel::stream` return type without pulling in `tokio_stream` or
/// `futures` umbrella crates as deps.
struct EmptyStream;

impl Stream for EmptyStream {
    type Item = StreamEvent;
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(None)
    }
}

/// Dead-code probe. Exists only to force the compiler to type-check
/// `run_agent`'s full surface from a wasm host's perspective.
async fn _wasm_smoke_probe() -> Result<(), AgentError> {
    let mut state = AgentState {
        model: Arc::new(WasmStubModel),
        tools: Vec::new(),
        session: Arc::new(InMemorySession::new()),
        extensions: Extensions::new(),
    };
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: String::new(),
        max_iterations: 1,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("probe"))],
        tx,
    )
    .await
}
