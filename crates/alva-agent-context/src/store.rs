// INPUT:  std::collections::HashMap, alva_kernel_abi::{AgentMessage, TokenCounter}, crate::types (ContextEntry, ContextLayer, ContextMetadata, Priority, ContextSnapshot, EntrySnapshot, BudgetInfo, ToolPattern, LayerStats)
// OUTPUT: ContextStore, estimate_tokens(), count_tokens_via()
// POS:    Per-agent context container providing four-layer CRUD, token tracking, and count_tokens_via() for bus-based token counting.
//! ContextStore — per-agent context container with four-layer management.

use std::collections::HashMap;

use alva_kernel_abi::AgentMessage;

use crate::types::*;

/// Per-agent context container. Stores entries organized by layer.
pub struct ContextStore {
    entries: Vec<ContextEntry>,
    /// Model context window size in tokens.
    model_window: usize,
    /// Token budget (may be less than model_window).
    budget_tokens: usize,
    /// Current turn index (incremented each turn).
    turn_index: usize,
    /// Tool call pattern tracking.
    tool_patterns: Vec<ToolPatternTracker>,
}

#[derive(Debug, Clone)]
struct ToolPatternTracker {
    tool_name: String,
    result_tokens: Vec<usize>,
}

impl ContextStore {
    pub fn new(model_window: usize, budget_tokens: usize) -> Self {
        Self {
            entries: Vec::new(),
            model_window,
            budget_tokens,
            turn_index: 0,
            tool_patterns: Vec::new(),
        }
    }

    // =====================================================================
    // Read
    // =====================================================================

    pub fn total_tokens(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| !e.metadata.compacted)
            .map(|e| e.metadata.estimated_tokens)
            .sum()
    }

    pub fn usage_ratio(&self) -> f32 {
        if self.model_window == 0 {
            return 0.0;
        }
        self.total_tokens() as f32 / self.model_window as f32
    }

    pub fn layer_breakdown(&self) -> HashMap<ContextLayer, LayerStats> {
        let mut map: HashMap<ContextLayer, (usize, usize)> = HashMap::new();
        let total = self.total_tokens().max(1) as f32;

        for entry in &self.entries {
            if entry.metadata.compacted {
                continue;
            }
            let (tokens, count) = map.entry(entry.metadata.layer).or_insert((0, 0));
            *tokens += entry.metadata.estimated_tokens;
            *count += 1;
        }

        map.into_iter()
            .map(|(layer, (token_count, entry_count))| {
                (
                    layer,
                    LayerStats {
                        token_count,
                        entry_count,
                        percentage: token_count as f32 / total,
                    },
                )
            })
            .collect()
    }

    pub fn get_entry(&self, id: &str) -> Option<&ContextEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn get_entry_mut(&mut self, id: &str) -> Option<&mut ContextEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    pub fn entries(&self) -> &[ContextEntry] {
        &self.entries
    }

    pub fn snapshot(&self) -> ContextSnapshot {
        let total = self.total_tokens();
        ContextSnapshot {
            total_tokens: total,
            budget_tokens: self.budget_tokens,
            model_window: self.model_window,
            usage_ratio: self.usage_ratio(),
            layer_breakdown: self.layer_breakdown(),
            entries: self
                .entries
                .iter()
                .filter(|e| !e.metadata.compacted)
                .map(|e| EntrySnapshot {
                    id: e.id.clone(),
                    layer: e.metadata.layer,
                    priority: e.metadata.priority,
                    estimated_tokens: e.metadata.estimated_tokens,
                    origin: e.metadata.origin.clone(),
                    age_turns: self.turn_index.saturating_sub(0), // TODO: per-entry turn tracking
                    last_referenced_turns: None,
                    preview: preview_message(&e.message, 100),
                })
                .collect(),
            recent_tool_patterns: self.get_tool_patterns(10),
        }
    }

    pub fn budget_info(&self) -> BudgetInfo {
        let used = self.total_tokens();
        BudgetInfo {
            model_window: self.model_window,
            budget_tokens: self.budget_tokens,
            used_tokens: used,
            remaining_tokens: self.budget_tokens.saturating_sub(used),
            usage_ratio: self.usage_ratio(),
        }
    }

    // =====================================================================
    // Write
    // =====================================================================

    pub fn append(&mut self, entry: ContextEntry) {
        self.entries.push(entry);
    }

    pub fn remove_message(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn remove_range(&mut self, from_idx: usize, to_idx: usize) {
        if from_idx < to_idx && to_idx <= self.entries.len() {
            self.entries.drain(from_idx..to_idx);
        }
    }

    pub fn rewrite_message(&mut self, id: &str, new_message: AgentMessage) {
        if let Some(entry) = self.get_entry_mut(id) {
            entry.message = new_message;
        }
    }

    pub fn rewrite_batch(&mut self, rewrites: Vec<(String, AgentMessage)>) {
        for (id, msg) in rewrites {
            self.rewrite_message(&id, msg);
        }
    }

    pub fn clear_layer(&mut self, layer: ContextLayer) {
        self.entries.retain(|e| e.metadata.layer != layer);
    }

    pub fn clear_conversation(&mut self) {
        // Keep L0 (AlwaysPresent), remove everything else
        self.entries
            .retain(|e| e.metadata.layer == ContextLayer::AlwaysPresent);
    }

    pub fn clear_all(&mut self) {
        self.entries.clear();
    }

    // =====================================================================
    // Compression shortcuts
    // =====================================================================

    /// Keep only the most recent `keep` conversation messages. L0/L1 untouched.
    pub fn sliding_window(&mut self, keep: usize) {
        let conversation_entries: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.metadata.layer == ContextLayer::RuntimeInject
                    || e.metadata.layer == ContextLayer::Memory
            })
            .map(|(i, _)| i)
            .collect();

        if conversation_entries.len() <= keep {
            return;
        }

        let to_remove = conversation_entries.len() - keep;
        let remove_indices: Vec<usize> = conversation_entries[..to_remove].to_vec();

        // Remove in reverse to preserve indices
        for &idx in remove_indices.iter().rev() {
            self.entries.remove(idx);
        }
    }

    pub fn replace_tool_result(&mut self, id: &str, summary: &str) {
        if let Some(entry) = self.get_entry_mut(id) {
            entry.metadata.replacement_summary = Some(summary.to_string());
            // NOTE: Do NOT set compacted = true here. The summary should remain
            // in context (visible to LLM via build_llm_messages and counted in
            // total_tokens). `compacted = true` means "fully externalized, do not
            // include in LLM context" — which is wrong for a summary replacement.
            entry.message = AgentMessage::Standard(alva_kernel_abi::Message {
                id: entry.id.clone(),
                role: alva_kernel_abi::MessageRole::Tool,
                content: vec![alva_kernel_abi::ContentBlock::Text {
                    text: summary.to_string(),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            entry.metadata.estimated_tokens = estimate_tokens(summary);
        }
    }

    pub fn tag_priority(&mut self, id: &str, priority: Priority) {
        if let Some(entry) = self.get_entry_mut(id) {
            entry.metadata.priority = priority;
        }
    }

    pub fn tag_exclude(&mut self, id: &str) {
        if let Some(entry) = self.get_entry_mut(id) {
            entry.metadata.priority = Priority::Disposable;
        }
    }

    // =====================================================================
    // Build LLM messages (ordered by layer)
    // =====================================================================

    /// Build the ordered message list for LLM consumption.
    /// Order: L0 → L1 → L2 → L3 → conversation (chronological).
    pub fn build_llm_messages(&self) -> Vec<AgentMessage> {
        let layer_order = [
            ContextLayer::AlwaysPresent,
            ContextLayer::OnDemand,
            ContextLayer::RuntimeInject,
            ContextLayer::Memory,
        ];

        let mut messages = Vec::new();

        for layer in &layer_order {
            for entry in &self.entries {
                if entry.metadata.layer == *layer && !entry.metadata.compacted {
                    messages.push(entry.message.clone());
                }
            }
        }

        messages
    }

    // =====================================================================
    // Tool pattern tracking
    // =====================================================================

    pub fn track_tool_call(&mut self, tool_name: &str, result_tokens: usize) {
        if let Some(tracker) = self
            .tool_patterns
            .iter_mut()
            .find(|t| t.tool_name == tool_name)
        {
            tracker.result_tokens.push(result_tokens);
        } else {
            self.tool_patterns.push(ToolPatternTracker {
                tool_name: tool_name.to_string(),
                result_tokens: vec![result_tokens],
            });
        }
    }

    pub fn get_tool_patterns(&self, last_n: usize) -> Vec<ToolPattern> {
        self.tool_patterns
            .iter()
            .map(|t| {
                let recent: Vec<_> = t
                    .result_tokens
                    .iter()
                    .rev()
                    .take(last_n)
                    .copied()
                    .collect();
                let total: usize = recent.iter().sum();
                let count = recent.len();
                ToolPattern {
                    tool_name: t.tool_name.clone(),
                    call_count: count,
                    avg_result_tokens: if count > 0 { total / count } else { 0 },
                    total_result_tokens: total,
                }
            })
            .collect()
    }

    pub fn increment_turn(&mut self) {
        self.turn_index += 1;
    }

}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Rough token estimation (chars / 4).
/// Default heuristic -- callers with bus access should use `count_tokens_via()` instead.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Count tokens using a real TokenCounter (from bus capability).
pub fn count_tokens_via(text: &str, counter: &dyn alva_kernel_abi::TokenCounter) -> usize {
    counter.count_tokens(text)
}

/// Extract a preview of a message (first N chars).
fn preview_message(msg: &AgentMessage, max_chars: usize) -> String {
    match msg {
        AgentMessage::Standard(m) => {
            let text = m.text_content();
            if text.len() > max_chars {
                format!("{}...", &text[..max_chars])
            } else {
                text
            }
        }
        AgentMessage::Extension { type_name, .. } => {
            format!("[extension: {}]", type_name)
        }
        _ => "[marker/steering/followup]".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{ContentBlock, Message, MessageRole};

    /// Create a ContextStore with a 10_000 token model window and 8_000 budget.
    fn test_store() -> ContextStore {
        ContextStore::new(10_000, 8_000)
    }

    /// Create a ContextEntry with given id, layer, and estimated token count.
    fn test_entry(id: &str, layer: ContextLayer, tokens: usize) -> ContextEntry {
        ContextEntry {
            id: id.to_string(),
            message: AgentMessage::Standard(Message {
                id: id.to_string(),
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: format!("msg-{}", id),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 1000,
            }),
            metadata: ContextMetadata::new(layer).with_tokens(tokens),
        }
    }

    #[test]
    fn test_append_and_get() {
        let mut store = test_store();
        let entry = test_entry("e1", ContextLayer::RuntimeInject, 100);
        store.append(entry);

        let got = store.get_entry("e1");
        assert!(got.is_some());
        assert_eq!(got.unwrap().id, "e1");

        // Non-existent entry returns None.
        assert!(store.get_entry("nope").is_none());
    }

    #[test]
    fn test_total_tokens() {
        let mut store = test_store();
        store.append(test_entry("a", ContextLayer::AlwaysPresent, 100));
        store.append(test_entry("b", ContextLayer::RuntimeInject, 250));
        assert_eq!(store.total_tokens(), 350);

        // Compacted entries are excluded from total_tokens.
        if let Some(e) = store.get_entry_mut("a") {
            e.metadata.compacted = true;
        }
        assert_eq!(store.total_tokens(), 250);
    }

    #[test]
    fn test_usage_ratio() {
        let mut store = test_store(); // model_window = 10_000
        store.append(test_entry("a", ContextLayer::AlwaysPresent, 5000));
        let ratio = store.usage_ratio();
        assert!((ratio - 0.5).abs() < f32::EPSILON);

        // Zero-window edge case.
        let store_zero = ContextStore::new(0, 0);
        assert_eq!(store_zero.usage_ratio(), 0.0);
    }

    #[test]
    fn test_layer_breakdown() {
        let mut store = test_store();
        store.append(test_entry("a", ContextLayer::AlwaysPresent, 200));
        store.append(test_entry("b", ContextLayer::AlwaysPresent, 300));
        store.append(test_entry("c", ContextLayer::RuntimeInject, 500));

        let breakdown = store.layer_breakdown();

        let l0 = breakdown.get(&ContextLayer::AlwaysPresent).unwrap();
        assert_eq!(l0.token_count, 500);
        assert_eq!(l0.entry_count, 2);

        let l2 = breakdown.get(&ContextLayer::RuntimeInject).unwrap();
        assert_eq!(l2.token_count, 500);
        assert_eq!(l2.entry_count, 1);

        // Percentage: each layer is 500 out of 1000 total = 0.5
        assert!((l0.percentage - 0.5).abs() < f32::EPSILON);
        assert!((l2.percentage - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_remove_message() {
        let mut store = test_store();
        store.append(test_entry("a", ContextLayer::AlwaysPresent, 100));
        store.append(test_entry("b", ContextLayer::RuntimeInject, 200));
        assert_eq!(store.entries().len(), 2);

        store.remove_message("a");
        assert_eq!(store.entries().len(), 1);
        assert!(store.get_entry("a").is_none());
        assert!(store.get_entry("b").is_some());

        // Removing non-existent is a no-op.
        store.remove_message("nope");
        assert_eq!(store.entries().len(), 1);
    }

    #[test]
    fn test_sliding_window() {
        let mut store = test_store();
        // L0 entries are not affected by sliding_window.
        store.append(test_entry("l0", ContextLayer::AlwaysPresent, 100));
        // Add RuntimeInject (L2) entries which ARE subject to sliding window.
        for i in 0..5 {
            store.append(test_entry(
                &format!("rt{}", i),
                ContextLayer::RuntimeInject,
                50,
            ));
        }
        // Also add Memory (L3) entries which are also subject to sliding window.
        for i in 0..3 {
            store.append(test_entry(
                &format!("mem{}", i),
                ContextLayer::Memory,
                50,
            ));
        }
        // Total: 1 L0 + 5 L2 + 3 L3 = 9 entries. 8 conversation entries.
        assert_eq!(store.entries().len(), 9);

        // Keep 3 most recent conversation entries (L2+L3).
        store.sliding_window(3);

        // L0 untouched + 3 kept = 4
        assert_eq!(store.entries().len(), 4);
        assert!(store.get_entry("l0").is_some());
        // The oldest 5 conversation entries should be gone.
        assert!(store.get_entry("rt0").is_none());
        assert!(store.get_entry("rt1").is_none());
        assert!(store.get_entry("rt2").is_none());
        assert!(store.get_entry("rt3").is_none());
        assert!(store.get_entry("rt4").is_none());
        // The 3 most recent (mem0, mem1, mem2) should survive.
        assert!(store.get_entry("mem0").is_some());
        assert!(store.get_entry("mem1").is_some());
        assert!(store.get_entry("mem2").is_some());
    }

    #[test]
    fn test_replace_tool_result_keeps_in_context() {
        let mut store = test_store();
        store.append(test_entry("tr1", ContextLayer::RuntimeInject, 5000));

        store.replace_tool_result("tr1", "Summary of tool result.");

        let entry = store.get_entry("tr1").unwrap();
        // Must NOT be compacted — the summary should remain visible to the LLM.
        assert!(!entry.metadata.compacted);
        assert_eq!(
            entry.metadata.replacement_summary,
            Some("Summary of tool result.".to_string())
        );
        // Token count is updated to reflect summary size.
        assert_eq!(
            entry.metadata.estimated_tokens,
            estimate_tokens("Summary of tool result.")
        );
        // The message content is replaced with the summary text.
        if let AgentMessage::Standard(m) = &entry.message {
            assert_eq!(m.text_content(), "Summary of tool result.");
        } else {
            panic!("Expected Standard message");
        }
        // total_tokens counts the summary (entry is not compacted).
        assert_eq!(
            store.total_tokens(),
            estimate_tokens("Summary of tool result.")
        );
    }

    #[test]
    fn test_clear_layer() {
        let mut store = test_store();
        store.append(test_entry("a", ContextLayer::AlwaysPresent, 100));
        store.append(test_entry("b", ContextLayer::RuntimeInject, 200));
        store.append(test_entry("c", ContextLayer::RuntimeInject, 300));
        store.append(test_entry("d", ContextLayer::Memory, 150));

        store.clear_layer(ContextLayer::RuntimeInject);

        assert_eq!(store.entries().len(), 2);
        assert!(store.get_entry("a").is_some());
        assert!(store.get_entry("b").is_none());
        assert!(store.get_entry("c").is_none());
        assert!(store.get_entry("d").is_some());
    }

    #[test]
    fn test_clear_conversation_keeps_l0() {
        let mut store = test_store();
        store.append(test_entry("l0a", ContextLayer::AlwaysPresent, 100));
        store.append(test_entry("l0b", ContextLayer::AlwaysPresent, 200));
        store.append(test_entry("l1", ContextLayer::OnDemand, 300));
        store.append(test_entry("l2", ContextLayer::RuntimeInject, 400));
        store.append(test_entry("l3", ContextLayer::Memory, 500));

        store.clear_conversation();

        // Only L0 (AlwaysPresent) should remain.
        assert_eq!(store.entries().len(), 2);
        assert!(store.get_entry("l0a").is_some());
        assert!(store.get_entry("l0b").is_some());
        assert!(store.get_entry("l1").is_none());
        assert!(store.get_entry("l2").is_none());
        assert!(store.get_entry("l3").is_none());
    }

    #[test]
    fn test_clear_all() {
        let mut store = test_store();
        store.append(test_entry("l0", ContextLayer::AlwaysPresent, 100));
        store.append(test_entry("l2", ContextLayer::RuntimeInject, 200));

        store.clear_all();

        assert!(store.entries().is_empty());
        assert_eq!(store.total_tokens(), 0);
    }

    #[test]
    fn test_build_llm_messages_layer_order() {
        let mut store = test_store();
        // Insert in reverse order to verify sorting.
        store.append(test_entry("mem", ContextLayer::Memory, 50));
        store.append(test_entry("rt", ContextLayer::RuntimeInject, 50));
        store.append(test_entry("od", ContextLayer::OnDemand, 50));
        store.append(test_entry("ap", ContextLayer::AlwaysPresent, 50));

        let msgs = store.build_llm_messages();
        assert_eq!(msgs.len(), 4);

        // Extract ids in order.
        let ids: Vec<String> = msgs
            .iter()
            .map(|m| match m {
                AgentMessage::Standard(msg) => msg.id.clone(),
                _ => "other".to_string(),
            })
            .collect();

        // Expected order: AlwaysPresent → OnDemand → RuntimeInject → Memory
        assert_eq!(ids, vec!["ap", "od", "rt", "mem"]);
    }

    #[test]
    fn test_build_llm_messages_excludes_compacted() {
        let mut store = test_store();
        store.append(test_entry("a", ContextLayer::AlwaysPresent, 100));
        store.append(test_entry("b", ContextLayer::RuntimeInject, 200));

        // Mark "b" as compacted.
        if let Some(e) = store.get_entry_mut("b") {
            e.metadata.compacted = true;
        }

        let msgs = store.build_llm_messages();
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_tool_pattern_tracking() {
        let mut store = test_store();

        store.track_tool_call("grep", 500);
        store.track_tool_call("grep", 300);
        store.track_tool_call("read_file", 1000);

        let patterns = store.get_tool_patterns(10);
        assert_eq!(patterns.len(), 2);

        let grep_pattern = patterns.iter().find(|p| p.tool_name == "grep").unwrap();
        assert_eq!(grep_pattern.call_count, 2);
        assert_eq!(grep_pattern.total_result_tokens, 800);
        assert_eq!(grep_pattern.avg_result_tokens, 400); // 800/2

        let read_pattern = patterns
            .iter()
            .find(|p| p.tool_name == "read_file")
            .unwrap();
        assert_eq!(read_pattern.call_count, 1);
        assert_eq!(read_pattern.total_result_tokens, 1000);
        assert_eq!(read_pattern.avg_result_tokens, 1000);
    }

    #[test]
    fn test_tool_pattern_tracking_last_n() {
        let mut store = test_store();

        // Track 5 calls with different token counts.
        for i in 1..=5 {
            store.track_tool_call("tool", i * 100);
        }

        // Get only last 3.
        let patterns = store.get_tool_patterns(3);
        let p = &patterns[0];
        assert_eq!(p.call_count, 3);
        // Last 3 are 500, 400, 300 (reversed iteration) → total = 1200, avg = 400
        assert_eq!(p.total_result_tokens, 1200);
        assert_eq!(p.avg_result_tokens, 400);
    }
}
