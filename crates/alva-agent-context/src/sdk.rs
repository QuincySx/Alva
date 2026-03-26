//! ContextManagementSDK trait — the privileged interface that plugins use to operate on context.

use alva_types::AgentMessage;
use async_trait::async_trait;

use crate::types::*;

/// The SDK interface that plugins call to read/write the context store.
///
/// Implemented by the framework. Plugins receive `&dyn ContextManagementSDK` in every hook.
#[async_trait]
pub trait ContextManagementSDK: Send + Sync {
    // =====================================================================
    // Read operations
    // =====================================================================

    /// Snapshot of the context store (metadata only, no full content).
    fn snapshot(&self, agent_id: &str) -> ContextSnapshot;

    /// Token budget and usage info.
    fn budget(&self, agent_id: &str) -> BudgetInfo;

    /// Read a single message by ID.
    fn read_message(&self, agent_id: &str, message_id: &str) -> Option<ContextEntry>;

    /// Analyze recent tool call patterns.
    fn tool_patterns(&self, agent_id: &str, last_n: usize) -> Vec<ToolPattern>;

    // =====================================================================
    // Inject operations (add content to context)
    // =====================================================================

    /// Insert a message into a specific layer.
    fn inject_message(&self, agent_id: &str, layer: ContextLayer, message: AgentMessage);

    /// Query memory and inject matching facts.
    fn inject_memory(&self, agent_id: &str, query: &str, max_tokens: usize) -> Vec<MemoryFact>;

    /// Re-read content from an externalized file back into context.
    fn inject_from_file(&self, agent_id: &str, path: &str, lines: Option<(usize, usize)>);

    // =====================================================================
    // Direct write operations (mutate existing context)
    // =====================================================================

    /// Remove a single message by ID.
    fn remove_message(&self, agent_id: &str, message_id: &str);

    /// Remove all messages in a range.
    fn remove_range(&self, agent_id: &str, range: &MessageRange);

    /// Rewrite a single message's content (preserves ID and metadata).
    fn rewrite_message(&self, agent_id: &str, message_id: &str, new_content: AgentMessage);

    /// Batch rewrite multiple messages. Vec of (message_id, new_content).
    fn rewrite_batch(&self, agent_id: &str, rewrites: Vec<(String, AgentMessage)>);

    /// Clear all entries in a specific layer.
    fn clear_layer(&self, agent_id: &str, layer: ContextLayer);

    /// Clear all conversation messages (keeps L0 always-present layer).
    fn clear_conversation(&self, agent_id: &str);

    /// Nuclear: clear everything including L0. Reset to empty.
    fn clear_all(&self, agent_id: &str);

    // =====================================================================
    // Compression shortcuts
    // =====================================================================

    /// Keep only the most recent N conversation messages. L0/L1 are untouched.
    fn sliding_window(&self, agent_id: &str, keep_recent: usize);

    /// Replace a tool result message with a summary string.
    fn replace_tool_result(&self, agent_id: &str, message_id: &str, summary: &str);

    /// Externalize messages to a file, leaving a reference in context.
    fn externalize(&self, agent_id: &str, range: MessageRange, path: &str);

    /// Request LLM-based summarization of a message range.
    async fn summarize(
        &self,
        agent_id: &str,
        range: MessageRange,
        hints: &[String],
    ) -> String;

    // =====================================================================
    // Metadata operations
    // =====================================================================

    /// Set the retention priority of a message.
    fn tag_priority(&self, agent_id: &str, message_id: &str, priority: Priority);

    /// Mark a message for removal at the next compression pass.
    fn tag_exclude(&self, agent_id: &str, message_id: &str);

    // =====================================================================
    // Memory operations (cross-session)
    // =====================================================================

    /// Query the memory store.
    fn query_memory(&self, query: &str, max_results: usize) -> Vec<MemoryFact>;

    /// Store a new memory fact.
    fn store_memory(&self, fact: MemoryFact);

    /// Delete a memory fact by ID.
    fn delete_memory(&self, fact_id: &str);

    // =====================================================================
    // Sub-agent context
    // =====================================================================

    /// Extract context entries relevant to a task description, within a token budget.
    fn extract_relevant(
        &self,
        agent_id: &str,
        task_description: &str,
        max_tokens: usize,
    ) -> Vec<ContextEntry>;
}
