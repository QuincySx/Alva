//! Context management types — ContextEntry, ContextLayer, Priority, and all action/decision enums.

use std::collections::HashMap;

use alva_types::AgentMessage;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core enums
// ---------------------------------------------------------------------------

/// Which layer of the five-layer context model an entry belongs to.
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

/// Source discriminator for memory injection (covers both memory facts and RAG results).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemorySource {
    /// Long-term memory fact.
    Fact,
    /// RAG/vector retrieval result.
    Retrieval { query: String },
}

// ---------------------------------------------------------------------------
// Media types
// ---------------------------------------------------------------------------

/// Where a multi-modal content block came from.
#[derive(Debug, Clone)]
pub enum MediaSource {
    UserMessage { message_id: String },
    ToolResult { tool_name: String, message_id: String },
    FileAttachment { file_path: String },
}

/// What to do with a multi-modal content block.
#[derive(Debug, Clone)]
pub enum MediaAction {
    /// Keep the media in context as-is.
    Keep,
    /// Replace with a text description (e.g. from a vision tool).
    Describe { description: String },
    /// Write to file, leave a reference in context.
    Externalize { path: String },
    /// Remove entirely.
    Remove,
}

// ---------------------------------------------------------------------------
// Retrieval chunk
// ---------------------------------------------------------------------------

/// A chunk returned by RAG/vector search.
#[derive(Debug, Clone)]
pub struct RetrievalChunk {
    pub source: String,
    pub text: String,
    pub score: f64,
    pub metadata: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Action / Decision enums (returned by plugin hooks)
// ---------------------------------------------------------------------------

/// Generic three-state decision for injection hooks.
#[derive(Debug, Clone)]
pub enum InjectDecision<T> {
    /// Allow the content to enter context as-is.
    Allow(T),
    /// Modify the content before injection.
    Modify(T),
    /// Reject — do not inject, with reason.
    Reject { reason: String },
    /// Replace with a summary.
    Summarize { summary: String },
}

/// What to do when ingesting a new message into the store.
#[derive(Debug, Clone)]
pub enum IngestAction {
    Keep,
    Modify(AgentMessage),
    Skip,
    TagAndKeep { priority: Priority },
}

/// Tool call evaluation result.
#[derive(Debug, Clone)]
pub enum ToolCallAction {
    Allow,
    Block { reason: String },
    AllowWithWarning { warning: String },
}

/// How to handle a tool result before it enters context.
#[derive(Debug, Clone)]
pub enum ToolResultAction {
    Keep,
    Replace { summary: String },
    Externalize { path: String },
    Truncate { max_lines: usize },
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

/// How sub-agent results should flow back to the parent.
#[derive(Debug, Clone)]
pub enum InjectionPlan {
    FullResult,
    Summary { text: String },
    Externalized { path: String, summary: String },
    Error { message: String },
}

/// Directive from parent to running sub-agent.
#[derive(Debug, Clone)]
pub enum SubAgentDirective {
    Continue,
    Steer { guidance: String },
    Terminate { reason: String },
}

// ---------------------------------------------------------------------------
// Injection (what on_user_message returns)
// ---------------------------------------------------------------------------

/// Content to inject into the context as a result of a hook decision.
#[derive(Debug, Clone)]
pub enum Injection {
    Memory(Vec<MemoryFact>),
    Skill { name: String, content: String },
    Message(AgentMessage),
    RuntimeContext(String),
}

// ---------------------------------------------------------------------------
// Snapshot types (read-only views for plugin decision-making)
// ---------------------------------------------------------------------------

/// Read-only snapshot of the context store, passed to the plugin for decisions.
#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub total_tokens: usize,
    pub budget_tokens: usize,
    pub model_window: usize,
    pub usage_ratio: f32,
    pub layer_breakdown: HashMap<ContextLayer, LayerStats>,
    pub entries: Vec<EntrySnapshot>,
    pub recent_tool_patterns: Vec<ToolPattern>,
}

#[derive(Debug, Clone)]
pub struct LayerStats {
    pub token_count: usize,
    pub entry_count: usize,
    pub percentage: f32,
}

/// Lightweight view of a single entry (no full content, just metadata + preview).
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
