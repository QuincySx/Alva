// INPUT:  serde
// OUTPUT: SessionStatus, Session
// POS:    Defines the session entity with lifecycle status tracking.
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Running,
    WaitingForHuman,
    Completed,
    Cancelled,
    Error,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workspace: String,
    pub agent_config_snapshot: serde_json::Value,
    pub status: SessionStatus,
    pub total_tokens: u32,
    pub iteration_count: u32,
}
