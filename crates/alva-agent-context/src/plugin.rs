// INPUT:  alva_types::AgentMessage, async_trait, crate::sdk::ContextHooksSDK, crate::types (ContextSnapshot, CompressAction, ContextEntry, Injection, IngestAction)
// OUTPUT: pub enum ContextError, pub trait ContextHooks
// POS:    Defines the 8-hook ContextHooks trait that plugins implement to control the context lifecycle.
//! ContextHooks trait — 8 hooks covering the context lifecycle.
//!
//! All methods have default no-op implementations. Plugins only override what they need.

use alva_types::AgentMessage;
use async_trait::async_trait;

use crate::sdk::ContextHooksSDK;
use crate::types::*;

/// Error type for context plugin operations.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Other(String),
}

#[async_trait]
pub trait ContextHooks: Send + Sync {
    fn name(&self) -> &str { std::any::type_name::<Self>() }

    async fn bootstrap(&self, sdk: &dyn ContextHooksSDK, agent_id: &str) -> Result<(), ContextError> {
        let _ = (sdk, agent_id); Ok(())
    }

    async fn on_message(&self, sdk: &dyn ContextHooksSDK, agent_id: &str, message: &AgentMessage) -> Vec<Injection> {
        let _ = (sdk, agent_id, message); vec![]
    }

    async fn on_budget_exceeded(&self, sdk: &dyn ContextHooksSDK, agent_id: &str, snapshot: &ContextSnapshot) -> Vec<CompressAction> {
        let _ = (sdk, agent_id, snapshot); vec![CompressAction::SlidingWindow { keep_recent: 20 }]
    }

    async fn assemble(&self, sdk: &dyn ContextHooksSDK, agent_id: &str, entries: Vec<ContextEntry>, token_budget: usize) -> Vec<ContextEntry> {
        let _ = (sdk, agent_id, token_budget); entries
    }

    async fn ingest(&self, sdk: &dyn ContextHooksSDK, agent_id: &str, entry: &ContextEntry) -> IngestAction {
        let _ = (sdk, agent_id, entry); IngestAction::Keep
    }

    async fn after_turn(&self, sdk: &dyn ContextHooksSDK, agent_id: &str) {
        let _ = (sdk, agent_id);
    }

    async fn dispose(&self) -> Result<(), ContextError> { Ok(()) }
}
