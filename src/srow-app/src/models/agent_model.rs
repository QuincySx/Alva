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

    pub fn get_status(&self, session_id: &str) -> Option<&AgentStatus> {
        self.statuses.get(session_id)
    }
}

impl Default for AgentModel {
    fn default() -> Self {
        let mut statuses = HashMap::new();
        // Pre-populate mock agent statuses
        statuses.insert(
            "sess-1a".to_string(),
            AgentStatus {
                session_id: "sess-1a".into(),
                kind: AgentStatusKind::Idle,
                detail: Some("Decision Agent".into()),
                updated_at: 1710900000000,
            },
        );
        statuses.insert(
            "sess-1b".to_string(),
            AgentStatus {
                session_id: "sess-1b".into(),
                kind: AgentStatusKind::Offline,
                detail: Some("Browser Agent".into()),
                updated_at: 1710890000000,
            },
        );
        statuses.insert(
            "sess-1c".to_string(),
            AgentStatus {
                session_id: "sess-1c".into(),
                kind: AgentStatusKind::Offline,
                detail: Some("Coding Agent".into()),
                updated_at: 1710880000000,
            },
        );
        Self { statuses }
    }
}
