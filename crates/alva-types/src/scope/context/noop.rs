// POS: No-op implementations — minimal defaults for when no context plugin is loaded.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::AgentMessage;

use super::{
    BudgetInfo, ContextEntry, ContextHandle, ContextHooks, ContextLayer, ContextSnapshot,
    ContextSystem, MemoryFact, MessageRange, Priority, ToolPattern,
};

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
