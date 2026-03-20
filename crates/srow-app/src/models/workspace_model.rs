use gpui::{Context, EventEmitter};

use crate::types::{Session, Workspace};

pub struct WorkspaceModel {
    pub workspaces: Vec<Workspace>,
    pub selected_workspace_id: Option<String>,
    pub sessions: Vec<Session>,
    pub selected_session_id: Option<String>,
}

pub enum WorkspaceModelEvent {
    WorkspaceSelected { workspace_id: String },
    SessionSelected { session_id: String },
}

impl EventEmitter<WorkspaceModelEvent> for WorkspaceModel {}

impl WorkspaceModel {
    pub fn select_workspace(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_workspace_id = Some(id.clone());
        self.sessions = mock_sessions_for(&id);
        self.selected_session_id = self.sessions.first().map(|s| s.id.clone());
        cx.emit(WorkspaceModelEvent::WorkspaceSelected {
            workspace_id: id,
        });
        cx.notify();
    }

    pub fn select_session(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_session_id = Some(id.clone());
        cx.emit(WorkspaceModelEvent::SessionSelected {
            session_id: id,
        });
        cx.notify();
    }
}

impl Default for WorkspaceModel {
    fn default() -> Self {
        let workspaces = mock_workspaces();
        let first_ws_id = workspaces.first().map(|w| w.id.clone());
        let sessions = first_ws_id
            .as_ref()
            .map(|id| mock_sessions_for(id))
            .unwrap_or_default();
        let first_session_id = sessions.first().map(|s| s.id.clone());

        Self {
            workspaces,
            selected_workspace_id: first_ws_id,
            sessions,
            selected_session_id: first_session_id,
        }
    }
}

fn mock_workspaces() -> Vec<Workspace> {
    vec![
        Workspace {
            id: "ws-1".into(),
            name: "Srow Agent".into(),
            path: "/Users/dev/srow-agent".into(),
            created_at: 1710900000000,
            updated_at: 1710900000000,
        },
        Workspace {
            id: "ws-2".into(),
            name: "Web Scraper".into(),
            path: "/Users/dev/web-scraper".into(),
            created_at: 1710800000000,
            updated_at: 1710800000000,
        },
    ]
}

fn mock_sessions_for(workspace_id: &str) -> Vec<Session> {
    match workspace_id {
        "ws-1" => vec![
            Session {
                id: "sess-1a".into(),
                workspace_id: "ws-1".into(),
                name: "Implement UI layout".into(),
                created_at: 1710900000000,
                updated_at: 1710900000000,
            },
            Session {
                id: "sess-1b".into(),
                workspace_id: "ws-1".into(),
                name: "Fix build errors".into(),
                created_at: 1710890000000,
                updated_at: 1710890000000,
            },
            Session {
                id: "sess-1c".into(),
                workspace_id: "ws-1".into(),
                name: "Code review".into(),
                created_at: 1710880000000,
                updated_at: 1710880000000,
            },
        ],
        "ws-2" => vec![
            Session {
                id: "sess-2a".into(),
                workspace_id: "ws-2".into(),
                name: "Scrape product data".into(),
                created_at: 1710800000000,
                updated_at: 1710800000000,
            },
            Session {
                id: "sess-2b".into(),
                workspace_id: "ws-2".into(),
                name: "Parse HTML tables".into(),
                created_at: 1710790000000,
                updated_at: 1710790000000,
            },
        ],
        _ => vec![],
    }
}
