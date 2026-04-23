// INPUT:  std::sync::Arc, async_trait, serde_json, alva_kernel_abi::{SpawnCommunication, SpawnCommContext, SpawnCommHandle, SpawnCommError, OnChildComplete, SpawnResult}, super::{Blackboard, BlackboardPlugin, AgentProfile, BoardMessage, MessageKind}, crate::scope::BoardRegistry
// OUTPUT: BlackboardCommunication, BlackboardOnComplete
// POS:    SpawnCommunication impl for the shared Blackboard — attaches a BlackboardPlugin as a child ContextHooks and posts the child's final output back to the board.

//! `BlackboardCommunication` — wires the shared Blackboard into the new
//! `SpawnCommunication` plugin contract.
//!
//! Previously `AgentSpawnTool` had a hardcoded `board: Option<String>`
//! field. That field is gone: Blackboard is now one `SpawnCommunication`
//! implementation among many. Users opt in by registering this capability
//! with a `SpawnCommunicationRegistry` on the bus.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use alva_kernel_abi::context::ContextHooks;
use alva_kernel_abi::{
    OnChildComplete, SpawnCommContext, SpawnCommError, SpawnCommHandle, SpawnCommunication,
    SpawnResult,
};

use super::{AgentProfile, Blackboard, BlackboardPlugin, BoardMessage, MessageKind};
use crate::scope::BoardRegistry;

// ---------------------------------------------------------------------------
// BlackboardCommunication
// ---------------------------------------------------------------------------

/// `SpawnCommunication` that attaches a shared Blackboard to the child.
///
/// The `config` payload shape is `{ "board_id": "<string>" }`. Agents that
/// pass the same `board_id` (within the same spawn scope root) share a
/// board instance — that is how teammates see each other's messages.
pub struct BlackboardCommunication {
    registry: Arc<BoardRegistry>,
}

impl BlackboardCommunication {
    pub fn new(registry: Arc<BoardRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl SpawnCommunication for BlackboardCommunication {
    fn kind(&self) -> &str {
        "blackboard"
    }

    fn description(&self) -> &str {
        "Shared chat room — child joins a named board, sees team messages, \
         posts artifacts with @mentions. Use when multiple sub-agents need \
         to coordinate as a team."
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "board_id": {
                    "type": "string",
                    "description": "Board identifier — agents on same id share state."
                }
            },
            "required": ["board_id"]
        })
    }

    async fn attach(
        &self,
        ctx: &SpawnCommContext<'_>,
        config: Value,
    ) -> Result<SpawnCommHandle, SpawnCommError> {
        let board_id = config
            .get("board_id")
            .and_then(Value::as_str)
            .ok_or_else(|| SpawnCommError::InvalidConfig("missing 'board_id'".into()))?;

        // Scope keys the board to the parent spawn tree — teammates under
        // the same parent share a board; siblings in different trees do not.
        let board: Arc<Blackboard> = self
            .registry
            .get_or_create_by_str(ctx.parent_scope_id, board_id)
            .await;

        board
            .register(AgentProfile::new(ctx.role, ctx.role))
            .await;

        let plugin = BlackboardPlugin::new(
            AgentProfile::new(ctx.role, ctx.role),
            board.clone(),
        );

        let on_complete = Arc::new(BlackboardOnComplete {
            board: board.clone(),
            role: ctx.role.to_string(),
        });

        let hooks: Vec<Arc<dyn ContextHooks>> = vec![Arc::new(plugin)];
        Ok(SpawnCommHandle::with_hooks(hooks).with_on_complete(on_complete))
    }
}

// ---------------------------------------------------------------------------
// BlackboardOnComplete
// ---------------------------------------------------------------------------

/// Posts the child's final text output to the shared board as an Artifact.
struct BlackboardOnComplete {
    board: Arc<Blackboard>,
    role: String,
}

#[async_trait]
impl OnChildComplete for BlackboardOnComplete {
    async fn call(&self, result: &SpawnResult) {
        // Mirror the old `agent_spawn.rs` behaviour: even on error we keep
        // the produced text around under an `Artifact` so teammates can see
        // what happened.
        self.board
            .post(
                BoardMessage::new(&self.role, &result.text).with_kind(
                    MessageKind::Artifact {
                        name: format!("{}-output", self.role),
                    },
                ),
            )
            .await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx<'a>(
        parent: &'a str,
        child: &'a str,
        role: &'a str,
    ) -> SpawnCommContext<'a> {
        SpawnCommContext {
            parent_scope_id: parent,
            parent_session_id: "psess",
            child_scope_id: child,
            child_session_id: "csess",
            role,
            bus: None,
        }
    }

    #[tokio::test]
    async fn attach_returns_plugin_and_callback() {
        let registry = Arc::new(BoardRegistry::new());
        let comm = BlackboardCommunication::new(registry.clone());

        let handle = comm
            .attach(
                &make_ctx("parent-1", "child-1", "planner"),
                json!({ "board_id": "team" }),
            )
            .await
            .expect("attach");

        assert_eq!(handle.hooks.len(), 1);
        assert!(handle.on_complete.is_some());

        // Board is actually registered with the agent profile.
        let board = registry.get_or_create_by_str("parent-1", "team").await;
        assert!(board.profile("planner").await.is_some());
    }

    #[tokio::test]
    async fn attach_missing_board_id_errors() {
        let registry = Arc::new(BoardRegistry::new());
        let comm = BlackboardCommunication::new(registry);
        let result = comm.attach(&make_ctx("p", "c", "r"), json!({})).await;
        match result {
            Err(SpawnCommError::InvalidConfig(_)) => {}
            Err(other) => panic!("expected InvalidConfig, got {other:?}"),
            Ok(_) => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn on_complete_posts_artifact() {
        let registry = Arc::new(BoardRegistry::new());
        let comm = BlackboardCommunication::new(registry.clone());

        let handle = comm
            .attach(
                &make_ctx("parent-2", "child-2", "coder"),
                json!({ "board_id": "t" }),
            )
            .await
            .unwrap();

        let cb = handle.on_complete.unwrap();
        cb.call(&SpawnResult {
            text: "final answer".into(),
            is_error: false,
            error: None,
        })
        .await;

        let board = registry.get_or_create_by_str("parent-2", "t").await;
        let msgs = board.all_messages().await;
        assert!(msgs
            .iter()
            .any(|m| matches!(m.kind, MessageKind::Artifact { .. })
                && m.content.contains("final answer")));
    }
}
