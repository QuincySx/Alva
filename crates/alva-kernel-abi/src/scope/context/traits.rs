// POS: Context trait definitions — ContextHooks and ContextHandle.

use async_trait::async_trait;

use crate::AgentMessage;

use super::{
    BudgetInfo, CompressAction, ContextEntry, ContextError, ContextLayer, ContextSnapshot,
    IngestAction, Injection, MemoryFact, MessageRange, Priority, ToolPattern,
};

// ===========================================================================
// Trait: ContextHooks
// ===========================================================================

/// The ContextHooks trait (7 lifecycle hooks + name()) that plugins implement to control the context lifecycle.
///
/// All methods have default no-op implementations. Plugins only override what they need.
#[async_trait]
pub trait ContextHooks: Send + Sync {
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    async fn bootstrap(&self, sdk: &dyn ContextHandle, agent_id: &str) -> Result<(), ContextError> {
        let _ = (sdk, agent_id);
        Ok(())
    }

    async fn on_message(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
        message: &AgentMessage,
    ) -> Vec<Injection> {
        let _ = (sdk, agent_id, message);
        vec![]
    }

    async fn on_budget_exceeded(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        let _ = (sdk, agent_id, snapshot);
        vec![CompressAction::SlidingWindow { keep_recent: 20 }]
    }

    async fn assemble(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
        entries: Vec<ContextEntry>,
        token_budget: usize,
    ) -> Vec<ContextEntry> {
        let _ = (sdk, agent_id, token_budget);
        entries
    }

    async fn ingest(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
        entry: &ContextEntry,
    ) -> IngestAction {
        let _ = (sdk, agent_id, entry);
        IngestAction::Keep
    }

    async fn after_turn(&self, sdk: &dyn ContextHandle, agent_id: &str) {
        let _ = (sdk, agent_id);
    }

    async fn dispose(&self) -> Result<(), ContextError> {
        Ok(())
    }
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

// Durable persistence deliberately lives in crate::agent_session::AgentSession.
