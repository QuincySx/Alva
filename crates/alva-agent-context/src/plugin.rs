//! ContextPlugin trait — 21 hooks covering the full context lifecycle.
//!
//! All methods have default no-op implementations. Plugins only override what they need.

use alva_types::AgentMessage;
use async_trait::async_trait;

use crate::sdk::ContextManagementSDK;
use crate::types::*;

/// Error type for context plugin operations.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Other(String),
}

/// The context management plugin trait.
///
/// Hooks are organized by lifecycle phase (see spec §3.5 for the full timing diagram):
///
/// ```text
/// PHASE 1 (once):     bootstrap
/// PHASE 2 (per-turn): on_agent_start → maintain
/// PHASE 3 (input):    on_user_message → on_inject_*
/// PHASE 4 (assemble): on_inject_system_prompt → assemble → on_budget_exceeded
/// PHASE 5 (execute):  before_tool_call → after_tool_call → on_sub_agent_*
/// PHASE 6 (finalize): ingest → after_turn → on_extract_memory
/// PHASE 7 (end):      on_agent_end → dispose
/// ```
#[async_trait]
pub trait ContextPlugin: Send + Sync {
    /// Human-readable name for this plugin.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    // =====================================================================
    // PHASE 1: Lifecycle
    // =====================================================================

    /// ❶ Session first activation. Import history, load memory, init store.
    /// Called once per session lifetime.
    async fn bootstrap(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        let _ = (sdk, agent_id);
        Ok(())
    }

    /// ❸ Per-turn maintenance before processing user input.
    /// Rewrite history entries, clean expired data.
    async fn maintain(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        let _ = (sdk, agent_id);
        Ok(())
    }

    /// Plugin teardown. Release resources.
    async fn dispose(&self) -> Result<(), ContextError> {
        Ok(())
    }

    // =====================================================================
    // PHASE 3: Five-layer injection control
    // =====================================================================

    /// ❺ L3 Memory injection. Filter, reorder, modify facts before they enter context.
    async fn on_inject_memory(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        facts: Vec<MemoryFact>,
    ) -> Vec<MemoryFact> {
        let _ = (sdk, agent_id);
        facts
    }

    /// ❻ L1 Skill loading. Allow, reject, or summarize skill content.
    async fn on_inject_skill(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        skill_name: &str,
        skill_content: String,
    ) -> InjectDecision<String> {
        let _ = (sdk, agent_id, skill_name);
        InjectDecision::Allow(skill_content)
    }

    /// ❼ L2 File/attachment injection. Allow, reject, summarize, or truncate.
    async fn on_inject_file(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        file_path: &str,
        content: String,
        content_tokens: usize,
    ) -> InjectDecision<String> {
        let _ = (sdk, agent_id, file_path, content_tokens);
        InjectDecision::Allow(content)
    }

    /// ❽ L2 Multi-modal content (image/audio/video). Decide keep/describe/externalize/remove.
    async fn on_inject_media(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        media_type: &str,
        source: MediaSource,
        size_bytes: usize,
        estimated_tokens: usize,
    ) -> InjectDecision<MediaAction> {
        let _ = (sdk, agent_id, media_type, source, size_bytes, estimated_tokens);
        InjectDecision::Allow(MediaAction::Keep)
    }

    /// ❾ L2 Runtime metadata injection (timestamp, channel, preferences).
    async fn on_inject_runtime(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        runtime_data: RuntimeContext,
    ) -> RuntimeContext {
        let _ = (sdk, agent_id);
        runtime_data
    }

    /// ❿ L0 System prompt sections. Modify, add, or remove sections.
    async fn on_inject_system_prompt(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        sections: Vec<PromptSection>,
    ) -> Vec<PromptSection> {
        let _ = (sdk, agent_id);
        sections
    }

    /// L3 Memory extraction. Filter candidates before they are stored.
    async fn on_extract_memory(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        candidates: Vec<MemoryFact>,
    ) -> Vec<MemoryFact> {
        let _ = (sdk, agent_id);
        candidates
    }

    // =====================================================================
    // PHASE 3-4: Per-turn processing
    // =====================================================================

    /// ❹ User message received. Decide what to enrich (memory, skill, runtime).
    /// Return injections to add to context.
    async fn on_user_message(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        message: &AgentMessage,
    ) -> Vec<Injection> {
        let _ = (sdk, agent_id, message);
        vec![]
    }

    /// ⓫ Assemble final context for LLM under token budget.
    /// Receives the full message list after layer sorting; returns the trimmed list.
    async fn assemble(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        messages: Vec<AgentMessage>,
        token_budget: usize,
    ) -> Vec<AgentMessage> {
        let _ = (sdk, agent_id, token_budget);
        messages
    }

    /// ⓬ Token budget exceeded during assembly. Return compression actions.
    async fn on_budget_exceeded(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        let _ = (sdk, agent_id, snapshot);
        vec![CompressAction::SlidingWindow { keep_recent: 20 }]
    }

    /// ⓴ New message about to be stored. Decide keep/modify/skip/tag.
    async fn ingest(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        message: &mut AgentMessage,
    ) -> IngestAction {
        let _ = (sdk, agent_id, message);
        IngestAction::Keep
    }

    /// ㉑ Turn finished. Async post-processing (extract memory, update patterns).
    async fn after_turn(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) {
        let _ = (sdk, agent_id);
    }

    // =====================================================================
    // PHASE 2 & 7: Observation & evaluation
    // =====================================================================

    /// ❷ Agent execution started (per-turn). Pure observation.
    async fn on_agent_start(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) {
        let _ = (sdk, agent_id);
    }

    /// Agent execution ended. Audit, stats, cleanup.
    async fn on_agent_end(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        error: Option<&str>,
    ) {
        let _ = (sdk, agent_id, error);
    }

    /// ⓭ LLM returned a response. Observe raw output for quality evaluation.
    async fn on_llm_output(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        response: &AgentMessage,
    ) {
        let _ = (sdk, agent_id, response);
    }

    // =====================================================================
    // PHASE 5: Tool interception
    // =====================================================================

    /// ⓮ Before tool execution. Evaluate and optionally block.
    async fn before_tool_call(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> ToolCallAction {
        let _ = (sdk, agent_id, tool_name, tool_input);
        ToolCallAction::Allow
    }

    /// ⓯ After tool execution. Observe result and decide persistence strategy.
    async fn after_tool_call(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        tool_name: &str,
        result: &AgentMessage,
        result_tokens: usize,
    ) -> ToolResultAction {
        let _ = (sdk, agent_id, tool_name, result, result_tokens);
        ToolResultAction::Keep
    }

    // =====================================================================
    // PHASE 5: Sub-agent management
    // =====================================================================

    /// ⓰ Sub-agent about to spawn. Prepare its initial context.
    async fn on_sub_agent_spawn(
        &self,
        sdk: &dyn ContextManagementSDK,
        parent_id: &str,
        child_config: &serde_json::Value,
        task_description: &str,
    ) -> Vec<ContextEntry> {
        let _ = (sdk, parent_id, child_config, task_description);
        vec![]
    }

    /// ⓱ Sub-agent completed a turn. Observe progress, optionally steer or terminate.
    async fn on_sub_agent_turn(
        &self,
        sdk: &dyn ContextManagementSDK,
        parent_id: &str,
        child_id: &str,
        turn_index: usize,
        turn_summary: &str,
    ) -> SubAgentDirective {
        let _ = (sdk, parent_id, child_id, turn_index, turn_summary);
        SubAgentDirective::Continue
    }

    /// ⓲ Sub-agent is about to call a tool. Parent can observe and block.
    async fn on_sub_agent_tool_call(
        &self,
        sdk: &dyn ContextManagementSDK,
        parent_id: &str,
        child_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> ToolCallAction {
        let _ = (sdk, parent_id, child_id, tool_name, tool_input);
        ToolCallAction::Allow
    }

    /// ⓳ Sub-agent finished. Decide how results flow back to parent.
    async fn on_sub_agent_complete(
        &self,
        sdk: &dyn ContextManagementSDK,
        parent_id: &str,
        child_id: &str,
        result: &str,
        result_tokens: usize,
    ) -> InjectionPlan {
        let _ = (sdk, parent_id, child_id, result, result_tokens);
        InjectionPlan::FullResult
    }
}
