// INPUT:  std::future::Future, std::pin::Pin, std::sync::Arc, alva_types::AgentMessage, async_trait, tokio::sync::Mutex, crate::plugin (ContextError, ContextHooks), crate::sdk::ContextHandle, crate::store::estimate_tokens, crate::types
// OUTPUT: pub type DefaultSummarizeFn, pub type ExtractMemoryFn, pub struct MemoryCandidate, pub struct DefaultHooksConfig, pub struct DefaultContextHooks
// POS:    Built-in production context plugin combining deterministic rules with optional LLM callbacks for summarization and memory extraction.
//! DefaultContextHooks — the built-in production plugin.
//!
//! Uses deterministic rules for fast-path decisions + LLM callbacks for
//! summarization and memory extraction. This is the plugin that ships as default.
//!
//! Design principles:
//! - Deterministic where possible (rules), LLM only where judgment is needed
//! - Fail-safe: if LLM callback fails, fall back to truncation
//! - Budget-aware: every decision considers token cost
//! - Prompt-cache friendly: never mutate L0/L1 order unless explicitly asked

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use alva_types::AgentMessage;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::plugin::{ContextError, ContextHooks};
use crate::sdk::ContextHandle;
use crate::store::estimate_tokens;
use crate::types::*;

// ---------------------------------------------------------------------------
// LLM callbacks (injected, not hardcoded)
// ---------------------------------------------------------------------------

/// Callback to generate an LLM summary of conversation text.
///
/// NOTE: This is distinct from `crate::sdk_impl::SummarizeFn` which takes
/// `&[AgentMessage]` and returns a plain `String`. This variant takes
/// pre-extracted `String` text and returns `Result<String, String>`,
/// matching the LLM callback pattern used by `DefaultContextHooks`.
pub type DefaultSummarizeFn = Arc<
    dyn Fn(String, Vec<String>) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Callback to extract memory candidates from conversation text.
pub type ExtractMemoryFn = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryCandidate>, String>> + Send>>
        + Send
        + Sync,
>;

/// A memory candidate extracted by LLM, before dedup/filtering.
#[derive(Debug, Clone)]
pub struct MemoryCandidate {
    pub text: String,
    pub confidence: f32,
    pub category: MemoryCategory,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the default context plugin.
pub struct DefaultHooksConfig {
    /// Hard-cap threshold at which emergency sliding window fires. Default: 0.95.
    pub emergency_threshold: f32,
    /// Messages to keep after sliding window. Default: 20.
    pub sliding_window_keep: usize,
    /// Max memory facts to inject per turn. Default: 10.
    pub max_memory_inject: usize,
    /// Max tokens for memory injection. Default: 1500.
    pub max_memory_tokens: usize,
    /// LLM summarization callback (optional — falls back to truncation if None).
    pub summarize_fn: Option<DefaultSummarizeFn>,
    /// LLM memory extraction callback (optional — skips extraction if None).
    pub extract_memory_fn: Option<ExtractMemoryFn>,
}

impl Default for DefaultHooksConfig {
    fn default() -> Self {
        Self {
            emergency_threshold: 0.95,
            sliding_window_keep: 20,
            max_memory_inject: 10,
            max_memory_tokens: 1500,
            summarize_fn: None,
            extract_memory_fn: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin state
// ---------------------------------------------------------------------------

/// Maximum number of recent messages to keep for memory extraction.
const RECENT_MESSAGE_BUFFER_SIZE: usize = 10;

/// Per-session state tracked by the plugin.
struct SessionState {
    /// Whether bootstrap has run.
    bootstrapped: bool,
    /// Number of turns completed.
    turn_count: usize,
    /// Recent message texts for memory extraction.
    /// Populated by `ingest` and `on_message` hooks.
    /// Used by `collect_conversation_text` in `after_turn`.
    recent_messages: Vec<RecentMessage>,
}

/// A cached recent message for memory extraction.
struct RecentMessage {
    role: &'static str,
    text: String,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            bootstrapped: false,
            turn_count: 0,
            recent_messages: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// DefaultContextHooks
// ---------------------------------------------------------------------------

/// The built-in production context management plugin.
///
/// Combines deterministic rules with optional LLM callbacks:
/// - Rules handle: budget checks, file/media truncation, tool result sizing
/// - LLM handles: summarization, memory extraction (when callbacks provided)
/// - Falls back to truncation if LLM is unavailable
pub struct DefaultContextHooks {
    config: DefaultHooksConfig,
    state: Mutex<SessionState>,
}

impl DefaultContextHooks {
    pub fn new(config: DefaultHooksConfig) -> Self {
        Self {
            config,
            state: Mutex::new(SessionState::default()),
        }
    }

    /// Check if a message is a tool result.
    fn is_tool_result(msg: &AgentMessage) -> bool {
        match msg {
            AgentMessage::Standard(m) => m.role == alva_types::MessageRole::Tool,
            _ => false,
        }
    }

    /// Extract tool_call_id from a message (for maintaining call chain).
    fn extract_tool_call_id(msg: &AgentMessage) -> Option<String> {
        match msg {
            AgentMessage::Standard(m) => m.tool_call_id.clone(),
            _ => None,
        }
    }

    /// Compress a tool result into a one-line summary.
    fn compact_tool_result(msg: &AgentMessage) -> String {
        match msg {
            AgentMessage::Standard(m) => {
                let full = m.text_content();
                let first_line = full.lines().next().unwrap_or("(empty)");
                let total_lines = full.lines().count();
                let tokens = estimate_tokens(&full);
                if total_lines <= 1 {
                    full
                } else {
                    format!(
                        "{} [...{} lines, ~{} tokens compacted]",
                        first_line, total_lines, tokens
                    )
                }
            }
            _ => "[compacted tool result]".to_string(),
        }
    }

    /// Estimate token count for a single message.
    fn estimate_message_tokens(msg: &AgentMessage) -> usize {
        match msg {
            AgentMessage::Standard(m) => estimate_tokens(&m.text_content()),
            AgentMessage::Extension { data, .. } => estimate_tokens(&data.to_string()),
            _ => 0, // Steering, FollowUp, Marker — negligible
        }
    }

    /// Extract conversation text for memory extraction from the recent messages buffer.
    ///
    /// Previously this read from the ContextStore snapshot, but the store only
    /// holds a synthetic usage-tracking entry. Now we use the internal buffer
    /// populated by `on_message` and `ingest`.
    fn collect_conversation_text(recent: &[RecentMessage]) -> String {
        recent
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Push a message into the recent messages buffer, evicting oldest if full.
    async fn record_recent_message(&self, role: &'static str, text: String) {
        let mut state = self.state.lock().await;
        if state.recent_messages.len() >= RECENT_MESSAGE_BUFFER_SIZE {
            state.recent_messages.remove(0);
        }
        state.recent_messages.push(RecentMessage { role, text });
    }
}

#[async_trait]
impl ContextHooks for DefaultContextHooks {
    fn name(&self) -> &str {
        "default-context-plugin"
    }

    // =====================================================================
    // PHASE 1: Lifecycle
    // =====================================================================

    async fn bootstrap(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        let mut state = self.state.lock().await;
        if state.bootstrapped {
            return Ok(());
        }

        // Inject relevant memory on session start
        let facts = sdk.inject_memory(agent_id, "", self.config.max_memory_tokens);
        if !facts.is_empty() {
            tracing::info!(
                agent_id,
                count = facts.len(),
                "bootstrap: injected {} memory facts",
                facts.len()
            );
        }

        state.bootstrapped = true;
        Ok(())
    }

    // =====================================================================
    // PHASE 4: Assembly & compression
    //
    // Three strategies combined:
    //   S1. Sliding window — cap message count, drop oldest
    //   S2. Tool result replacement (micro_compact) — every turn, replace
    //       old tool outputs with one-line summaries
    //   S3. LLM summarization — when S1+S2 aren't enough, summarize
    //       old conversation while preserving decisions & identifiers
    //
    // Prompt Cache: messages are assumed to arrive in cache-friendly order
    // (system prompt → tool defs → skills → conversation). We only trim
    // from the conversation tail, never touch the stable prefix.
    // =====================================================================

    /// Core assembly: three-strategy compression within token budget.
    ///
    /// Operates on `Vec<ContextEntry>` — each entry wraps a message + metadata.
    /// Compression replaces the message inside the entry, preserving metadata.
    async fn assemble(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        entries: Vec<ContextEntry>,
        token_budget: usize,
    ) -> Vec<ContextEntry> {
        if entries.is_empty() {
            return entries;
        }

        let max_messages = self.config.sliding_window_keep;
        let mut total_tokens: usize = entries.iter().map(|e| Self::estimate_message_tokens(&e.message)).sum();

        // --- S2: micro_compact — replace old tool results in-place -------
        // Walk entries, for any ToolResult older than the recent 5, replace
        // with a one-liner if it's over 500 tokens. This runs BEFORE
        // sliding window so we save tokens without losing message count.
        //
        // S3: LLM summarization — for large old messages (>2000 tokens) that
        // have summarize_fn available, use LLM summary instead of first-line
        // truncation. Limited to 3 concurrent summarizations with 5s timeout.
        let mut compacted: Vec<ContextEntry> = Vec::with_capacity(entries.len());
        let recent_boundary = entries.len().saturating_sub(5);

        // Collect indices of old large messages eligible for LLM summarization.
        // We limit to 3 to bound latency.
        let mut llm_summary_candidates: Vec<(usize, String, Vec<String>)> = Vec::new();
        if self.config.summarize_fn.is_some() {
            for (i, entry) in entries.iter().enumerate() {
                if i >= recent_boundary {
                    break;
                }
                let msg_tokens = Self::estimate_message_tokens(&entry.message);
                let is_tool_result = Self::is_tool_result(&entry.message);
                // Only LLM-summarize non-tool messages > 2000 tokens (tool results
                // use the cheaper compact_tool_result path below).
                if !is_tool_result && msg_tokens > 2000 && llm_summary_candidates.len() < 3 {
                    let text = match &entry.message {
                        AgentMessage::Standard(m) => m.text_content(),
                        AgentMessage::Extension { data, .. } => data.to_string(),
                        _ => continue, // skip non-content messages
                    };
                    llm_summary_candidates.push((
                        i,
                        text,
                        vec!["Preserve key decisions and identifiers".to_string()],
                    ));
                }
            }
        }

        // Run LLM summarizations concurrently (up to 3) with 5-second timeout each.
        let mut llm_summaries: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();
        if let Some(ref summarize_fn) = self.config.summarize_fn {
            let futs: Vec<_> = llm_summary_candidates
                .into_iter()
                .map(|(idx, text, hints)| {
                    let sf = summarize_fn.clone();
                    async move {
                        let result = tokio::time::timeout(
                            std::time::Duration::from_secs(5),
                            sf(text, hints),
                        )
                        .await;
                        match result {
                            Ok(Ok(summary)) => (idx, Some(summary)),
                            Ok(Err(e)) => {
                                tracing::warn!("assemble LLM summarize failed: {}", e);
                                (idx, None)
                            }
                            Err(_) => {
                                tracing::warn!("assemble LLM summarize timed out for index {}", idx);
                                (idx, None)
                            }
                        }
                    }
                })
                .collect();

            let results = futures::future::join_all(futs).await;
            for (idx, summary_opt) in results {
                if let Some(summary) = summary_opt {
                    llm_summaries.insert(idx, summary);
                }
            }
        }

        for (i, entry) in entries.into_iter().enumerate() {
            let msg_tokens = Self::estimate_message_tokens(&entry.message);
            let is_old = i < recent_boundary;
            let is_tool_result = Self::is_tool_result(&entry.message);

            if is_old && is_tool_result && msg_tokens > 500 {
                // Replace tool results with compact placeholder (cheap, deterministic).
                let summary = Self::compact_tool_result(&entry.message);
                let summary_tokens = estimate_tokens(&summary);
                total_tokens = total_tokens - msg_tokens + summary_tokens;
                let new_msg = AgentMessage::Standard(alva_types::Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: alva_types::MessageRole::Tool,
                    content: vec![alva_types::ContentBlock::Text { text: summary }],
                    tool_call_id: Self::extract_tool_call_id(&entry.message),
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                });
                let mut meta = entry.metadata.clone();
                meta.compacted = true;
                meta.estimated_tokens = summary_tokens;
                compacted.push(ContextEntry {
                    id: entry.id,
                    message: new_msg,
                    metadata: meta,
                });
            } else if let Some(summary) = llm_summaries.remove(&i) {
                // Replace with LLM-generated summary.
                let summary_tokens = estimate_tokens(&summary);
                total_tokens = total_tokens - msg_tokens + summary_tokens;
                let role = match &entry.message {
                    AgentMessage::Standard(m) => m.role.clone(),
                    _ => alva_types::MessageRole::User,
                };
                let new_msg = AgentMessage::Standard(alva_types::Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role,
                    content: vec![alva_types::ContentBlock::Text {
                        text: format!("[summarized] {}", summary),
                    }],
                    tool_call_id: None,
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                });
                let mut meta = entry.metadata.clone();
                meta.compacted = true;
                meta.estimated_tokens = summary_tokens;
                meta.replacement_summary = Some(summary);
                compacted.push(ContextEntry {
                    id: entry.id,
                    message: new_msg,
                    metadata: meta,
                });
            } else {
                compacted.push(entry);
            }
        }

        // --- S1: sliding window — cap message count ----------------------
        let mut kept: Vec<ContextEntry> = if compacted.len() > max_messages {
            let dropped = compacted.len() - max_messages;
            // Recalculate tokens for dropped messages
            for entry in compacted.iter().take(dropped) {
                total_tokens -= Self::estimate_message_tokens(&entry.message);
            }
            tracing::debug!(
                total = compacted.len(),
                dropped,
                keeping = max_messages,
                "assemble: sliding window"
            );
            compacted.into_iter().skip(dropped).collect()
        } else {
            compacted
        };

        // --- Budget enforcement — drop oldest until fit ------------------
        while total_tokens > token_budget && kept.len() > 1 {
            let removed = kept.remove(0);
            total_tokens -= Self::estimate_message_tokens(&removed.message);
        }

        tracing::debug!(
            messages = kept.len(),
            tokens = total_tokens,
            budget = token_budget,
            "assemble: final context"
        );

        kept
    }

    /// Budget exceeded callback — escalating strategies.
    ///
    /// Level 1 (cheap): Remove Disposable entries — no-op until store holds real entries
    /// Level 2 (cheap): Replace old large tool results — no-op until store holds real entries
    /// Level 3 (costs LLM): Summarize old conversation
    /// Level 4 (emergency): Hard sliding window — ALWAYS works (applied to state.messages by agent loop)
    ///
    /// NOTE: In the current architecture, the snapshot only contains the synthetic
    /// usage-tracking entry from sync_external_usage. Levels 1 and 2 scan
    /// snapshot.entries for Disposable/Tool entries, which won't exist yet.
    /// These levels will activate once ContextStore is integrated with the
    /// real conversation (tracked in store integration TODO).
    ///
    /// Tool-result compression for the current architecture is handled by
    /// `assemble()` (S2: micro_compact), which operates on actual messages.
    async fn on_budget_exceeded(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        let mut actions = Vec::new();
        let budget = sdk.budget(agent_id);

        tracing::info!(
            agent_id,
            used = budget.used_tokens,
            budget = budget.budget_tokens,
            ratio = format!("{:.0}%", budget.usage_ratio * 100.0),
            "on_budget_exceeded: compressing"
        );

        // Level 1: remove Disposable (effective once store holds real entries)
        if snapshot.entries.iter().any(|e| e.priority == Priority::Disposable) {
            actions.push(CompressAction::RemoveByPriority {
                priority: Priority::Disposable,
            });
        }

        // Level 2: replace old large tool results (effective once store holds real entries)
        // In the current architecture, assemble()'s micro_compact (S2) provides
        // equivalent functionality directly on the message list.
        for entry in &snapshot.entries {
            if entry.priority <= Priority::Low
                && entry.estimated_tokens > 500
                && matches!(&entry.origin, EntryOrigin::Tool { .. })
            {
                actions.push(CompressAction::ReplaceToolResult {
                    message_id: entry.id.clone(),
                    summary: format!(
                        "[tool result compressed: ~{} tokens → 1 line]",
                        entry.estimated_tokens
                    ),
                });
            }
        }

        // Level 3: LLM summarization (if callback available and ratio > 85%)
        if budget.usage_ratio > 0.85 && self.config.summarize_fn.is_some() {
            let old_count = snapshot
                .entries
                .len()
                .saturating_sub(self.config.sliding_window_keep);
            if old_count > 3 {
                actions.push(CompressAction::Summarize {
                    range: MessageRange {
                        from: MessageSelector::FromStart,
                        to: MessageSelector::ByIndex(old_count),
                    },
                    hints: vec![
                        "Preserve architecture decisions verbatim".into(),
                        "Preserve ALL file paths, UUIDs, hashes, ports exactly".into(),
                        "Preserve unfinished TODOs and blocking issues".into(),
                        "Discard intermediate reasoning and tool outputs".into(),
                    ],
                });
            }
        }

        // Level 4: emergency sliding window (always effective — agent loop applies
        // this directly to state.messages via drain)
        if budget.usage_ratio > self.config.emergency_threshold {
            actions.push(CompressAction::SlidingWindow {
                keep_recent: self.config.sliding_window_keep,
            });
        }

        actions
    }

    // =====================================================================
    // PHASE 4: User message enrichment
    // =====================================================================

    async fn on_message(
        &self,
        sdk: &dyn ContextHandle,
        _agent_id: &str,
        message: &AgentMessage,
    ) -> Vec<Injection> {
        let mut injections = Vec::new();

        // Extract user text for memory query
        let query = match message {
            AgentMessage::Standard(m) => m.text_content(),
            _ => String::new(),
        };

        // Record into recent messages buffer for memory extraction
        if !query.is_empty() {
            self.record_recent_message("User", query.clone()).await;

            // Query memory for relevant facts
            let facts = sdk.query_memory(&query, self.config.max_memory_inject);
            if !facts.is_empty() {
                injections.push(Injection::memory(facts));
            }
        }

        injections
    }

    // =====================================================================
    // PHASE 6: Post-turn
    // =====================================================================

    // =====================================================================
    // PHASE 5: Ingest
    // =====================================================================

    async fn ingest(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        entry: &ContextEntry,
    ) -> IngestAction {
        // Record LLM output text into the recent messages buffer for memory extraction.
        let text = match &entry.message {
            AgentMessage::Standard(m) => m.text_content(),
            AgentMessage::Extension { data, .. } => data.to_string(),
            _ => String::new(),
        };
        if !text.is_empty() {
            let role = match &entry.message {
                AgentMessage::Standard(m) => match m.role {
                    alva_types::MessageRole::User => "User",
                    alva_types::MessageRole::Assistant => "Assistant",
                    alva_types::MessageRole::Tool => "Tool",
                    _ => "System",
                },
                _ => "Extension",
            };
            self.record_recent_message(role, text).await;
        }
        IngestAction::Keep
    }

    // =====================================================================
    // PHASE 6: Post-turn
    // =====================================================================

    async fn after_turn(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
    ) {
        // Collect recent messages under lock, then release before async LLM work.
        let conversation = {
            let mut state = self.state.lock().await;
            state.turn_count += 1;

            // Extract memory every 3 turns (not every turn, to save cost)
            if state.turn_count % 3 != 0 || self.config.extract_memory_fn.is_none() {
                return;
            }

            Self::collect_conversation_text(&state.recent_messages)
        };

        if conversation.is_empty() {
            return;
        }

        if let Some(ref extract_fn) = self.config.extract_memory_fn {
            match extract_fn(conversation).await {
                Ok(candidates) => {
                    // Build candidate MemoryFacts from raw extraction results.
                    let facts: Vec<MemoryFact> = candidates
                        .into_iter()
                        .filter(|c| c.confidence >= 0.65)
                        .map(|candidate| MemoryFact {
                            id: uuid::Uuid::new_v4().to_string(),
                            text: candidate.text,
                            fingerprint: String::new(), // TODO: compute SHA1
                            confidence: candidate.confidence,
                            category: candidate.category,
                            source_session: agent_id.to_string(),
                            created_at: chrono::Utc::now().timestamp_millis(),
                            last_accessed_at: chrono::Utc::now().timestamp_millis(),
                            access_count: 0,
                        })
                        .collect();

                    for fact in facts {
                        sdk.store_memory(fact);
                    }
                }
                Err(e) => {
                    tracing::warn!("Memory extraction failed: {}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ContextHooks;
    use crate::sdk_impl::ContextHandleImpl;
    use crate::store::ContextStore;
    use alva_types::{ContentBlock, Message, MessageRole};
    use std::sync::{Arc, Mutex};

    /// Create a real SDK backed by a ContextStore for testing.
    fn test_sdk() -> ContextHandleImpl {
        let store = Arc::new(Mutex::new(ContextStore::new(100_000, 80_000)));
        ContextHandleImpl::new(store)
    }

    /// Create a user AgentMessage with the given text.
    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: format!("msg-{}", text),
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 1000,
        })
    }

    /// Create a tool-result AgentMessage with the given text and token-like size.
    fn tool_msg(id: &str, text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: id.to_string(),
            role: MessageRole::Tool,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: Some(format!("call-{}", id)),
            usage: None,
            timestamp: 3000,
        })
    }

    /// Wrap an AgentMessage in a ContextEntry with default metadata.
    fn wrap_entry(msg: AgentMessage) -> ContextEntry {
        let id = match &msg {
            AgentMessage::Standard(m) => m.id.clone(),
            AgentMessage::Extension { data, .. } => data.to_string(),
            _ => uuid::Uuid::new_v4().to_string(),
        };
        ContextEntry {
            id,
            message: msg,
            metadata: ContextMetadata::new(ContextLayer::RuntimeInject),
        }
    }

    #[tokio::test]
    async fn test_assemble_sliding_window() {
        let sdk = test_sdk();
        let config = DefaultHooksConfig {
            sliding_window_keep: 5,
            ..DefaultHooksConfig::default()
        };
        let plugin = DefaultContextHooks::new(config);

        // Create 10 user messages wrapped in entries.
        let entries: Vec<ContextEntry> = (0..10)
            .map(|i| wrap_entry(user_msg(&format!("message number {}", i))))
            .collect();

        let result = plugin
            .assemble(&sdk, "agent-1", entries, 100_000)
            .await;

        // Should keep only the last 5.
        assert_eq!(result.len(), 5);

        // Verify the kept entries are the last 5 (indices 5..10).
        for (i, entry) in result.iter().enumerate() {
            if let AgentMessage::Standard(m) = &entry.message {
                let expected_text = format!("message number {}", i + 5);
                assert_eq!(m.text_content(), expected_text);
            } else {
                panic!("Expected Standard message");
            }
        }
    }

    #[tokio::test]
    async fn test_assemble_micro_compact() {
        let sdk = test_sdk();
        let config = DefaultHooksConfig {
            sliding_window_keep: 100, // High enough to not trigger sliding window.
            ..DefaultHooksConfig::default()
        };
        let plugin = DefaultContextHooks::new(config);

        // Create entries: 6 old tool results (>500 tokens each) + 5 recent user msgs.
        // The old tool results should be compacted (micro_compact).
        let large_text: String = "line1\n".repeat(400); // ~2400 chars = ~600 tokens
        let mut entries = Vec::new();
        for i in 0..6 {
            entries.push(wrap_entry(tool_msg(&format!("tool-{}", i), &large_text)));
        }
        for i in 0..5 {
            entries.push(wrap_entry(user_msg(&format!("recent {}", i))));
        }

        let result = plugin
            .assemble(&sdk, "agent-1", entries, 100_000)
            .await;

        assert_eq!(result.len(), 11); // All 11 entries kept (no sliding window)

        // The first 6 (old tool results) should have been compacted.
        for entry in result.iter().take(6) {
            if let AgentMessage::Standard(m) = &entry.message {
                assert_eq!(m.role, MessageRole::Tool);
                // Compacted message should contain "[...X lines" marker.
                assert!(
                    m.text_content().contains("[..."),
                    "Expected compacted tool result, got: {}",
                    m.text_content()
                );
                // Metadata should be marked as compacted.
                assert!(entry.metadata.compacted);
            } else {
                panic!("Expected Standard message");
            }
        }

        // The last 5 (recent user) should be unchanged.
        for (i, entry) in result.iter().skip(6).enumerate() {
            if let AgentMessage::Standard(m) = &entry.message {
                assert_eq!(m.text_content(), format!("recent {}", i));
            }
        }
    }

    #[tokio::test]
    async fn test_assemble_budget_enforcement() {
        let sdk = test_sdk();
        let config = DefaultHooksConfig {
            sliding_window_keep: 100,
            ..DefaultHooksConfig::default()
        };
        let plugin = DefaultContextHooks::new(config);

        // Each message is ~100 tokens (400 chars / 4). Create 10 entries.
        let text_400_chars: String = "a".repeat(400);
        let entries: Vec<ContextEntry> = (0..10)
            .map(|i| {
                wrap_entry(AgentMessage::Standard(Message {
                    id: format!("m{}", i),
                    role: MessageRole::User,
                    content: vec![ContentBlock::Text {
                        text: text_400_chars.clone(),
                    }],
                    tool_call_id: None,
                    usage: None,
                    timestamp: 1000 + i as i64,
                }))
            })
            .collect();

        // Budget of 300 tokens. Each message ~100 tokens.
        // Budget enforcement drops oldest until total <= 300, keeping at least 1.
        let result = plugin
            .assemble(&sdk, "agent-1", entries, 300)
            .await;

        // 300 budget / 100 tokens per message = 3 messages should fit.
        assert!(result.len() <= 3, "Expected <=3, got {}", result.len());
        assert!(!result.is_empty());

        // Verify the kept entries are the most recent ones.
        if let AgentMessage::Standard(m) = &result[result.len() - 1].message {
            assert_eq!(m.id, "m9"); // The last message should survive.
        }
    }

    #[tokio::test]
    async fn test_assemble_empty() {
        let sdk = test_sdk();
        let plugin = DefaultContextHooks::new(DefaultHooksConfig::default());

        let result = plugin
            .assemble(&sdk, "agent-1", vec![], 100_000)
            .await;

        assert!(result.is_empty());
    }
}
