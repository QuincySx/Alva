// INPUT:  async_trait, crate::plugin::ContextHooks, crate::sdk::ContextHandle, crate::types (ContextSnapshot, CompressAction, Priority)
// OUTPUT: pub struct RulesContextHooks
// POS:    Deterministic, zero-LLM-cost context plugin for development and fallback use.
//! RulesContextHooks — deterministic, zero-LLM-cost plugin for development and fallback.

use async_trait::async_trait;

use crate::plugin::ContextHooks;
use crate::sdk::ContextHandle;
use crate::types::*;

/// A pure-rules context plugin. No LLM calls, fully deterministic.
///
/// Use during development to verify the hooks pipeline works,
/// or as a fallback when the Agent-driven plugin is unavailable.
pub struct RulesContextHooks {
    /// Max conversation messages to keep (sliding window).
    pub max_messages: usize,
}

impl Default for RulesContextHooks {
    fn default() -> Self {
        Self { max_messages: 30 }
    }
}

#[async_trait]
impl ContextHooks for RulesContextHooks {
    fn name(&self) -> &str {
        "rules-context-plugin"
    }

    async fn on_budget_exceeded(
        &self,
        _sdk: &dyn ContextHandle,
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

#[cfg(test)]
mod tests {
    //! Tests for RulesContextHooks deterministic compression plugin.
    //!
    //! This is the fallback path when an Agent-driven plugin isn't
    //! available — so its decision ordering (RemoveByPriority::Disposable
    //! BEFORE SlidingWindow) and the max_messages threading are
    //! production-critical, but the whole thing is pure functions:
    //! no real LLM, no real storage.
    use super::*;
    use alva_kernel_abi::context::{
        ContextLayer, ContextSnapshot, EntryOrigin, EntrySnapshot, NoopContextHandle,
    };
    use std::collections::HashMap;

    // -- Construction defaults --------------------------------------------

    #[test]
    fn default_uses_thirty_message_sliding_window() {
        // Pin: documented default keeps last 30 messages. Changing
        // this without a deliberate decision silently shrinks /
        // expands every fallback session's retained history.
        let h = RulesContextHooks::default();
        assert_eq!(h.max_messages, 30);
    }

    #[test]
    fn name_returns_canonical_plugin_id() {
        // Bus / extension registries look up by this string — pin
        // so a refactor doesn't break the lookup silently.
        let h = RulesContextHooks::default();
        assert_eq!(h.name(), "rules-context-plugin");
    }

    // -- on_budget_exceeded decision logic --------------------------------

    fn make_entry(id: &str, priority: Priority) -> EntrySnapshot {
        EntrySnapshot {
            id: id.into(),
            layer: ContextLayer::RuntimeInject,
            priority,
            estimated_tokens: 100,
            origin: EntryOrigin::User,
            age_turns: 0,
            last_referenced_turns: None,
            preview: "".into(),
        }
    }

    fn make_snapshot(entries: Vec<EntrySnapshot>) -> ContextSnapshot {
        ContextSnapshot {
            total_tokens: 0,
            budget_tokens: 0,
            model_window: 0,
            usage_ratio: 0.0,
            layer_breakdown: HashMap::new(),
            entries,
            recent_tool_patterns: Vec::new(),
        }
    }

    #[tokio::test]
    async fn snapshot_with_disposable_emits_remove_then_sliding_window() {
        // Pin the decision ORDER: RemoveByPriority { Disposable }
        // first, SlidingWindow second. Plugin consumers apply actions
        // in order — flipping them means SlidingWindow first would
        // potentially drop messages that the Disposable cull could
        // have freed cheaply.
        let h = RulesContextHooks { max_messages: 30 };
        let snapshot = make_snapshot(vec![
            make_entry("a", Priority::Disposable),
            make_entry("b", Priority::Normal),
        ]);
        let handle: &dyn ContextHandle = &NoopContextHandle;
        let actions = h.on_budget_exceeded(handle, "agent-1", &snapshot).await;
        assert_eq!(actions.len(), 2);
        assert!(
            matches!(
                &actions[0],
                CompressAction::RemoveByPriority {
                    priority: Priority::Disposable
                }
            ),
            "first action must be RemoveByPriority::Disposable: {:?}",
            actions[0]
        );
        assert!(
            matches!(
                &actions[1],
                CompressAction::SlidingWindow { keep_recent: 30 }
            ),
            "second action must be SlidingWindow with the configured keep_recent: {:?}",
            actions[1]
        );
    }

    #[tokio::test]
    async fn snapshot_without_disposable_emits_only_sliding_window() {
        // Pin: skip the cheap Disposable cull when nothing's disposable,
        // go straight to SlidingWindow. A regression that always
        // emits both would wastefully RemoveByPriority a no-op step.
        let h = RulesContextHooks { max_messages: 30 };
        let snapshot = make_snapshot(vec![
            make_entry("a", Priority::Normal),
            make_entry("b", Priority::High),
            make_entry("c", Priority::Critical),
        ]);
        let handle: &dyn ContextHandle = &NoopContextHandle;
        let actions = h.on_budget_exceeded(handle, "agent-1", &snapshot).await;
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            CompressAction::SlidingWindow { keep_recent: 30 }
        ));
    }

    #[tokio::test]
    async fn empty_snapshot_still_emits_sliding_window() {
        // Pin: SlidingWindow always fires, even on empty entries.
        // Caller relies on this as the "default last-line" action.
        let h = RulesContextHooks::default();
        let snapshot = make_snapshot(vec![]);
        let handle: &dyn ContextHandle = &NoopContextHandle;
        let actions = h.on_budget_exceeded(handle, "agent-1", &snapshot).await;
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], CompressAction::SlidingWindow { .. }));
    }

    #[tokio::test]
    async fn sliding_window_uses_configured_max_messages() {
        // Pin keep_recent threading — user-configured max_messages
        // must flow into the SlidingWindow action verbatim.
        let h = RulesContextHooks { max_messages: 7 };
        let snapshot = make_snapshot(vec![]);
        let handle: &dyn ContextHandle = &NoopContextHandle;
        let actions = h.on_budget_exceeded(handle, "agent-1", &snapshot).await;
        match &actions[0] {
            CompressAction::SlidingWindow { keep_recent } => assert_eq!(*keep_recent, 7),
            other => panic!("expected SlidingWindow, got {other:?}"),
        }
    }
}
