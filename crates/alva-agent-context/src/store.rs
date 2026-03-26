//! ContextStore — per-agent context container with five-layer management.

use std::collections::HashMap;
use std::path::PathBuf;

use alva_types::AgentMessage;

use crate::types::*;

/// Per-agent context container. Stores entries organized by layer.
pub struct ContextStore {
    entries: Vec<ContextEntry>,
    /// Model context window size in tokens.
    model_window: usize,
    /// Token budget (may be less than model_window).
    budget_tokens: usize,
    /// Directory for externalized files.
    pub externalize_dir: PathBuf,
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
    pub fn new(model_window: usize, budget_tokens: usize, externalize_dir: PathBuf) -> Self {
        Self {
            entries: Vec::new(),
            model_window,
            budget_tokens,
            externalize_dir,
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
            entry.metadata.compacted = true;
            // Replace message content with summary
            entry.message = AgentMessage::Standard(alva_types::Message {
                id: entry.id.clone(),
                role: alva_types::MessageRole::Tool,
                content: vec![alva_types::ContentBlock::Text {
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
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
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
        AgentMessage::Custom { type_name, .. } => {
            format!("[custom: {}]", type_name)
        }
    }
}
