// INPUT:  std::collections::HashMap, std::path::PathBuf, serde_json::Value
// OUTPUT: pub struct RuntimeRequest, pub struct RuntimeOptions
// POS:    Defines the engine-agnostic request and options types for executing an agent session.

use std::collections::HashMap;
use std::path::PathBuf;

/// Request to execute an agent session.
#[derive(Debug, Clone)]
pub struct RuntimeRequest {
    /// User prompt.
    pub prompt: String,

    /// Resume an existing session (pass session_id).
    pub resume_session: Option<String>,

    /// Custom system prompt.
    pub system_prompt: Option<String>,

    /// Working directory for the agent.
    pub working_directory: Option<PathBuf>,

    /// Runtime options.
    pub options: RuntimeOptions,
}

/// Engine-agnostic runtime options.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
    /// Enable streaming deltas.
    pub streaming: bool,

    /// Maximum agentic turns.
    pub max_turns: Option<u32>,

    /// Engine-specific pass-through configuration.
    pub extra: HashMap<String, serde_json::Value>,
}

impl RuntimeRequest {
    /// Create a simple request with just a prompt.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            resume_session: None,
            system_prompt: None,
            working_directory: None,
            options: RuntimeOptions::default(),
        }
    }

    /// Set the working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(cwd.into());
        self
    }

    /// Enable streaming.
    pub fn with_streaming(mut self) -> Self {
        self.options.streaming = true;
        self
    }
}
