// INPUT:  crate::run::run_agent, crate::state::{AgentState, AgentConfig}, alva_types::*
// OUTPUT: pub async fn run_child_agent(), pub struct ChildAgentParams, pub struct ChildAgentOutput
// POS:    Shared helper for running a child agent to completion and collecting its text output.
//         Used by agent_spawn (AI-driven), task_spawn (developer-constrained), and team (graph-based).

use std::sync::Arc;
use std::time::Duration;

use alva_types::base::cancel::CancellationToken;
use alva_types::base::message::Message;
use alva_types::model::LanguageModel;
use alva_types::session::{AgentSession, InMemorySession};
use alva_types::tool::Tool;
use alva_types::{AgentMessage, ModelConfig};

use crate::event::AgentEvent;
use crate::middleware::MiddlewareStack;
use crate::run::run_agent;
use crate::shared::Extensions;
use crate::state::{AgentConfig, AgentState};

/// Parameters for running a child agent.
pub struct ChildAgentParams {
    pub model: Arc<dyn LanguageModel>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub system_prompt: String,
    pub task: String,
    pub max_iterations: u32,
    pub timeout: Duration,
    /// If set, links the child session to a parent for tree-wide tracking.
    pub parent_session_id: Option<String>,
    pub cancel: CancellationToken,
    /// If set, the child agent uses this middleware stack (inherits parent's
    /// security, timeout, logging, etc.). If None, an empty stack is used.
    pub middleware: Option<MiddlewareStack>,
    /// If set, overrides the default ModelConfig.
    pub model_config: Option<ModelConfig>,
    /// Context window size. 0 = no limit.
    pub context_window: usize,
}

/// Output from a completed child agent run.
pub struct ChildAgentOutput {
    /// The collected text output (from events or session fallback).
    pub text: String,
    /// Whether the agent encountered an error.
    pub is_error: bool,
    /// Error message, if any.
    pub error: Option<String>,
}

/// Run a child agent to completion and collect its text output.
///
/// This encapsulates the common pattern shared by all sub-agent tools:
/// build V2 state → run_agent → collect output from events → fallback to session.
pub async fn run_child_agent(params: ChildAgentParams) -> ChildAgentOutput {
    let session: Arc<dyn AgentSession> = match &params.parent_session_id {
        Some(parent_id) => Arc::new(InMemorySession::with_parent(parent_id)),
        None => Arc::new(InMemorySession::new()),
    };

    let mut state = AgentState {
        model: params.model,
        tools: params.tools,
        session,
        extensions: Extensions::new(),
    };

    let config = AgentConfig {
        middleware: params.middleware.unwrap_or_default(),
        system_prompt: params.system_prompt,
        max_iterations: params.max_iterations,
        model_config: params.model_config.unwrap_or_default(),
        context_window: params.context_window,
        loop_hook: None,
    };

    let user_msg = AgentMessage::Standard(Message::user(&params.task));
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Run with timeout
    let result = tokio::time::timeout(params.timeout, async {
        run_agent(
            &mut state,
            &config,
            params.cancel.clone(),
            vec![user_msg],
            event_tx,
        )
        .await
    })
    .await;

    // Collect output from events
    let mut output = String::new();
    while let Ok(event) = event_rx.try_recv() {
        if let AgentEvent::MessageEnd { message } = event {
            if let AgentMessage::Standard(msg) = &message {
                let text = msg.text_content();
                if !text.is_empty() {
                    output.push_str(&text);
                }
            }
        }
    }

    // Fallback: collect from session messages
    if output.is_empty() {
        output = state
            .session
            .messages()
            .iter()
            .filter_map(|m| {
                if let AgentMessage::Standard(msg) = m {
                    if msg.role == alva_types::MessageRole::Assistant {
                        let text = msg.text_content();
                        if !text.is_empty() {
                            return Some(text);
                        }
                    }
                }
                None
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    match result {
        Ok(Ok(())) => ChildAgentOutput {
            text: output,
            is_error: false,
            error: None,
        },
        Ok(Err(e)) => ChildAgentOutput {
            text: output,
            is_error: true,
            error: Some(e.to_string()),
        },
        Err(_) => {
            params.cancel.cancel();
            ChildAgentOutput {
                text: output,
                is_error: true,
                error: Some(format!("timed out after {:?}", params.timeout)),
            }
        }
    }
}
