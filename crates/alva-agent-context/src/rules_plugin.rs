// INPUT:  async_trait, crate::plugin::ContextPlugin, crate::sdk::ContextPluginSDK, crate::types (ContextSnapshot, CompressAction, Priority)
// OUTPUT: pub struct RulesContextPlugin
// POS:    Deterministic, zero-LLM-cost context plugin for development and fallback use.
//! RulesContextPlugin — deterministic, zero-LLM-cost plugin for development and fallback.

use async_trait::async_trait;

use crate::plugin::ContextPlugin;
use crate::sdk::ContextPluginSDK;
use crate::types::*;

/// A pure-rules context plugin. No LLM calls, fully deterministic.
///
/// Use during development to verify the hooks pipeline works,
/// or as a fallback when the Agent-driven plugin is unavailable.
pub struct RulesContextPlugin {
    /// Max conversation messages to keep (sliding window).
    pub max_messages: usize,
}

impl Default for RulesContextPlugin {
    fn default() -> Self {
        Self {
            max_messages: 30,
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
        _sdk: &dyn ContextPluginSDK,
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
}
