// INPUT:  alva_types types (AgentMessage, ContentBlock, Message, MessageRole), async_trait, serde, serde_json, thiserror, chrono, tokio, tracing, uuid, std
// OUTPUT: ContextHooks, ContextHandle, SessionAccess, ContextSystem, value types, apply helpers
// POS:    Shared context management vocabulary — traits, value types, container, and pure-transform helpers.
//! Context management shared vocabulary.
//!
//! This module provides the **trait definitions**, **value types**, the **ContextSystem**
//! container, and pure **apply** helpers that multiple crates need. Concrete implementations
//! (e.g., `DefaultContextHooks`, `RulesContextHooks`, `ContextHooksChain`,
//! `ContextHandleImpl`, `ContextStore`, `InMemorySession`) live in `alva-agent-context`.
//!
//! By placing traits and types here in `alva-types`, the core agent crate can depend on
//! types alone, making the full context system an optional plugin.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::AgentMessage;

// ===========================================================================
// Error
// ===========================================================================

/// Error type for context plugin operations.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Other(String),
}

// ===========================================================================
// Value types
// ===========================================================================

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
// ContextEntry
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
    /// Skip -- do not add to context.
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

/// Content type for injection requests.
#[derive(Debug, Clone)]
pub enum InjectionContent {
    /// L0: system prompt section.
    SystemPrompt(PromptSection),
    /// L1: skill / domain knowledge.
    Skill { name: String, content: String },
    /// L2: conversation message or tool result.
    Message(AgentMessage),
    /// L2: runtime metadata.
    RuntimeContext(String),
    /// L3: memory facts.
    Memory(Vec<MemoryFact>),
}

/// Injection request -- plugin returns these from `on_message`.
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

// ===========================================================================
// Session types
// ===========================================================================

/// A single event in the session log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Unique identifier for this event.
    pub uuid: String,
    /// Parent event (e.g., tool_result points to tool_use).
    pub parent_uuid: Option<String>,
    /// Event type: "user", "assistant", "system", "progress", etc.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Timestamp (epoch millis).
    pub timestamp: i64,
    /// Conversation message (present for user/assistant events).
    pub message: Option<SessionMessage>,
    /// Arbitrary payload (present for progress/system events).
    pub data: Option<serde_json::Value>,
}

/// A conversation message within a session event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// "user", "assistant", "tool"
    pub role: String,
    /// Message content -- string or content blocks array.
    pub content: serde_json::Value,
}

impl SessionEvent {
    /// Create a user message event.
    pub fn user_message(content: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "user".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some(SessionMessage {
                role: "user".to_string(),
                content,
            }),
            data: None,
        }
    }

    /// Create an assistant message event.
    pub fn assistant_message(content: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "assistant".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some(SessionMessage {
                role: "assistant".to_string(),
                content,
            }),
            data: None,
        }
    }

    /// Create a tool result event linked to a parent tool_use.
    pub fn tool_result(parent_tool_use_uuid: &str, content: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: Some(parent_tool_use_uuid.to_string()),
            event_type: "tool_result".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some(SessionMessage {
                role: "tool".to_string(),
                content,
            }),
            data: None,
        }
    }

    /// Create a progress event (tool execution status, hook triggers, etc.)
    pub fn progress(data: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "progress".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: None,
            data: Some(data),
        }
    }

    /// Create a system event.
    pub fn system(data: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "system".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: None,
            data: Some(data),
        }
    }
}

/// Filter criteria for querying session events.
/// All fields are optional -- None means "don't filter on this".
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    /// Filter by event type ("user", "assistant", "progress", etc.)
    pub event_type: Option<String>,
    /// Filter by message role ("user", "assistant", "tool")
    pub role: Option<String>,
    /// Text search in message content
    pub text_contains: Option<String>,
    /// Only events after this uuid (cursor-based pagination)
    pub after_uuid: Option<String>,
    /// Only the last N matching events
    pub last_n: Option<usize>,
    /// Maximum results to return
    pub limit: usize,
}

/// A query result with preview text.
#[derive(Debug, Clone)]
pub struct EventMatch {
    pub event: SessionEvent,
    /// Truncated preview of the content (for display).
    pub preview: String,
}

// ===========================================================================
// Trait: ContextHooks
// ===========================================================================

/// The 8-hook ContextHooks trait that plugins implement to control the context lifecycle.
///
/// All methods have default no-op implementations. Plugins only override what they need.
#[async_trait]
pub trait ContextHooks: Send + Sync {
    fn name(&self) -> &str { std::any::type_name::<Self>() }

    async fn bootstrap(&self, sdk: &dyn ContextHandle, agent_id: &str) -> Result<(), ContextError> {
        let _ = (sdk, agent_id); Ok(())
    }

    async fn on_message(&self, sdk: &dyn ContextHandle, agent_id: &str, message: &AgentMessage) -> Vec<Injection> {
        let _ = (sdk, agent_id, message); vec![]
    }

    async fn on_budget_exceeded(&self, sdk: &dyn ContextHandle, agent_id: &str, snapshot: &ContextSnapshot) -> Vec<CompressAction> {
        let _ = (sdk, agent_id, snapshot); vec![CompressAction::SlidingWindow { keep_recent: 20 }]
    }

    async fn assemble(&self, sdk: &dyn ContextHandle, agent_id: &str, entries: Vec<ContextEntry>, token_budget: usize) -> Vec<ContextEntry> {
        let _ = (sdk, agent_id, token_budget); entries
    }

    async fn ingest(&self, sdk: &dyn ContextHandle, agent_id: &str, entry: &ContextEntry) -> IngestAction {
        let _ = (sdk, agent_id, entry); IngestAction::Keep
    }

    async fn after_turn(&self, sdk: &dyn ContextHandle, agent_id: &str) {
        let _ = (sdk, agent_id);
    }

    async fn dispose(&self) -> Result<(), ContextError> { Ok(()) }
}

// ===========================================================================
// Trait: ContextHandle
// ===========================================================================

/// The SDK interface that plugins call to read/write the context store.
///
/// Implemented by the framework. Plugins receive `&dyn ContextHandle` in every hook.
#[async_trait]
pub trait ContextHandle: Send + Sync {
    // Read operations
    fn snapshot(&self, agent_id: &str) -> ContextSnapshot;
    fn budget(&self, agent_id: &str) -> BudgetInfo;
    fn read_message(&self, agent_id: &str, message_id: &str) -> Option<ContextEntry>;
    fn tool_patterns(&self, agent_id: &str, last_n: usize) -> Vec<ToolPattern>;

    // Inject operations
    fn inject_message(&self, agent_id: &str, layer: ContextLayer, message: AgentMessage);
    fn inject_memory(&self, agent_id: &str, query: &str, max_tokens: usize) -> Vec<MemoryFact>;
    fn inject_from_file(&self, agent_id: &str, path: &str, lines: Option<(usize, usize)>);

    // Direct write operations
    fn remove_message(&self, agent_id: &str, message_id: &str);
    fn remove_range(&self, agent_id: &str, range: &MessageRange);
    fn rewrite_message(&self, agent_id: &str, message_id: &str, new_content: AgentMessage);
    fn rewrite_batch(&self, agent_id: &str, rewrites: Vec<(String, AgentMessage)>);
    fn clear_layer(&self, agent_id: &str, layer: ContextLayer);
    fn clear_conversation(&self, agent_id: &str);
    fn clear_all(&self, agent_id: &str);

    // Compression shortcuts
    fn sliding_window(&self, agent_id: &str, keep_recent: usize);
    fn replace_tool_result(&self, agent_id: &str, message_id: &str, summary: &str);
    fn externalize(&self, agent_id: &str, range: MessageRange, path: &str);
    async fn summarize(&self, agent_id: &str, range: MessageRange, hints: &[String]) -> String;

    // Metadata operations
    fn tag_priority(&self, agent_id: &str, message_id: &str, priority: Priority);
    fn tag_exclude(&self, agent_id: &str, message_id: &str);

    // Memory operations (cross-session)
    fn query_memory(&self, query: &str, max_results: usize) -> Vec<MemoryFact>;
    fn store_memory(&self, fact: MemoryFact);
    fn delete_memory(&self, fact_id: &str);
}

// ===========================================================================
// Trait: SessionAccess
// ===========================================================================

/// The session storage interface.
///
/// Append-only event log with query and rollback support.
/// Implementations: InMemorySession (testing), SQLite (desktop), file (CLI), remote (cloud).
#[async_trait]
pub trait SessionAccess: Send + Sync {
    /// Session identifier.
    fn session_id(&self) -> &str;

    /// Append an event to the log.
    async fn append(&self, event: SessionEvent);

    /// Query events matching the filter. Storage layer does the filtering.
    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch>;

    /// Count events matching the filter (without loading content).
    async fn count(&self, filter: &EventQuery) -> usize;

    /// Rollback: delete all events after the given uuid.
    /// Returns the number of events removed.
    async fn rollback_after(&self, uuid: &str) -> usize;

    /// Save a context snapshot (binary, opaque to storage).
    async fn save_snapshot(&self, data: &[u8]);

    /// Load the last saved context snapshot.
    async fn load_snapshot(&self) -> Option<Vec<u8>>;

    /// Clear all events and snapshots.
    async fn clear(&self);
}

// ===========================================================================
// ContextSystem container
// ===========================================================================

/// Bundles the context management trio: hooks (strategy), handle (operations), session (persistence).
///
/// This prevents the mismatch bug where someone swaps `context_hooks` but forgets
/// to update `context_handle`, or sets a session on the wrong agent.
pub struct ContextSystem {
    hooks: Arc<dyn ContextHooks>,
    handle: Arc<dyn ContextHandle>,
    session: Option<Arc<dyn SessionAccess>>,
}

impl ContextSystem {
    pub fn new(
        hooks: Arc<dyn ContextHooks>,
        handle: Arc<dyn ContextHandle>,
    ) -> Self {
        Self {
            hooks,
            handle,
            session: None,
        }
    }

    pub fn with_session(mut self, session: Arc<dyn SessionAccess>) -> Self {
        self.session = Some(session);
        self
    }

    pub fn hooks(&self) -> &dyn ContextHooks {
        self.hooks.as_ref()
    }

    pub fn handle(&self) -> &dyn ContextHandle {
        self.handle.as_ref()
    }

    pub fn session(&self) -> Option<&dyn SessionAccess> {
        self.session.as_deref()
    }

    /// Replace hooks (e.g., swap in a ContextHooksChain).
    pub fn set_hooks(&mut self, hooks: Arc<dyn ContextHooks>) {
        self.hooks = hooks;
    }

    /// Replace session.
    pub fn set_session(&mut self, session: Arc<dyn SessionAccess>) {
        self.session = Some(session);
    }
}

// ===========================================================================
// No-op implementations — minimal defaults for when no context plugin is loaded
// ===========================================================================

/// A no-op ContextHooks that passes everything through unchanged.
///
/// Used as the default when no context plugin is configured.
pub struct NoopContextHooks;

#[async_trait]
impl ContextHooks for NoopContextHooks {
    fn name(&self) -> &str { "noop" }
}

/// A no-op ContextHandle that returns empty/zero values for all operations.
///
/// Used as the default when no context plugin is configured.
pub struct NoopContextHandle;

#[async_trait]
impl ContextHandle for NoopContextHandle {
    fn snapshot(&self, _agent_id: &str) -> ContextSnapshot {
        ContextSnapshot {
            total_tokens: 0,
            budget_tokens: 0,
            model_window: 0,
            usage_ratio: 0.0,
            layer_breakdown: HashMap::new(),
            entries: Vec::new(),
            recent_tool_patterns: Vec::new(),
        }
    }
    fn budget(&self, _agent_id: &str) -> BudgetInfo {
        // No-op: return zeros so callers don't assume a real budget exists.
        BudgetInfo {
            model_window: 0,
            budget_tokens: 0,
            used_tokens: 0,
            remaining_tokens: 0,
            usage_ratio: 0.0,
        }
    }
    fn read_message(&self, _agent_id: &str, _message_id: &str) -> Option<ContextEntry> { None }
    fn tool_patterns(&self, _agent_id: &str, _last_n: usize) -> Vec<ToolPattern> { vec![] }

    fn inject_message(&self, _agent_id: &str, _layer: ContextLayer, _message: AgentMessage) {}
    fn inject_memory(&self, _agent_id: &str, _query: &str, _max_tokens: usize) -> Vec<MemoryFact> { vec![] }
    fn inject_from_file(&self, _agent_id: &str, _path: &str, _lines: Option<(usize, usize)>) {}

    fn remove_message(&self, _agent_id: &str, _message_id: &str) {}
    fn remove_range(&self, _agent_id: &str, _range: &MessageRange) {}
    fn rewrite_message(&self, _agent_id: &str, _message_id: &str, _new_content: AgentMessage) {}
    fn rewrite_batch(&self, _agent_id: &str, _rewrites: Vec<(String, AgentMessage)>) {}
    fn clear_layer(&self, _agent_id: &str, _layer: ContextLayer) {}
    fn clear_conversation(&self, _agent_id: &str) {}
    fn clear_all(&self, _agent_id: &str) {}

    fn sliding_window(&self, _agent_id: &str, _keep_recent: usize) {}
    fn replace_tool_result(&self, _agent_id: &str, _message_id: &str, _summary: &str) {}
    fn externalize(&self, _agent_id: &str, _range: MessageRange, _path: &str) {}
    async fn summarize(&self, _agent_id: &str, _range: MessageRange, _hints: &[String]) -> String {
        "[no context system configured]".to_string()
    }

    fn tag_priority(&self, _agent_id: &str, _message_id: &str, _priority: Priority) {}
    fn tag_exclude(&self, _agent_id: &str, _message_id: &str) {}

    fn query_memory(&self, _query: &str, _max_results: usize) -> Vec<MemoryFact> { vec![] }
    fn store_memory(&self, _fact: MemoryFact) {}
    fn delete_memory(&self, _fact_id: &str) {}
}

/// Default ContextSystem uses no-op hooks and handle.
///
/// To get a fully functional context system backed by ContextStore and
/// RulesContextHooks, use `alva_agent_context::default_context_system()`.
impl Default for ContextSystem {
    fn default() -> Self {
        Self::new(
            Arc::new(NoopContextHooks),
            Arc::new(NoopContextHandle),
        )
    }
}

// ===========================================================================
// Bus events — emitted by context management for observability and coordination
// ===========================================================================

/// Emitted when token usage exceeds the configured budget threshold.
///
/// Sender: ContextHooks (on_budget_exceeded)
/// Receiver: UI layer, compression middleware, metrics
#[derive(Clone, Debug)]
pub struct TokenBudgetExceeded {
    pub agent_id: String,
    pub usage_ratio: f32,
    pub used_tokens: usize,
    pub budget_tokens: usize,
}
impl alva_agent_bus::BusEvent for TokenBudgetExceeded {}

/// Emitted after context compression is applied.
///
/// Sender: ContextHooks (assemble/on_budget_exceeded)
/// Receiver: UI layer, metrics
#[derive(Clone, Debug)]
pub struct ContextCompacted {
    pub agent_id: String,
    pub strategy: String,
    pub tokens_before: usize,
    pub tokens_after: usize,
}
impl alva_agent_bus::BusEvent for ContextCompacted {}

/// Emitted after memory facts are extracted from conversation.
///
/// Sender: DefaultContextHooks (after_turn)
/// Receiver: UI layer, metrics
#[derive(Clone, Debug)]
pub struct MemoryExtracted {
    pub agent_id: String,
    pub fact_count: usize,
}
impl alva_agent_bus::BusEvent for MemoryExtracted {}

// ===========================================================================
// Apply helpers — pure transformations on value types
// ===========================================================================

// NOTE: The `apply` module (apply_injections, apply_compressions) has been moved
// to `alva-agent-context::apply`. Types crate should not contain runtime logic.
