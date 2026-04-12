// INPUT:  async_trait, serde, serde_json, crate::base::error::AgentError
// OUTPUT: ToolDefinition, ToolCall, Tool (trait), ToolRegistry, ToolFs, ToolFsExecResult, ToolFsDirEntry
// POS:    Canonical tool abstractions — defines the Tool trait (using ToolExecutionContext), wire types, and a name-based registry.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::base::error::AgentError;

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
// ToolCall — wire type flowing through the agent loop
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

// NOTE: ToolContext and LocalToolContext have been removed.
// Use tool::execution::ToolExecutionContext instead.

// ---------------------------------------------------------------------------
// ToolPermissionResult — permission check outcome
// ---------------------------------------------------------------------------

/// Result of a tool permission check.
#[derive(Debug, Clone)]
pub enum ToolPermissionResult {
    /// Tool use is allowed.
    Allow,
    /// Tool use is denied with a reason.
    Deny(String),
    /// Tool use requires user confirmation with a question.
    Ask(String),
}

// ---------------------------------------------------------------------------
// SearchReadInfo — search/read nature of a tool invocation
// ---------------------------------------------------------------------------

/// Describes the search/read nature of a tool invocation.
#[derive(Debug, Clone)]
pub struct SearchReadInfo {
    pub is_search: bool,
    pub is_read: bool,
    pub is_list: bool,
}

// ---------------------------------------------------------------------------
// ToolFs — abstract filesystem + command execution interface
// ---------------------------------------------------------------------------

/// Abstract filesystem + command execution interface.
///
/// Tools call these methods instead of direct system APIs.
/// Implementations include local FS, sandbox delegates, or mocks.
#[async_trait]
pub trait ToolFs: Send + Sync {
    /// Execute a shell command. Returns (stdout, stderr, exit_code).
    async fn exec(
        &self,
        command: &str,
        cwd: Option<&str>,
        timeout_ms: u64,
    ) -> Result<ToolFsExecResult, AgentError>;

    /// Read a file's contents.
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError>;

    /// Write content to a file (creates parent dirs as needed).
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError>;

    /// List directory entries (non-recursive).
    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError>;

    /// Check if a path exists.
    async fn exists(&self, path: &str) -> Result<bool, AgentError>;
}

/// Result of ToolFs::exec().
#[derive(Debug, Clone)]
pub struct ToolFsExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ToolFsExecResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Directory entry from ToolFs::list_dir().
#[derive(Debug, Clone)]
pub struct ToolFsDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

// NOTE: EmptyToolContext has been removed.
// Use tool::execution::MinimalExecutionContext instead.

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
    /// The `ctx` provides cancellation, progress reporting, configuration,
    /// filesystem access, and session information — everything the tool
    /// needs from its execution environment.
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn super::execution::ToolExecutionContext,
    ) -> Result<super::execution::ToolOutput, AgentError>;

    // -----------------------------------------------------------------------
    // Extended metadata methods (all have defaults so existing impls compile)
    // -----------------------------------------------------------------------

    /// Whether this tool can safely run concurrently with other tool calls.
    fn is_concurrency_safe(&self, _input: &serde_json::Value) -> bool {
        false
    }

    /// Whether this tool invocation is purely read-only (no side effects).
    fn is_read_only(&self, _input: &serde_json::Value) -> bool {
        false
    }

    /// Whether this tool invocation is destructive (deletes files, drops data, etc.).
    fn is_destructive(&self, _input: &serde_json::Value) -> bool {
        false
    }

    /// Whether this tool manages its own execution timeout.
    ///
    /// When `true`, generic per-tool timeout middleware (e.g.
    /// `ToolTimeoutMiddleware`) MUST NOT wrap this call with an additional
    /// timeout. The tool is then solely responsible for bounding its own
    /// runtime — via a scope budget, an internal cancellation protocol, or
    /// similar.
    ///
    /// Examples:
    /// - Sub-agent spawning tools that enforce their own per-scope budget
    /// - Long-running stream/watch tools with explicit cancellation
    ///
    /// Default: `false` (the middleware's default timeout applies).
    fn manages_own_timeout(&self) -> bool {
        false
    }

    /// Classify the search/read nature of this invocation, if applicable.
    fn is_search_or_read(&self, _input: &serde_json::Value) -> Option<SearchReadInfo> {
        None
    }

    /// Check whether the tool should be allowed to run with the given input.
    fn check_permissions(
        &self,
        _input: &serde_json::Value,
        _ctx: &dyn super::execution::ToolExecutionContext,
    ) -> ToolPermissionResult {
        ToolPermissionResult::Allow
    }

    /// A human-friendly display name, potentially customized based on input.
    fn user_facing_name(&self, _input: &serde_json::Value) -> String {
        self.name().to_string()
    }

    /// Maximum number of characters allowed in a tool result before truncation.
    fn max_result_size_chars(&self) -> Option<usize> {
        None
    }

    /// Whether this tool should be deferred (not shown in the initial tool list).
    fn should_defer(&self) -> bool {
        false
    }

    /// Alternative names that can be used to invoke this tool.
    fn aliases(&self) -> Vec<String> {
        vec![]
    }

    /// Whether this tool is currently enabled and available.
    fn is_enabled(&self) -> bool {
        true
    }

    /// The prompt/instructions text for this tool (defaults to description).
    fn tool_prompt(&self) -> String {
        self.description().to_string()
    }
}

// ---------------------------------------------------------------------------
// ToolRegistry — name → Tool lookup
// ---------------------------------------------------------------------------

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::from(tool));
    }

    /// Register a tool that is already wrapped in an Arc.
    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Get a cloned Arc reference to a tool by name.
    pub fn get_arc(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    /// Get all tools as Arc references.
    pub fn list_arc(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.values().cloned().collect()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub fn remove(&mut self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.remove(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
