// INPUT:  async_trait, serde, serde_json, crate::base::error::AgentError
// OUTPUT: ToolDefinition, ToolCall, Tool (trait), ToolRegistry, ToolFs, ToolFsExecResult, ToolFsDirEntry
// POS:    Canonical tool abstractions with deterministic registration order and explicit replacement detection.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::schema::ToolSchemaContext;
use crate::base::error::AgentError;

// ---------------------------------------------------------------------------
// ToolDefinition — JSON Schema description for LLM function calling
// (definition relocated to alva-llm-wire; re-exported here for backward compat)
// ---------------------------------------------------------------------------

pub use alva_llm_wire::tool_def::ToolDefinition;

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
// EffectDescription — what this invocation will do (no judgment)
// ---------------------------------------------------------------------------

/// 工具对"这份入参会让我干什么"的自述 —— **不含判断**。
///
/// "允不允许执行、要不要人工确认"是 harness 的工具策略层的决策动词；
/// 同一个工具在不同安全档下答案不同，工具无权替产品拍板，更不能拒绝
/// 自己。工具只负责描述：只有工具知道自己的入参长什么样，harness 为了
/// 拼审批提示语不应该知道某个具体工具的字段。
#[derive(Debug, Clone)]
pub struct EffectDescription {
    /// 一句话自述这份入参的效果。harness 需要审批提示语时可原样引用；
    /// 默认实现回退到工具的 `description()`。
    pub summary: String,
}

impl EffectDescription {
    pub fn of(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
        }
    }
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

    /// JSON Schema for parameters, with access to runtime state.
    ///
    /// `ctx` carries the config bus when the run loop asks (most production
    /// paths); providers and tests outside a run pass
    /// [`ToolSchemaContext::empty`]. Tools whose schema depends on live
    /// runtime state (e.g. a dynamic enum of sibling tool names, registered
    /// `SpawnCommunication` kinds, MCP servers, Skills) read it from
    /// `ctx.bus`; static tools ignore the argument.
    fn parameters_schema(&self, ctx: &ToolSchemaContext) -> serde_json::Value;

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
    // 调度与只读自述（唯一带默认值的两个非核心方法）
    // -----------------------------------------------------------------------

    /// 该调用如何被并发执行 —— 调度描述符。见 [`Scheduling`]。
    ///
    /// 内核批量执行工具时唯一需要向工具打听的额外信息；默认值与拆分前的
    /// 四个方法默认逐条一致（不进并行波、无锁、自管超时关闭）。
    fn scheduling(&self, _input: &serde_json::Value) -> super::scheduler::Scheduling {
        super::scheduler::Scheduling::default()
    }

    /// Whether this tool invocation is purely read-only (no side effects).
    fn is_read_only(&self, _input: &serde_json::Value) -> bool {
        false
    }

    /// 自述这份入参会让我干什么 —— 不含判断。
    ///
    /// 决策（允许 / 拒绝 / 询问）住在 harness 的工具策略层；策略层需要
    /// 审批提示语时引用这里的 `summary`。默认自述就是工具的 `description()`。
    fn describe_effect(&self, _input: &serde_json::Value) -> EffectDescription {
        EffectDescription::of(self.description())
    }
}

// ---------------------------------------------------------------------------
// ToolRegistry — name → Tool lookup
// ---------------------------------------------------------------------------

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    order: Vec<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Register a tool. Returns `true` for a new name and `false` when an
    /// existing name was replaced.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> bool {
        let name = tool.name().to_string();
        let is_new = !self.tools.contains_key(&name);
        if is_new {
            self.order.push(name.clone());
        }
        self.tools.insert(name, Arc::from(tool));
        is_new
    }

    /// Register a tool that is already wrapped in an Arc.
    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) -> bool {
        let name = tool.name().to_string();
        let is_new = !self.tools.contains_key(&name);
        if is_new {
            self.order.push(name.clone());
        }
        self.tools.insert(name, tool);
        is_new
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Get a cloned Arc reference to a tool by name.
    pub fn get_arc(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<&dyn Tool> {
        self.order
            .iter()
            .filter_map(|name| self.tools.get(name).map(|tool| tool.as_ref()))
            .collect()
    }

    /// Get all tools as Arc references.
    pub fn list_arc(&self) -> Vec<Arc<dyn Tool>> {
        self.order
            .iter()
            .filter_map(|name| self.tools.get(name).cloned())
            .collect()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let ctx = ToolSchemaContext::empty();
        self.order
            .iter()
            .filter_map(|name| {
                self.tools
                    .get(name)
                    .map(|tool| crate::tool::tool_definition(tool.as_ref(), &ctx))
            })
            .collect()
    }

    pub fn remove(&mut self, name: &str) -> Option<Arc<dyn Tool>> {
        let removed = self.tools.remove(name);
        if removed.is_some() {
            self.order.retain(|registered| registered != name);
        }
        removed
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// tool_definition — build the LLM function-calling definition for a tool
// ---------------------------------------------------------------------------

/// Build a [`ToolDefinition`] from a tool's self-description. Replaces the
/// old `Tool::definition` convenience method so the trait stays seven
/// methods; providers and the run loop pass the `ToolSchemaContext` they
/// already hold, everyone else passes [`ToolSchemaContext::empty`].
pub fn tool_definition(tool: &dyn Tool, ctx: &ToolSchemaContext) -> ToolDefinition {
    ToolDefinition {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        parameters: tool.parameters_schema(ctx),
    }
}
