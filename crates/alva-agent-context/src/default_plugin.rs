//! DefaultContextPlugin — the built-in production plugin.
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

use crate::plugin::{ContextError, ContextPlugin};
use crate::sdk::ContextManagementSDK;
use crate::store::estimate_tokens;
use crate::types::*;

// ---------------------------------------------------------------------------
// LLM callbacks (injected, not hardcoded)
// ---------------------------------------------------------------------------

/// Callback to generate an LLM summary of conversation text.
pub type SummarizeFn = Arc<
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
pub struct DefaultPluginConfig {
    /// Budget threshold (0.0-1.0) at which compression triggers. Default: 0.7.
    pub compress_threshold: f32,
    /// Hard-cap threshold at which emergency sliding window fires. Default: 0.95.
    pub emergency_threshold: f32,
    /// Messages to keep after sliding window. Default: 20.
    pub sliding_window_keep: usize,
    /// Tool result token threshold for auto-replace. Default: 5000.
    pub large_tool_result_tokens: usize,
    /// File injection token threshold for auto-truncate. Default: 8000.
    pub large_file_tokens: usize,
    /// Media token threshold for auto-remove. Default: 2000.
    pub large_media_tokens: usize,
    /// Sub-agent result token threshold for summarization. Default: 2000.
    pub sub_agent_result_tokens: usize,
    /// Max memory facts to inject per turn. Default: 10.
    pub max_memory_inject: usize,
    /// Max tokens for memory injection. Default: 1500.
    pub max_memory_tokens: usize,
    /// LLM summarization callback (optional — falls back to truncation if None).
    pub summarize_fn: Option<SummarizeFn>,
    /// LLM memory extraction callback (optional — skips extraction if None).
    pub extract_memory_fn: Option<ExtractMemoryFn>,
}

impl Default for DefaultPluginConfig {
    fn default() -> Self {
        Self {
            compress_threshold: 0.7,
            emergency_threshold: 0.95,
            sliding_window_keep: 20,
            large_tool_result_tokens: 5000,
            large_file_tokens: 8000,
            large_media_tokens: 2000,
            sub_agent_result_tokens: 2000,
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
    /// Accumulated tool call token counts this session.
    tool_token_totals: Vec<(String, usize)>,
    /// Number of turns completed.
    turn_count: usize,
    /// Recent message texts for memory extraction.
    /// Populated by `on_user_message` and `on_llm_output` hooks.
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
            tool_token_totals: Vec::new(),
            turn_count: 0,
            recent_messages: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// DefaultContextPlugin
// ---------------------------------------------------------------------------

/// The built-in production context management plugin.
///
/// Combines deterministic rules with optional LLM callbacks:
/// - Rules handle: budget checks, file/media truncation, tool result sizing
/// - LLM handles: summarization, memory extraction (when callbacks provided)
/// - Falls back to truncation if LLM is unavailable
pub struct DefaultContextPlugin {
    config: DefaultPluginConfig,
    state: Mutex<SessionState>,
}

impl DefaultContextPlugin {
    pub fn new(config: DefaultPluginConfig) -> Self {
        Self {
            config,
            state: Mutex::new(SessionState::default()),
        }
    }

    /// Try LLM summarization, fall back to truncation on failure.
    async fn summarize_or_truncate(&self, text: &str, hints: &[String]) -> String {
        if let Some(ref summarize) = self.config.summarize_fn {
            match summarize(text.to_string(), hints.to_vec()).await {
                Ok(summary) => return summary,
                Err(e) => {
                    tracing::warn!("LLM summarization failed, falling back to truncation: {}", e);
                }
            }
        }
        // Fallback: keep first 2000 chars
        let truncated: String = text.chars().take(2000).collect();
        format!("{}\n\n[... summarization unavailable, truncated]", truncated)
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
            AgentMessage::Custom { data, .. } => estimate_tokens(&data.to_string()),
        }
    }

    /// Extract conversation text for memory extraction from the recent messages buffer.
    ///
    /// Previously this read from the ContextStore snapshot, but the store only
    /// holds a synthetic usage-tracking entry. Now we use the internal buffer
    /// populated by `on_user_message` and `on_llm_output`.
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
impl ContextPlugin for DefaultContextPlugin {
    fn name(&self) -> &str {
        "default-context-plugin"
    }

    // =====================================================================
    // PHASE 1: Lifecycle
    // =====================================================================

    async fn bootstrap(
        &self,
        sdk: &dyn ContextManagementSDK,
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

    async fn maintain(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        // NOTE: In the current architecture, the real conversation lives in
        // AgentState.messages (owned by the agent loop), not in ContextStore.
        // The store only holds a synthetic usage-tracking entry. Therefore,
        // Disposable-entry cleanup via snapshot is a no-op here.
        //
        // Actual compression happens in two places:
        //   1. agent_loop: on_budget_exceeded → CompressAction applied to state.messages
        //   2. assemble(): micro_compact + sliding_window + budget enforcement
        //
        // Budget check still works because sync_external_usage provides real
        // token counts before this hook is called.
        let budget = sdk.budget(agent_id);
        if budget.usage_ratio > self.config.compress_threshold {
            tracing::info!(
                agent_id,
                usage = format!("{:.0}%", budget.usage_ratio * 100.0),
                "maintain: budget approaching threshold (compression deferred to assemble)"
            );
        }
        Ok(())
    }

    // =====================================================================
    // PHASE 3: Five-layer injection control
    // =====================================================================

    async fn on_inject_memory(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        mut facts: Vec<MemoryFact>,
    ) -> Vec<MemoryFact> {
        // Cap the number of facts
        facts.truncate(self.config.max_memory_inject);

        // Cap total tokens
        let mut total_tokens = 0;
        facts.retain(|f| {
            let tokens = estimate_tokens(&f.text);
            if total_tokens + tokens > self.config.max_memory_tokens {
                return false;
            }
            total_tokens += tokens;
            true
        });

        facts
    }

    async fn on_inject_skill(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        skill_name: &str,
        skill_content: String,
    ) -> InjectDecision<String> {
        let tokens = estimate_tokens(&skill_content);
        if tokens > 15000 {
            // Very large skill: summarize if we have LLM, otherwise truncate
            let summary = self
                .summarize_or_truncate(
                    &skill_content,
                    &[format!("Skill: {}", skill_name)],
                )
                .await;
            InjectDecision::Summarize { summary }
        } else {
            InjectDecision::Allow(skill_content)
        }
    }

    async fn on_inject_file(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        file_path: &str,
        content: String,
        content_tokens: usize,
    ) -> InjectDecision<String> {
        if content_tokens > self.config.large_file_tokens {
            tracing::info!(
                file_path,
                tokens = content_tokens,
                "on_inject_file: file exceeds threshold, truncating"
            );
            let truncated: String = content.lines().take(500).collect::<Vec<_>>().join("\n");
            InjectDecision::Modify(format!(
                "{}\n\n[... file truncated: {} → ~{} tokens. Use read tool for full content.]",
                truncated,
                content_tokens,
                estimate_tokens(&truncated)
            ))
        } else {
            InjectDecision::Allow(content)
        }
    }

    async fn on_inject_media(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        media_type: &str,
        _source: MediaSource,
        _size_bytes: usize,
        estimated_tokens: usize,
    ) -> InjectDecision<MediaAction> {
        if estimated_tokens > self.config.large_media_tokens {
            tracing::info!(
                media_type,
                tokens = estimated_tokens,
                "on_inject_media: large media, rejecting"
            );
            InjectDecision::Reject {
                reason: format!(
                    "Media ({}) is {} tokens. Use a vision/analysis tool to process it instead.",
                    media_type, estimated_tokens
                ),
            }
        } else {
            InjectDecision::Allow(MediaAction::Keep)
        }
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
    async fn assemble(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        messages: Vec<AgentMessage>,
        token_budget: usize,
    ) -> Vec<AgentMessage> {
        if messages.is_empty() {
            return messages;
        }

        let max_messages = self.config.sliding_window_keep;
        let mut total_tokens: usize = messages.iter().map(|m| Self::estimate_message_tokens(m)).sum();

        // --- S2: micro_compact — replace old tool results in-place -------
        // Walk messages, for any ToolResult older than the recent 5, replace
        // with a one-liner if it's over 500 tokens. This runs BEFORE
        // sliding window so we save tokens without losing message count.
        //
        // S3: LLM summarization — for large old messages (>2000 tokens) that
        // have summarize_fn available, use LLM summary instead of first-line
        // truncation. Limited to 3 concurrent summarizations with 5s timeout.
        let mut compacted: Vec<AgentMessage> = Vec::with_capacity(messages.len());
        let recent_boundary = messages.len().saturating_sub(5);

        // Collect indices of old large messages eligible for LLM summarization.
        // We limit to 3 to bound latency.
        let mut llm_summary_candidates: Vec<(usize, String, Vec<String>)> = Vec::new();
        if self.config.summarize_fn.is_some() {
            for (i, msg) in messages.iter().enumerate() {
                if i >= recent_boundary {
                    break;
                }
                let msg_tokens = Self::estimate_message_tokens(msg);
                let is_tool_result = Self::is_tool_result(msg);
                // Only LLM-summarize non-tool messages > 2000 tokens (tool results
                // use the cheaper compact_tool_result path below).
                if !is_tool_result && msg_tokens > 2000 && llm_summary_candidates.len() < 3 {
                    let text = match msg {
                        AgentMessage::Standard(m) => m.text_content(),
                        AgentMessage::Custom { data, .. } => data.to_string(),
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

        for (i, msg) in messages.into_iter().enumerate() {
            let msg_tokens = Self::estimate_message_tokens(&msg);
            let is_old = i < recent_boundary;
            let is_tool_result = Self::is_tool_result(&msg);

            if is_old && is_tool_result && msg_tokens > 500 {
                // Replace tool results with compact placeholder (cheap, deterministic).
                let summary = Self::compact_tool_result(&msg);
                let summary_tokens = estimate_tokens(&summary);
                total_tokens = total_tokens - msg_tokens + summary_tokens;
                compacted.push(AgentMessage::Standard(alva_types::Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: alva_types::MessageRole::Tool,
                    content: vec![alva_types::ContentBlock::Text { text: summary }],
                    tool_call_id: Self::extract_tool_call_id(&msg),
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                }));
            } else if let Some(summary) = llm_summaries.remove(&i) {
                // Replace with LLM-generated summary.
                let summary_tokens = estimate_tokens(&summary);
                total_tokens = total_tokens - msg_tokens + summary_tokens;
                let role = match &msg {
                    AgentMessage::Standard(m) => m.role.clone(),
                    _ => alva_types::MessageRole::User,
                };
                compacted.push(AgentMessage::Standard(alva_types::Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role,
                    content: vec![alva_types::ContentBlock::Text {
                        text: format!("[summarized] {}", summary),
                    }],
                    tool_call_id: None,
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                }));
            } else {
                compacted.push(msg);
            }
        }

        // --- S1: sliding window — cap message count ----------------------
        let mut kept: Vec<AgentMessage> = if compacted.len() > max_messages {
            let dropped = compacted.len() - max_messages;
            // Recalculate tokens for dropped messages
            for msg in compacted.iter().take(dropped) {
                total_tokens -= Self::estimate_message_tokens(msg);
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
            total_tokens -= Self::estimate_message_tokens(&removed);
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
        sdk: &dyn ContextManagementSDK,
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
    // PHASE 5: Tool & sub-agent
    // =====================================================================

    /// After tool execution: track patterns + truncate large results.
    ///
    /// This is the "new result" path. Old results get micro_compacted
    /// during `assemble` on subsequent turns.
    async fn after_tool_call(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        tool_name: &str,
        _result: &AgentMessage,
        result_tokens: usize,
    ) -> ToolResultAction {
        // Track for pattern analysis
        {
            let mut state = self.state.lock().await;
            if let Some(entry) = state
                .tool_token_totals
                .iter_mut()
                .find(|(name, _)| name == tool_name)
            {
                entry.1 += result_tokens;
            } else {
                state
                    .tool_token_totals
                    .push((tool_name.to_string(), result_tokens));
            }
        }

        if result_tokens > self.config.large_tool_result_tokens {
            tracing::info!(
                tool_name,
                tokens = result_tokens,
                "after_tool_call: large result, truncating"
            );
            ToolResultAction::Truncate { max_lines: 200 }
        } else {
            ToolResultAction::Keep
        }
    }

    async fn on_sub_agent_turn(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _parent_id: &str,
        child_id: &str,
        turn_index: usize,
        _turn_summary: &str,
    ) -> SubAgentDirective {
        // Safety: terminate if sub-agent runs too many turns
        if turn_index > 50 {
            tracing::warn!(child_id, turn_index, "sub-agent exceeded 50 turns, terminating");
            return SubAgentDirective::Terminate {
                reason: "Exceeded maximum turn count (50)".to_string(),
            };
        }
        SubAgentDirective::Continue
    }

    async fn on_sub_agent_complete(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _parent_id: &str,
        _child_id: &str,
        result: &str,
        result_tokens: usize,
    ) -> InjectionPlan {
        if result_tokens > self.config.sub_agent_result_tokens {
            // Summarize if LLM available
            let summary = self
                .summarize_or_truncate(
                    result,
                    &["Sub-agent result summary".to_string()],
                )
                .await;
            InjectionPlan::Summary { text: summary }
        } else {
            InjectionPlan::FullResult
        }
    }

    // =====================================================================
    // PHASE 4: User message enrichment
    // =====================================================================

    async fn on_user_message(
        &self,
        sdk: &dyn ContextManagementSDK,
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
                injections.push(Injection::Memory(facts));
            }
        }

        injections
    }

    // =====================================================================
    // PHASE 6: Post-turn
    // =====================================================================

    async fn after_turn(
        &self,
        sdk: &dyn ContextManagementSDK,
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
                    let mut facts: Vec<MemoryFact> = candidates
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

                    // Let on_extract_memory filter/modify candidates before storing.
                    facts = self.on_extract_memory(sdk, agent_id, facts).await;

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

    // =====================================================================
    // PHASE 2: Observation
    // =====================================================================

    async fn on_agent_start(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) {
        let budget = sdk.budget(agent_id);
        tracing::debug!(
            agent_id,
            used = budget.used_tokens,
            budget = budget.budget_tokens,
            "turn start"
        );
    }

    /// ⓭ Record LLM output text into the recent messages buffer for memory extraction.
    async fn on_llm_output(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        response: &AgentMessage,
    ) {
        let text = match response {
            AgentMessage::Standard(m) => m.text_content(),
            AgentMessage::Custom { data, .. } => data.to_string(),
        };
        if !text.is_empty() {
            self.record_recent_message("Assistant", text).await;
        }
    }

    async fn on_agent_end(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        error: Option<&str>,
    ) {
        let budget = sdk.budget(agent_id);
        let state = self.state.lock().await;
        tracing::info!(
            agent_id,
            turns = state.turn_count,
            final_tokens = budget.used_tokens,
            error = error.unwrap_or("none"),
            "agent ended"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ContextPlugin;
    use crate::sdk_impl::ContextSDKImpl;
    use crate::store::ContextStore;
    use alva_types::{ContentBlock, Message, MessageRole};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Create a real SDK backed by a ContextStore for testing.
    fn test_sdk() -> ContextSDKImpl {
        let store = Arc::new(Mutex::new(ContextStore::new(
            100_000,
            80_000,
            PathBuf::from("/tmp/test"),
        )));
        ContextSDKImpl::new(store)
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

    #[tokio::test]
    async fn test_assemble_sliding_window() {
        let sdk = test_sdk();
        let config = DefaultPluginConfig {
            sliding_window_keep: 5,
            ..DefaultPluginConfig::default()
        };
        let plugin = DefaultContextPlugin::new(config);

        // Create 10 user messages.
        let messages: Vec<AgentMessage> = (0..10)
            .map(|i| user_msg(&format!("message number {}", i)))
            .collect();

        let result = plugin
            .assemble(&sdk, "agent-1", messages, 100_000)
            .await;

        // Should keep only the last 5.
        assert_eq!(result.len(), 5);

        // Verify the kept messages are the last 5 (indices 5..10).
        for (i, msg) in result.iter().enumerate() {
            if let AgentMessage::Standard(m) = msg {
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
        let config = DefaultPluginConfig {
            sliding_window_keep: 100, // High enough to not trigger sliding window.
            ..DefaultPluginConfig::default()
        };
        let plugin = DefaultContextPlugin::new(config);

        // Create messages: 6 old tool results (>500 tokens each) + 5 recent user msgs.
        // The old tool results should be compacted (micro_compact).
        let large_text: String = "line1\n".repeat(400); // ~2400 chars = ~600 tokens
        let mut messages = Vec::new();
        for i in 0..6 {
            messages.push(tool_msg(&format!("tool-{}", i), &large_text));
        }
        for i in 0..5 {
            messages.push(user_msg(&format!("recent {}", i)));
        }

        let result = plugin
            .assemble(&sdk, "agent-1", messages, 100_000)
            .await;

        assert_eq!(result.len(), 11); // All 11 messages kept (no sliding window)

        // The first 6 (old tool results) should have been compacted.
        for msg in result.iter().take(6) {
            if let AgentMessage::Standard(m) = msg {
                assert_eq!(m.role, MessageRole::Tool);
                // Compacted message should contain "[...X lines" marker.
                assert!(
                    m.text_content().contains("[..."),
                    "Expected compacted tool result, got: {}",
                    m.text_content()
                );
            } else {
                panic!("Expected Standard message");
            }
        }

        // The last 5 (recent user) should be unchanged.
        for (i, msg) in result.iter().skip(6).enumerate() {
            if let AgentMessage::Standard(m) = msg {
                assert_eq!(m.text_content(), format!("recent {}", i));
            }
        }
    }

    #[tokio::test]
    async fn test_assemble_budget_enforcement() {
        let sdk = test_sdk();
        let config = DefaultPluginConfig {
            sliding_window_keep: 100,
            ..DefaultPluginConfig::default()
        };
        let plugin = DefaultContextPlugin::new(config);

        // Each message is ~100 tokens (400 chars / 4). Create 10 messages.
        let text_400_chars: String = "a".repeat(400);
        let messages: Vec<AgentMessage> = (0..10)
            .map(|i| {
                AgentMessage::Standard(Message {
                    id: format!("m{}", i),
                    role: MessageRole::User,
                    content: vec![ContentBlock::Text {
                        text: text_400_chars.clone(),
                    }],
                    tool_call_id: None,
                    usage: None,
                    timestamp: 1000 + i as i64,
                })
            })
            .collect();

        // Budget of 300 tokens. Each message ~100 tokens.
        // Budget enforcement drops oldest until total <= 300, keeping at least 1.
        let result = plugin
            .assemble(&sdk, "agent-1", messages, 300)
            .await;

        // 300 budget / 100 tokens per message ≈ 3 messages should fit.
        assert!(result.len() <= 3, "Expected <=3, got {}", result.len());
        assert!(!result.is_empty());

        // Verify the kept messages are the most recent ones.
        if let AgentMessage::Standard(m) = &result[result.len() - 1] {
            assert_eq!(m.id, "m9"); // The last message should survive.
        }
    }

    #[tokio::test]
    async fn test_assemble_empty() {
        let sdk = test_sdk();
        let plugin = DefaultContextPlugin::new(DefaultPluginConfig::default());

        let result = plugin
            .assemble(&sdk, "agent-1", vec![], 100_000)
            .await;

        assert!(result.is_empty());
    }
}
