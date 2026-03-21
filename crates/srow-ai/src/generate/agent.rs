use std::sync::Arc;

use srow_core::error::ChatError;
use srow_core::ports::provider::language_model::LanguageModel;
use srow_core::ports::tool::ToolRegistry;

use super::generate_text::generate_text;
use super::stop_condition::{step_count_is, StopCondition};
use super::stream_text::stream_text;
use super::types::*;

/// Agent — a configured wrapper around generate_text/stream_text.
///
/// Provides a builder-style API for configuring an AI agent with a model,
/// tools, instructions, and stop conditions. Default stop_when: step_count_is(20)
/// (AI SDK convention).
///
/// # Example
/// ```ignore
/// let agent = Agent::new(model)
///     .with_instructions("You are a helpful assistant.")
///     .with_tools(tool_registry)
///     .with_max_output_tokens(4096);
///
/// let result = agent.generate(Prompt::Text("Hello".into())).await?;
/// ```
pub struct Agent {
    pub id: Option<String>,
    pub instructions: Option<String>,
    pub model: Arc<dyn LanguageModel>,
    pub tools: Option<Arc<ToolRegistry>>,
    pub stop_when: Option<Arc<dyn StopCondition>>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub workspace: std::path::PathBuf,
}

impl Agent {
    pub fn new(model: Arc<dyn LanguageModel>) -> Self {
        Self {
            id: None,
            instructions: None,
            model,
            tools: None,
            stop_when: None,
            max_output_tokens: Some(8192),
            temperature: None,
            workspace: std::path::PathBuf::from("."),
        }
    }

    /// Set the agent identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the system instructions for the agent.
    pub fn with_instructions(mut self, s: impl Into<String>) -> Self {
        self.instructions = Some(s.into());
        self
    }

    /// Provide a tool registry for tool-use loops.
    pub fn with_tools(mut self, t: Arc<ToolRegistry>) -> Self {
        self.tools = Some(t);
        self
    }

    /// Set a custom stop condition. Defaults to step_count_is(20).
    pub fn with_stop_when(mut self, s: Arc<dyn StopCondition>) -> Self {
        self.stop_when = Some(s);
        self
    }

    /// Set the maximum number of output tokens per LLM call.
    pub fn with_max_output_tokens(mut self, n: u32) -> Self {
        self.max_output_tokens = Some(n);
        self
    }

    /// Set the temperature for generation.
    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Set the workspace directory for tool execution.
    pub fn with_workspace(mut self, w: std::path::PathBuf) -> Self {
        self.workspace = w;
        self
    }

    /// Run non-streaming generation with the agent's configuration.
    pub async fn generate(&self, prompt: Prompt) -> Result<GenerateTextResult, ChatError> {
        generate_text(self.to_call_settings(), prompt).await
    }

    /// Run streaming generation with the agent's configuration.
    ///
    /// Returns immediately with a `StreamTextResult` containing channels
    /// for real-time chunks and final values.
    pub fn stream(&self, prompt: Prompt) -> StreamTextResult {
        stream_text(self.to_call_settings(), prompt)
    }

    /// Convert Agent fields into CallSettings for the underlying functions.
    fn to_call_settings(&self) -> CallSettings {
        CallSettings {
            model: self.model.clone(),
            system: self.instructions.clone(),
            tools: self.tools.clone(),
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            stop_when: Some(
                self.stop_when
                    .clone()
                    .unwrap_or_else(|| step_count_is(20)),
            ),
            max_retries: 2,
            workspace: self.workspace.clone(),
        }
    }
}
