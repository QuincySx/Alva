// INPUT:  std::collections::HashMap, alva_types::AgentMessage, serde::{Deserialize, Serialize}, chrono
// OUTPUT: pub enum ContextLayer, pub enum Priority, pub struct ContextEntry, pub struct ContextMetadata, pub enum EntryOrigin, pub struct MessageRange, pub enum MessageSelector, pub struct PromptSection, pub struct RuntimeContext, pub struct MemoryFact, pub enum MemoryCategory, pub enum MemorySource, pub enum MediaSource, pub enum MediaAction, pub struct RetrievalChunk, pub enum IngestAction, pub enum ToolCallAction, pub enum ToolResultAction, pub enum CompressAction, pub enum InjectionContent, pub struct Injection, pub struct ContextSnapshot, pub struct LayerStats, pub struct EntrySnapshot, pub struct ToolPattern, pub struct BudgetInfo
// POS:    Central type definitions for the context management system including layers, priorities, entries, actions, and snapshot types.
//! Context management types — ContextEntry, ContextLayer, Priority, and all action/decision enums.

use std::collections::HashMap;

use alva_types::AgentMessage;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core enums
// ---------------------------------------------------------------------------

/// Which layer of the four-layer context model an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextLayer {
    /// L0: Identity, conventions, hard constraints. Always present.
    AlwaysPresent,
    /// L1: Skills, domain knowledge. Loaded on demand.
    OnDemand,
    /// L2: Timestamp, channel, files, media, tool results. Rebuilt each turn.
    RuntimeInject,
    /// L3: Cross-session memory facts. Injected by query.
    Memory,
}

/// Retention priority during compression. Higher = harder to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// Can always be removed.
    Disposable = 0,
    /// Remove if needed.
    Low = 1,
    /// Default.
    Normal = 2,
    /// Keep unless desperate.
    High = 3,
    /// Never remove (user intent, architecture decisions, identifiers).
    Critical = 4,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

// ---------------------------------------------------------------------------
// ContextEntry — a single item in the context store
// ---------------------------------------------------------------------------

/// A context entry = message + management metadata.
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub id: String,
    pub message: AgentMessage,
    pub metadata: ContextMetadata,
}

/// Management metadata attached to each context entry.
#[derive(Debug, Clone)]
pub struct ContextMetadata {
    /// Which layer this entry belongs to.
    pub layer: ContextLayer,
    /// Retention priority (can be dynamically adjusted by plugin).
    pub priority: Priority,
    /// Estimated token count.
    pub estimated_tokens: usize,
    /// Whether this entry has been compacted/replaced.
    pub compacted: bool,
    /// If externalized, the file path.
    pub externalized_path: Option<String>,
    /// If replaced, the replacement summary.
    pub replacement_summary: Option<String>,
    /// Which agent produced this entry.
    pub source_agent: Option<String>,
    /// Provenance: who created this entry.
    pub origin: EntryOrigin,
    /// Creation timestamp (epoch millis).
    pub created_at: i64,
    /// Last time this entry was referenced by subsequent messages.
    pub last_referenced_at: Option<i64>,
}

impl ContextMetadata {
    pub fn new(layer: ContextLayer) -> Self {
        Self {
            layer,
            priority: Priority::default(),
            estimated_tokens: 0,
            compacted: false,
            externalized_path: None,
            replacement_summary: None,
            source_agent: None,
            origin: EntryOrigin::System,
            created_at: chrono::Utc::now().timestamp_millis(),
            last_referenced_at: None,
        }
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_tokens(mut self, tokens: usize) -> Self {
        self.estimated_tokens = tokens;
        self
    }

    pub fn with_origin(mut self, origin: EntryOrigin) -> Self {
        self.origin = origin;
        self
    }
}

/// Who created this context entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryOrigin {
    User,
    Model,
    Tool { tool_name: String },
    Plugin { plugin_name: String },
    SubAgent { agent_id: String },
    System,
}

// ---------------------------------------------------------------------------
// Message range selector
// ---------------------------------------------------------------------------

/// Selects a range of messages in the context store.
#[derive(Debug, Clone)]
pub struct MessageRange {
    pub from: MessageSelector,
    pub to: MessageSelector,
}

#[derive(Debug, Clone)]
pub enum MessageSelector {
    FromStart,
    ToEnd,
    ByIndex(usize),
    ById(String),
}

// ---------------------------------------------------------------------------
// System prompt section
// ---------------------------------------------------------------------------

/// A named section of the system prompt (L0).
#[derive(Debug, Clone)]
pub struct PromptSection {
    /// Unique identifier, e.g. "identity", "conventions", "constraints".
    pub id: String,
    pub content: String,
    pub priority: Priority,
}

// ---------------------------------------------------------------------------
// Runtime context (L2)
// ---------------------------------------------------------------------------

/// Dynamic runtime data injected each turn.
#[derive(Debug, Clone, Default)]
pub struct RuntimeContext {
    pub timestamp: String,
    pub session_metadata: HashMap<String, String>,
    pub user_preferences: HashMap<String, String>,
    pub channel_info: Option<String>,
    pub custom: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Memory fact (L3)
// ---------------------------------------------------------------------------

/// A single memory fact persisted across sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: String,
    pub text: String,
    pub fingerprint: String,
    pub confidence: f32,
    pub category: MemoryCategory,
    pub source_session: String,
    pub created_at: i64,
    pub last_accessed_at: i64,
    pub access_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryCategory {
    UserPreference,
    UserProfile,
    ProjectContext,
    TaskPattern,
    Constraint,
}

// ---------------------------------------------------------------------------
// Action / Decision enums (returned by plugin hooks)
// ---------------------------------------------------------------------------

/// What to do when ingesting a new message into the store.
#[derive(Debug, Clone)]
pub enum IngestAction {
    /// Keep entry as-is.
    Keep,
    /// Skip — do not add to context.
    Skip,
    /// Modify message content and/or override priority.
    Modify {
        message: AgentMessage,
        priority: Option<Priority>,
    },
}

/// Compression actions the plugin can request.
#[derive(Debug, Clone)]
pub enum CompressAction {
    SlidingWindow { keep_recent: usize },
    Summarize { range: MessageRange, hints: Vec<String> },
    ReplaceToolResult { message_id: String, summary: String },
    Externalize { range: MessageRange, path: String },
    RemoveByPriority { priority: Priority },
}

// ---------------------------------------------------------------------------
// Injection (what on_message returns)
// ---------------------------------------------------------------------------

/// 注入请求的内容类型
#[derive(Debug, Clone)]
pub enum InjectionContent {
    /// L0: 系统提示词段落
    SystemPrompt(PromptSection),
    /// L1: 技能/领域知识
    Skill { name: String, content: String },
    /// L2: 对话消息、工具结果
    Message(AgentMessage),
    /// L2: 运行时元数据
    RuntimeContext(String),
    /// L3: 记忆事实
    Memory(Vec<MemoryFact>),
}

/// 注入请求 — plugin 通过 on_message 返回
#[derive(Debug, Clone)]
pub struct Injection {
    pub content: InjectionContent,
    pub layer: ContextLayer,
    pub priority: Option<Priority>,
}

impl Injection {
    pub fn system_prompt(section: PromptSection) -> Self {
        Self {
            content: InjectionContent::SystemPrompt(section),
            layer: ContextLayer::AlwaysPresent,
            priority: None,
        }
    }

    pub fn skill(name: String, content: String) -> Self {
        Self {
            content: InjectionContent::Skill { name, content },
            layer: ContextLayer::OnDemand,
            priority: None,
        }
    }

    pub fn message(msg: AgentMessage) -> Self {
        Self {
            content: InjectionContent::Message(msg),
            layer: ContextLayer::RuntimeInject,
            priority: None,
        }
    }

    pub fn runtime_context(data: String) -> Self {
        Self {
            content: InjectionContent::RuntimeContext(data),
            layer: ContextLayer::RuntimeInject,
            priority: None,
        }
    }

    pub fn memory(facts: Vec<MemoryFact>) -> Self {
        Self {
            content: InjectionContent::Memory(facts),
            layer: ContextLayer::Memory,
            priority: None,
        }
    }

    pub fn with_layer(mut self, layer: ContextLayer) -> Self {
        self.layer = layer;
        self
    }

    pub fn with_priority(mut self, p: Priority) -> Self {
        self.priority = Some(p);
        self
    }
}

// ---------------------------------------------------------------------------
// Snapshot types (read-only views for plugin decision-making)
// ---------------------------------------------------------------------------

/// Read-only snapshot of the context store, passed to the plugin for decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub total_tokens: usize,
    pub budget_tokens: usize,
    pub model_window: usize,
    pub usage_ratio: f32,
    pub layer_breakdown: HashMap<ContextLayer, LayerStats>,
    pub entries: Vec<EntrySnapshot>,
    pub recent_tool_patterns: Vec<ToolPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerStats {
    pub token_count: usize,
    pub entry_count: usize,
    pub percentage: f32,
}

/// Lightweight view of a single entry (no full content, just metadata + preview).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrySnapshot {
    pub id: String,
    pub layer: ContextLayer,
    pub priority: Priority,
    pub estimated_tokens: usize,
    pub origin: EntryOrigin,
    pub age_turns: usize,
    pub last_referenced_turns: Option<usize>,
    pub preview: String,
}

/// Tool call pattern statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPattern {
    pub tool_name: String,
    pub call_count: usize,
    pub avg_result_tokens: usize,
    pub total_result_tokens: usize,
}

/// Token budget information.
#[derive(Debug, Clone)]
pub struct BudgetInfo {
    pub model_window: usize,
    pub budget_tokens: usize,
    pub used_tokens: usize,
    pub remaining_tokens: usize,
    pub usage_ratio: f32,
}
