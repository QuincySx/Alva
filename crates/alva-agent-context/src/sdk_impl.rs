// INPUT:  std::sync::{Arc, Mutex}, async_trait, alva_types::AgentMessage, uuid, crate::message_store::MessageStore, crate::sdk::ContextHandle, crate::store::{ContextStore, estimate_tokens}, crate::types
// OUTPUT: pub struct ContextHandleImpl
// POS:    Concrete ContextHandle implementation backed by a shared ContextStore with Mutex-based synchronization.
//! Concrete implementation of ContextHandle backed by ContextStore.

use std::sync::{Arc, Mutex};
use async_trait::async_trait;

use alva_types::AgentMessage;

use crate::sdk::ContextHandle;
use crate::store::{ContextStore, estimate_tokens};
use crate::types::*;

/// Optional memory backend — allows the context handle to delegate
/// memory operations without depending on `alva-agent-memory` directly.
///
/// Inject via `ContextHandleImpl::with_memory()`.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    fn query(&self, query: &str, max_results: usize) -> Vec<MemoryFact>;
    fn store(&self, fact: MemoryFact);
    fn delete(&self, fact_id: &str);
}

/// Optional summarization backend — allows plugging in an LLM or
/// heuristic summarizer without coupling to a specific model provider.
///
/// Inject via `ContextHandleImpl::with_summarizer()`.
pub type SummarizeFn =
    Arc<dyn Fn(&[AgentMessage], &[String]) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>> + Send + Sync>;

/// Concrete SDK implementation wrapping a shared ContextStore.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) because all ContextStore
/// operations are pure in-memory work with no I/O or `.await` points while the
/// lock is held. This avoids `blocking_lock()` panics on single-threaded tokio
/// runtimes and eliminates any deadlock risk from calling SDK methods inside
/// async plugin hooks.
pub struct ContextHandleImpl {
    store: Arc<Mutex<ContextStore>>,
    memory: Option<Arc<dyn MemoryBackend>>,
    summarizer: Option<SummarizeFn>,
    bus: Option<alva_types::BusHandle>,
}

impl ContextHandleImpl {
    pub fn new(store: Arc<Mutex<ContextStore>>) -> Self {
        Self {
            store,
            memory: None,
            summarizer: None,
            bus: None,
        }
    }

    /// Attach a memory backend for query/store/delete operations.
    pub fn with_memory(mut self, memory: Arc<dyn MemoryBackend>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach a summarization function for the `summarize()` method.
    pub fn with_summarizer(mut self, summarizer: SummarizeFn) -> Self {
        self.summarizer = Some(summarizer);
        self
    }

    /// Attach a bus handle for bus-aware token counting.
    pub fn with_bus(mut self, bus: alva_types::BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Access the underlying store directly (for middleware adapter).
    pub fn store(&self) -> &Arc<Mutex<ContextStore>> {
        &self.store
    }

    /// Count tokens using bus TokenCounter if available, fallback to chars/4.
    fn count_tokens(&self, text: &str) -> usize {
        if let Some(ref bus) = self.bus {
            if let Some(counter) = bus.get::<dyn alva_types::TokenCounter>() {
                return counter.count_tokens(text);
            }
        }
        estimate_tokens(text)
    }
}

#[async_trait]
impl ContextHandle for ContextHandleImpl {
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
            AgentMessage::Standard(m) => self.count_tokens(&m.text_content()),
            AgentMessage::Custom { data, .. } => self.count_tokens(&data.to_string()),
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
        match &self.memory {
            Some(backend) => {
                let facts = backend.query(query, 20);
                // Trim to fit within token budget
                let mut result = Vec::new();
                let mut tokens_used = 0usize;
                for fact in facts {
                    let fact_tokens = self.count_tokens(&fact.text);
                    if tokens_used + fact_tokens > max_tokens {
                        break;
                    }
                    tokens_used += fact_tokens;
                    result.push(fact);
                }
                result
            }
            None => vec![],
        }
    }

    fn inject_from_file(&self, _agent_id: &str, path: &str, lines: Option<(usize, usize)>) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path, error = %e, "inject_from_file: failed to read");
                return;
            }
        };

        // Extract requested line range
        let text = match lines {
            Some((start, end)) => content
                .lines()
                .skip(start.saturating_sub(1))
                .take(end.saturating_sub(start.saturating_sub(1)))
                .collect::<Vec<_>>()
                .join("\n"),
            None => content,
        };

        let tokens = self.count_tokens(&text);
        let entry = ContextEntry {
            id: uuid::Uuid::new_v4().to_string(),
            message: AgentMessage::Standard(alva_types::Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: alva_types::MessageRole::System,
                content: vec![alva_types::ContentBlock::Text {
                    text: format!("<file path=\"{}\">\n{}\n</file>", path, text),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            }),
            metadata: ContextMetadata::new(ContextLayer::RuntimeInject)
                .with_tokens(tokens)
                .with_origin(EntryOrigin::Plugin {
                    plugin_name: "sdk:inject_from_file".to_string(),
                }),
        };
        self.store
            .lock()
            .expect("ContextStore mutex poisoned")
            .append(entry);
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
        let store = self.store.lock().expect("ContextStore mutex poisoned");
        let entries = store.entries();
        let len = entries.len();
        let (from, to) = {
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
        };
        drop(store);

        // Serialize entries to file
        let store = self.store.lock().expect("ContextStore mutex poisoned");
        let to_externalize: Vec<&ContextEntry> = store.entries()[from..to].iter().collect();
        let json = serde_json::to_string_pretty(&to_externalize.iter().map(|e| {
            serde_json::json!({
                "id": e.id,
                "message": e.message,
                "layer": format!("{:?}", e.metadata.layer),
            })
        }).collect::<Vec<_>>()).unwrap_or_default();
        drop(store);

        if let Err(e) = std::fs::write(path, &json) {
            tracing::warn!(path, error = %e, "externalize: failed to write");
            return;
        }

        // Remove externalized entries and append a reference placeholder
        let mut store = self.store.lock().expect("ContextStore mutex poisoned");
        let count = to - from;
        store.remove_range(from, to);
        let placeholder = ContextEntry {
            id: uuid::Uuid::new_v4().to_string(),
            message: AgentMessage::Standard(alva_types::Message::system(
                &format!("[Externalized {} entries to {}]", count, path)
            )),
            metadata: ContextMetadata::new(ContextLayer::RuntimeInject)
                .with_tokens(10)
                .with_origin(EntryOrigin::System),
        };
        store.append(placeholder);
        tracing::debug!(path, count, "externalized entries");
    }

    async fn summarize(
        &self,
        _agent_id: &str,
        range: MessageRange,
        hints: &[String],
    ) -> String {
        // Collect messages in the range — lock scope is limited to this block
        let messages: Vec<AgentMessage> = {
            let store = self.store.lock().expect("ContextStore mutex poisoned");
            let entries = store.entries();
            let (from, to) = resolve_range(&store, &range);

            entries[from..to]
                .iter()
                .map(|e| e.message.clone())
                .collect()
        }; // MutexGuard dropped here, before any .await

        // Use plugged-in summarizer if available
        if let Some(summarizer) = &self.summarizer {
            return summarizer(&messages, hints).await;
        }

        // Fallback: truncated concatenation (no LLM)
        let mut text = String::new();
        for msg in &messages {
            match msg {
                AgentMessage::Standard(m) => {
                    let content = m.text_content();
                    if content.len() > 200 {
                        text.push_str(&content[..200]);
                        text.push_str("...");
                    } else {
                        text.push_str(&content);
                    }
                    text.push('\n');
                }
                AgentMessage::Custom { type_name, .. } => {
                    text.push_str(&format!("[custom: {}]\n", type_name));
                }
            }
        }

        if !hints.is_empty() {
            text.push_str(&format!("\nHints: {}", hints.join(", ")));
        }

        format!("[Summary of {} messages]\n{}", messages.len(), text.trim())
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

    fn query_memory(&self, query: &str, max_results: usize) -> Vec<MemoryFact> {
        match &self.memory {
            Some(backend) => backend.query(query, max_results),
            None => vec![],
        }
    }

    fn store_memory(&self, fact: MemoryFact) {
        match &self.memory {
            Some(backend) => backend.store(fact),
            None => {
                tracing::debug!(fact_id = fact.id, "store_memory: no memory backend configured");
            }
        }
    }

    fn delete_memory(&self, fact_id: &str) {
        match &self.memory {
            Some(backend) => backend.delete(fact_id),
            None => {
                tracing::debug!(fact_id, "delete_memory: no memory backend configured");
            }
        }
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
