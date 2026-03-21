// INPUT:  std::collections::HashMap, gpui (Context, EventEmitter), chrono, crate::types (AgentStatus, AgentStatusKind)
// OUTPUT: pub struct AgentModel, pub enum AgentModelEvent
// POS:    GPUI model tracking per-session agent status (Idle, Running, Error, etc.) and emitting change events.
use std::collections::HashMap;

use gpui::{Context, EventEmitter};

use crate::types::{AgentStatus, AgentStatusKind};

pub struct AgentModel {
    pub statuses: HashMap<String, AgentStatus>,
}

pub enum AgentModelEvent {
    StatusChanged { session_id: String },
}

impl EventEmitter<AgentModelEvent> for AgentModel {}

impl AgentModel {
    pub fn set_status(
        &mut self,
        session_id: &str,
        kind: AgentStatusKind,
        cx: &mut Context<Self>,
    ) {
        let status = AgentStatus {
            session_id: session_id.to_string(),
            kind,
            detail: None,
            updated_at: chrono::Utc::now().timestamp_millis(),
        };
        self.statuses.insert(session_id.to_string(), status);
        cx.emit(AgentModelEvent::StatusChanged {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn set_status_with_detail(
        &mut self,
        session_id: &str,
        kind: AgentStatusKind,
        detail: String,
        cx: &mut Context<Self>,
    ) {
        let status = AgentStatus {
            session_id: session_id.to_string(),
            kind,
            detail: Some(detail),
            updated_at: chrono::Utc::now().timestamp_millis(),
        };
        self.statuses.insert(session_id.to_string(), status);
        cx.emit(AgentModelEvent::StatusChanged {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn get_status(&self, session_id: &str) -> Option<&AgentStatus> {
        self.statuses.get(session_id)
    }
}

impl Default for AgentModel {
    fn default() -> Self {
        Self {
            statuses: HashMap::new(),
        }
    }
}
