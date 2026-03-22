// INPUT:  crate::domain::tool, crate::error, async_trait, serde_json, std::collections
// OUTPUT: ToolContext, Tool (trait), ToolRegistry
// POS:    Defines the abstract tool interface, execution context, and name-based registry.
//         Kept during migration because runtime/tools/ depends on these.
//         TODO: Migrate to agent-base equivalents when tool implementations are ported.
use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// Runtime context injected into tool execution
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub allow_dangerous: bool,
}

/// Abstract tool interface
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (must match ToolCall.name from LLM)
    fn name(&self) -> &str;

    /// JSON Schema definition for function calling
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with JSON input
    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError>;
}

/// Registry holding all available tools
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
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
