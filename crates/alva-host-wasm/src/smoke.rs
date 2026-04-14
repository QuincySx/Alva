// INPUT:  alva_kernel_core::{run_agent, AgentState, AgentConfig, MiddlewareStack, Extensions}, alva_kernel_abi::*, crate::StubLanguageModel
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

use std::sync::Arc;

use alva_kernel_abi::base::cancel::CancellationToken;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::model::ModelConfig;
use alva_kernel_abi::agent_session::InMemoryAgentSession;
use alva_kernel_abi::AgentMessage;
use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::run::run_agent;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};

use crate::StubLanguageModel;

/// Dead-code probe. Exists only to force the compiler to type-check
/// `run_agent`'s full surface from a wasm host's perspective. Uses the
/// public `StubLanguageModel` so the probe stays in sync with whatever
/// consumers and tests actually rely on.
async fn _wasm_smoke_probe() -> Result<(), AgentError> {
    let mut state = AgentState {
        model: Arc::new(StubLanguageModel::default()),
        tools: Vec::new(),
        session: Arc::new(InMemoryAgentSession::new()),
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
