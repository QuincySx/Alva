// INPUT:  std::sync::{Arc, Mutex}, async_trait, alva_types::AgentMessage, uuid, crate::message_store::MessageStore, crate::sdk::ContextHooksSDK, crate::store::{ContextStore, estimate_tokens}, crate::types
// OUTPUT: pub struct ContextSDKImpl
// POS:    Concrete ContextHooksSDK implementation backed by a shared ContextStore with Mutex-based synchronization.
//! Concrete implementation of ContextHooksSDK backed by ContextStore.

use std::sync::{Arc, Mutex};
use async_trait::async_trait;

use alva_types::AgentMessage;

use crate::message_store::MessageStore;
use crate::sdk::ContextHooksSDK;
use crate::store::{ContextStore, estimate_tokens};
use crate::types::*;

/// Concrete SDK implementation wrapping a shared ContextStore.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) because all ContextStore
/// operations are pure in-memory work with no I/O or `.await` points while the
/// lock is held. This avoids `blocking_lock()` panics on single-threaded tokio
/// runtimes and eliminates any deadlock risk from calling SDK methods inside
/// async plugin hooks.
pub struct ContextSDKImpl {
    store: Arc<Mutex<ContextStore>>,
    message_store: Option<Arc<dyn MessageStore>>,
}

impl ContextSDKImpl {
    pub fn new(store: Arc<Mutex<ContextStore>>) -> Self {
        Self {
            store,
            message_store: None,
        }
    }

    /// Attach a `MessageStore` for turn-based persistence.
    pub fn with_message_store(mut self, ms: Arc<dyn MessageStore>) -> Self {
        self.message_store = Some(ms);
        self
    }

    /// Access the underlying store directly (for middleware adapter).
    pub fn store(&self) -> &Arc<Mutex<ContextStore>> {
        &self.store
    }
}

#[async_trait]
impl ContextHooksSDK for ContextSDKImpl {
    // =====================================================================
    // Read
    // =====================================================================

    fn snapshot(&self, _agent_id: &str) -> ContextSnapshot {
        let store = self.store.lock().expect("ContextStore mutex poisoned");
        store.snapshot()
    }

    fn budget(&self, _agent_id: &str) -> BudgetInfo {
        let store = self.store.lock().expect("ContextStore mutex poisoned");
        store.budget_info()
    }

    fn read_message(&self, _agent_id: &str, message_id: &str) -> Option<ContextEntry> {
        let store = self.store.lock().expect("ContextStore mutex poisoned");
        store.get_entry(message_id).cloned()
    }

    fn tool_patterns(&self, _agent_id: &str, last_n: usize) -> Vec<ToolPattern> {
        let store = self.store.lock().expect("ContextStore mutex poisoned");
        store.get_tool_patterns(last_n)
    }

    // =====================================================================
    // Inject
    // =====================================================================

    fn inject_message(&self, _agent_id: &str, layer: ContextLayer, message: AgentMessage) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        let tokens = match &message {
            AgentMessage::Standard(m) => estimate_tokens(&m.text_content()),
            AgentMessage::Custom { data, .. } => estimate_tokens(&data.to_string()),
        };
        let entry = ContextEntry {
            id: uuid::Uuid::new_v4().to_string(),
            message,
            metadata: ContextMetadata::new(layer)
                .with_tokens(tokens)
                .with_origin(EntryOrigin::Plugin {
                    plugin_name: "sdk".to_string(),
                }),
        };
        store.append(entry);
    }

    fn inject_memory(&self, _agent_id: &str, query: &str, max_tokens: usize) -> Vec<MemoryFact> {
        // TODO: integrate with alva-agent-memory for real search
        let _ = (query, max_tokens);
        vec![]
    }

    fn inject_from_file(&self, _agent_id: &str, path: &str, lines: Option<(usize, usize)>) {
        // TODO: read file content and inject as RuntimeInject entry
        let _ = (path, lines);
        tracing::debug!(path, "inject_from_file: not yet implemented");
    }

    // =====================================================================
    // Direct write
    // =====================================================================

    fn remove_message(&self, _agent_id: &str, message_id: &str) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.remove_message(message_id);
    }

    fn remove_range(&self, _agent_id: &str, range: &MessageRange) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        let (from, to) = resolve_range(&store, range);
        store.remove_range(from, to);
    }

    fn rewrite_message(&self, _agent_id: &str, message_id: &str, new_content: AgentMessage) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.rewrite_message(message_id, new_content);
    }

    fn rewrite_batch(&self, _agent_id: &str, rewrites: Vec<(String, AgentMessage)>) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.rewrite_batch(rewrites);
    }

    fn clear_layer(&self, _agent_id: &str, layer: ContextLayer) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.clear_layer(layer);
    }

    fn clear_conversation(&self, _agent_id: &str) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.clear_conversation();
    }

    fn clear_all(&self, _agent_id: &str) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.clear_all();
    }

    // =====================================================================
    // Compression shortcuts
    // =====================================================================

    fn sliding_window(&self, _agent_id: &str, keep_recent: usize) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.sliding_window(keep_recent);
    }

    fn replace_tool_result(&self, _agent_id: &str, message_id: &str, summary: &str) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.replace_tool_result(message_id, summary);
    }

    fn externalize(&self, _agent_id: &str, range: MessageRange, path: &str) {
        // TODO: write entries to file, replace with reference
        let _ = (range, path);
        tracing::debug!(path, "externalize: not yet implemented");
    }

    async fn summarize(
        &self,
        _agent_id: &str,
        _range: MessageRange,
        _hints: &[String],
    ) -> String {
        // TODO: call LLM for summarization
        "[summary placeholder]".to_string()
    }

    // =====================================================================
    // Metadata
    // =====================================================================

    fn tag_priority(&self, _agent_id: &str, message_id: &str, priority: Priority) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.tag_priority(message_id, priority);
    }

    fn tag_exclude(&self, _agent_id: &str, message_id: &str) {
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        store.tag_exclude(message_id);
    }

    // =====================================================================
    // Memory
    // =====================================================================

    fn query_memory(&self, _query: &str, _max_results: usize) -> Vec<MemoryFact> {
        // TODO: integrate with alva-agent-memory
        vec![]
    }

    fn store_memory(&self, fact: MemoryFact) {
        // TODO: integrate with alva-agent-memory
        tracing::debug!(fact_id = fact.id, "store_memory: recorded (not yet persisted)");
    }

    fn delete_memory(&self, fact_id: &str) {
        // TODO: integrate with alva-agent-memory
        tracing::debug!(fact_id, "delete_memory: not yet implemented");
    }

}

/// Resolve a MessageRange to (from_index, to_index) on the store.
fn resolve_range(store: &ContextStore, range: &MessageRange) -> (usize, usize) {
    let entries = store.entries();
    let len = entries.len();

    let from = match &range.from {
        MessageSelector::FromStart => 0,
        MessageSelector::ByIndex(i) => *i,
        MessageSelector::ById(id) => entries.iter().position(|e| e.id == *id).unwrap_or(0),
        MessageSelector::ToEnd => 0,
    };

    let to = match &range.to {
        MessageSelector::ToEnd => len,
        MessageSelector::ByIndex(i) => *i,
        MessageSelector::ById(id) => entries
            .iter()
            .position(|e| e.id == *id)
            .map(|i| i + 1)
            .unwrap_or(len),
        MessageSelector::FromStart => len,
    };

    (from.min(len), to.min(len))
}
