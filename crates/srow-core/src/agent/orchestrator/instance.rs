// INPUT:  serde, uuid
// OUTPUT: AgentInstanceStatus, AgentInstance
// POS:    Runtime Agent instance with lifecycle state machine (Idle/Running/WaitingForHuman/Completed/Error/Cancelled).
use serde::{Deserialize, Serialize};

/// Agent instance status in its lifecycle
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstanceStatus {
    /// Created but not yet started
    Idle,
    /// Currently executing a task
    Running,
    /// Paused, waiting for human input
    WaitingForHuman,
    /// Successfully completed its task
    Completed,
    /// Failed with an error
    Error,
    /// Cancelled by orchestrator or user
    Cancelled,
}

/// A running Agent instance — a runtime instantiation of an `OrchestratorAgentTemplate`.
///
/// Each instance has its own session (isolated conversation context) and tracks
/// the task it was assigned by the brain Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    /// Unique instance ID
    pub id: String,
    /// Template this instance was created from
    pub template_id: String,
    /// Session ID (each instance gets its own isolated session)
    pub session_id: String,
    /// Current status
    pub status: AgentInstanceStatus,
    /// Task description assigned to this instance
    pub task: String,
    /// Result output (populated when Completed)
    pub result: Option<String>,
    /// Error message (populated when Error)
    pub error: Option<String>,
}

impl AgentInstance {
    /// Create a new idle agent instance
    pub fn new(template_id: impl Into<String>, task: impl Into<String>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();
        Self {
            id,
            template_id: template_id.into(),
            session_id,
            status: AgentInstanceStatus::Idle,
            task: task.into(),
            result: None,
            error: None,
        }
    }

    /// Transition to Running
    pub fn start(&mut self) {
        self.status = AgentInstanceStatus::Running;
    }

    /// Transition to Completed with a result
    pub fn complete(&mut self, result: String) {
        self.status = AgentInstanceStatus::Completed;
        self.result = Some(result);
    }

    /// Transition to Error
    pub fn fail(&mut self, error: String) {
        self.status = AgentInstanceStatus::Error;
        self.error = Some(error);
    }

    /// Transition to Cancelled
    pub fn cancel(&mut self) {
        self.status = AgentInstanceStatus::Cancelled;
    }

    /// Check if the instance is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            AgentInstanceStatus::Completed
                | AgentInstanceStatus::Error
                | AgentInstanceStatus::Cancelled
        )
    }
}
