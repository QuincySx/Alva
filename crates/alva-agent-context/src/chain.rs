// INPUT:  std::sync::Arc, async_trait, crate::plugin (ContextHooks, ContextError), crate::sdk::ContextHandle, crate::types
// OUTPUT: pub struct ContextHooksChain
// POS:    Composite plugin that chains multiple ContextHooks implementations into a pipeline with additive/pipeline/veto semantics.
//! ContextHooksChain — run multiple plugins in pipeline order.
//!
//! Hook semantics:
//! - **Additive** (bootstrap, on_message, on_budget_exceeded, after_turn): results collected from all plugins
//! - **Pipeline** (assemble): each plugin transforms the previous plugin's output
//! - **Pipeline + veto** (ingest): each plugin transforms, but any Skip terminates the chain

use std::sync::Arc;

use async_trait::async_trait;

use crate::plugin::{ContextError, ContextHooks};
use crate::sdk::ContextHandle;
use crate::types::*;

/// Chains multiple `ContextHooks` implementations.
///
/// Plugins execute in order — first in the list runs first.
/// UI can control priority by controlling insertion order.
///
/// Every plugin receives the same `ContextHandle`, so any plugin can
/// inspect the full original store state to undo decisions made by
/// earlier plugins in the chain.
pub struct ContextHooksChain {
    plugins: Vec<Arc<dyn ContextHooks>>,
}

impl ContextHooksChain {
    pub fn new(plugins: Vec<Arc<dyn ContextHooks>>) -> Self {
        Self { plugins }
    }

    pub fn push(&mut self, plugin: Arc<dyn ContextHooks>) {
        self.plugins.push(plugin);
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

#[async_trait]
impl ContextHooks for ContextHooksChain {
    fn name(&self) -> &str {
        "context-hooks-chain"
    }

    /// Additive: all plugins bootstrap.
    async fn bootstrap(
        &self,
        handle: &dyn ContextHandle,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        for p in &self.plugins {
            p.bootstrap(handle, agent_id).await?;
        }
        Ok(())
    }

    /// Additive: collect injections from all plugins.
    async fn on_message(
        &self,
        handle: &dyn ContextHandle,
        agent_id: &str,
        message: &alva_kernel_abi::AgentMessage,
    ) -> Vec<Injection> {
        let mut all = Vec::new();
        for p in &self.plugins {
            all.extend(p.on_message(handle, agent_id, message).await);
        }
        all
    }

    /// Additive: collect compression actions from all plugins.
    async fn on_budget_exceeded(
        &self,
        handle: &dyn ContextHandle,
        agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        let mut all = Vec::new();
        for p in &self.plugins {
            all.extend(p.on_budget_exceeded(handle, agent_id, snapshot).await);
        }
        all
    }

    /// Pipeline: each plugin transforms the previous plugin's output.
    /// Any plugin can inspect the original store state via `handle.snapshot()`.
    async fn assemble(
        &self,
        handle: &dyn ContextHandle,
        agent_id: &str,
        entries: Vec<ContextEntry>,
        token_budget: usize,
    ) -> Vec<ContextEntry> {
        let mut current = entries;
        for p in &self.plugins {
            current = p.assemble(handle, agent_id, current, token_budget).await;
        }
        current
    }

    /// Pipeline + veto: each plugin can modify or approve. Any Skip terminates.
    async fn ingest(
        &self,
        handle: &dyn ContextHandle,
        agent_id: &str,
        entry: &ContextEntry,
    ) -> IngestAction {
        let mut action = IngestAction::Keep;
        for p in &self.plugins {
            action = p.ingest(handle, agent_id, entry).await;
            if matches!(action, IngestAction::Skip) {
                break;
            }
        }
        action
    }

    /// Additive: all plugins run after-turn cleanup.
    async fn after_turn(
        &self,
        handle: &dyn ContextHandle,
        agent_id: &str,
    ) {
        for p in &self.plugins {
            p.after_turn(handle, agent_id).await;
        }
    }

    /// All plugins dispose in reverse order.
    async fn dispose(&self) -> Result<(), ContextError> {
        for p in self.plugins.iter().rev() {
            p.dispose().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ContextHooks;
    use alva_kernel_abi::{AgentMessage, ContentBlock, Message, MessageRole};

    struct InjectPlugin {
        skill_name: String,
    }

    #[async_trait]
    impl ContextHooks for InjectPlugin {
        fn name(&self) -> &str { "inject-plugin" }

        async fn on_message(
            &self, _handle: &dyn ContextHandle, _id: &str, _msg: &AgentMessage,
        ) -> Vec<Injection> {
            vec![Injection::skill(self.skill_name.clone(), "content".into())]
        }
    }

    struct SkipToolResults;

    #[async_trait]
    impl ContextHooks for SkipToolResults {
        fn name(&self) -> &str { "skip-tool-results" }

        async fn ingest(
            &self, _handle: &dyn ContextHandle, _id: &str, entry: &ContextEntry,
        ) -> IngestAction {
            if matches!(&entry.metadata.origin, EntryOrigin::Tool { .. }) {
                IngestAction::Skip
            } else {
                IngestAction::Keep
            }
        }
    }

    struct HalveAssembler;

    #[async_trait]
    impl ContextHooks for HalveAssembler {
        fn name(&self) -> &str { "halve-assembler" }

        async fn assemble(
            &self, _handle: &dyn ContextHandle, _id: &str,
            entries: Vec<ContextEntry>, _budget: usize,
        ) -> Vec<ContextEntry> {
            let len = entries.len();
            let keep = len / 2;
            entries.into_iter().skip(len - keep.max(1)).collect()
        }
    }

    fn make_sdk() -> crate::sdk_impl::ContextHandleImpl {
        let store = Arc::new(std::sync::Mutex::new(
            crate::store::ContextStore::new(100_000, 80_000),
        ));
        crate::sdk_impl::ContextHandleImpl::new(store)
    }

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: text.into(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 1000,
        })
    }

    fn make_entry(id: &str) -> ContextEntry {
        ContextEntry {
            id: id.into(),
            message: user_msg(id),
            metadata: ContextMetadata::new(ContextLayer::RuntimeInject),
        }
    }

    #[tokio::test]
    async fn test_on_message_additive() {
        let chain = ContextHooksChain::new(vec![
            Arc::new(InjectPlugin { skill_name: "rust".into() }),
            Arc::new(InjectPlugin { skill_name: "python".into() }),
        ]);
        let sdk = make_sdk();
        let msg = user_msg("hi");

        let injections = chain.on_message(&sdk, "a1", &msg).await;
        assert_eq!(injections.len(), 2); // both plugins contributed
    }

    #[tokio::test]
    async fn test_assemble_pipeline() {
        let chain = ContextHooksChain::new(vec![
            Arc::new(HalveAssembler),
            Arc::new(HalveAssembler),
        ]);
        let sdk = make_sdk();

        let entries: Vec<ContextEntry> = (0..8).map(|i| make_entry(&format!("e{}", i))).collect();
        let result = chain.assemble(&sdk, "a1", entries, 100_000).await;

        // 8 → halve → 4 → halve → 2
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_ingest_veto() {
        let chain = ContextHooksChain::new(vec![
            Arc::new(SkipToolResults), // will Skip tool results
        ]);
        let sdk = make_sdk();

        // Tool entry → should be skipped
        let mut tool_entry = make_entry("tool-1");
        tool_entry.metadata.origin = EntryOrigin::Tool { tool_name: "read_file".into() };
        let action = chain.ingest(&sdk, "a1", &tool_entry).await;
        assert!(matches!(action, IngestAction::Skip));

        // User entry → should be kept
        let user_entry = make_entry("user-1");
        let action = chain.ingest(&sdk, "a1", &user_entry).await;
        assert!(matches!(action, IngestAction::Keep));
    }
}
