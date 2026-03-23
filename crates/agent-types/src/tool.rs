use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;

use crate::cancel::CancellationToken;
use crate::error::AgentError;

// ---------------------------------------------------------------------------
// ToolDefinition — JSON Schema description for LLM function calling
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing parameters
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// ToolCall / ToolResult — wire types flowing through the agent loop
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ToolContext — base runtime context for tools (generic, no filesystem assumptions)
// ---------------------------------------------------------------------------

/// Base runtime context for tools — generic, no filesystem assumptions.
///
/// This is a trait (not a concrete struct) so that agent-types stays generic.
/// Each application layer provides its own implementation.
/// Tools that don't need context can ignore the parameter.
pub trait ToolContext: Send + Sync {
    /// Current session identifier.
    fn session_id(&self) -> &str;

    /// Read a configuration value by key.
    fn get_config(&self, key: &str) -> Option<String>;

    /// Downcast support.
    fn as_any(&self) -> &dyn Any;

    /// Try to get local filesystem context. Returns None for remote contexts.
    fn local(&self) -> Option<&dyn LocalToolContext> {
        None
    }
}

// ---------------------------------------------------------------------------
// LocalToolContext — extension for tools that operate on a local filesystem
// ---------------------------------------------------------------------------

/// Extension trait for tools that operate on a local filesystem.
pub trait LocalToolContext: ToolContext {
    /// Current workspace / project root path.
    fn workspace(&self) -> &std::path::Path;

    /// Whether the tool is allowed to perform dangerous operations.
    fn allow_dangerous(&self) -> bool;
}

/// No-op context for tools that don't need runtime information.
pub struct EmptyToolContext;

impl ToolContext for EmptyToolContext {
    fn session_id(&self) -> &str {
        ""
    }
    fn get_config(&self, _key: &str) -> Option<String> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn local(&self) -> Option<&dyn LocalToolContext> {
        Some(self)
    }
}

impl LocalToolContext for EmptyToolContext {
    fn workspace(&self) -> &std::path::Path {
        std::path::Path::new(".")
    }
    fn allow_dangerous(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tool trait — the single canonical tool abstraction
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (must match ToolCall.name from LLM).
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema for parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Full definition for LLM function calling (convenience).
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }

    /// Execute the tool.
    ///
    /// Both `cancel` and `ctx` are provided. Tools that don't need runtime
    /// context can ignore `ctx`. Tools that don't need cancellation can
    /// ignore `cancel`.
    async fn execute(
        &self,
        input: serde_json::Value,
        cancel: &CancellationToken,
        ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError>;
}

// ---------------------------------------------------------------------------
// ToolRegistry — name → Tool lookup
// ---------------------------------------------------------------------------

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub fn remove(&mut self, name: &str) -> Option<Box<dyn Tool>> {
        self.tools.remove(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
