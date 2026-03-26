//! RulesContextPlugin — deterministic, zero-LLM-cost plugin for development and fallback.

use async_trait::async_trait;

use crate::plugin::ContextPlugin;
use crate::sdk::ContextManagementSDK;
use crate::types::*;

/// A pure-rules context plugin. No LLM calls, fully deterministic.
///
/// Use during development to verify the hooks pipeline works,
/// or as a fallback when the Agent-driven plugin is unavailable.
pub struct RulesContextPlugin {
    /// Max conversation messages to keep (sliding window).
    pub max_messages: usize,
    /// Tool result token threshold for auto-truncation.
    pub large_result_threshold: usize,
    /// Sub-agent result token threshold for auto-truncation.
    pub sub_agent_result_threshold: usize,
}

impl Default for RulesContextPlugin {
    fn default() -> Self {
        Self {
            max_messages: 30,
            large_result_threshold: 5000,
            sub_agent_result_threshold: 2000,
        }
    }
}

#[async_trait]
impl ContextPlugin for RulesContextPlugin {
    fn name(&self) -> &str {
        "rules-context-plugin"
    }

    async fn on_budget_exceeded(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        let mut actions = Vec::new();

        // Step 1: Remove disposable entries
        let has_disposable = snapshot
            .entries
            .iter()
            .any(|e| e.priority == Priority::Disposable);
        if has_disposable {
            actions.push(CompressAction::RemoveByPriority {
                priority: Priority::Disposable,
            });
        }

        // Step 2: If still likely over budget, sliding window
        actions.push(CompressAction::SlidingWindow {
            keep_recent: self.max_messages,
        });

        actions
    }

    async fn after_tool_call(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        _tool_name: &str,
        _result: &alva_types::AgentMessage,
        result_tokens: usize,
    ) -> ToolResultAction {
        if result_tokens > self.large_result_threshold {
            ToolResultAction::Truncate { max_lines: 200 }
        } else {
            ToolResultAction::Keep
        }
    }

    async fn on_sub_agent_complete(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _parent_id: &str,
        _child_id: &str,
        result: &str,
        result_tokens: usize,
    ) -> InjectionPlan {
        if result_tokens > self.sub_agent_result_threshold {
            // Truncate long results
            let truncated: String = result.chars().take(2000).collect();
            InjectionPlan::Summary {
                text: format!("{}...[truncated from {} tokens]", truncated, result_tokens),
            }
        } else {
            InjectionPlan::FullResult
        }
    }

    async fn on_inject_file(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        _file_path: &str,
        content: String,
        content_tokens: usize,
    ) -> InjectDecision<String> {
        if content_tokens > 10000 {
            // Files over 10K tokens: truncate to first 2000 lines
            let truncated: String = content.lines().take(2000).collect::<Vec<_>>().join("\n");
            InjectDecision::Modify(format!(
                "{}\n\n[... truncated, original {} tokens]",
                truncated, content_tokens
            ))
        } else {
            InjectDecision::Allow(content)
        }
    }

    async fn on_inject_media(
        &self,
        _sdk: &dyn ContextManagementSDK,
        _agent_id: &str,
        _media_type: &str,
        _source: MediaSource,
        _size_bytes: usize,
        estimated_tokens: usize,
    ) -> InjectDecision<MediaAction> {
        if estimated_tokens > 3000 {
            // Large media: remove to save tokens
            InjectDecision::Reject {
                reason: format!(
                    "Media too large ({} estimated tokens), use a tool to analyze it instead",
                    estimated_tokens
                ),
            }
        } else {
            InjectDecision::Allow(MediaAction::Keep)
        }
    }
}
