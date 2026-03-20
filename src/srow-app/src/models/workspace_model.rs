use gpui::{Context, EventEmitter};

use crate::types::{Session, Workspace};

/// A sidebar item is either a global session (no workspace) or a workspace node.
#[derive(Debug, Clone)]
pub enum SidebarItem {
    GlobalSession(Session),
    Workspace(Workspace),
}

impl SidebarItem {
    pub fn updated_at(&self) -> i64 {
        match self {
            SidebarItem::GlobalSession(s) => s.updated_at,
            SidebarItem::Workspace(w) => w.updated_at,
        }
    }
}

pub struct WorkspaceModel {
    pub sidebar_items: Vec<SidebarItem>,
    pub selected_session_id: Option<String>,
}

pub enum WorkspaceModelEvent {
    SessionSelected { session_id: String },
}

impl EventEmitter<WorkspaceModelEvent> for WorkspaceModel {}

impl WorkspaceModel {
    pub fn select_session(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_session_id = Some(id.clone());
        cx.emit(WorkspaceModelEvent::SessionSelected {
            session_id: id,
        });
        cx.notify();
    }

    pub fn toggle_workspace(&mut self, workspace_id: String, cx: &mut Context<Self>) {
        for item in &mut self.sidebar_items {
            if let SidebarItem::Workspace(ws) = item {
                if ws.id == workspace_id {
                    ws.expanded = !ws.expanded;
                    break;
                }
            }
        }
        cx.notify();
    }
}

impl Default for WorkspaceModel {
    fn default() -> Self {
        let mut items = mock_sidebar_items();
        // Sort by updated_at descending (most recent first)
        items.sort_by(|a, b| b.updated_at().cmp(&a.updated_at()));

        // Default select first global session
        let first_session_id = items.iter().find_map(|item| {
            if let SidebarItem::GlobalSession(s) = item {
                Some(s.id.clone())
            } else {
                None
            }
        });

        Self {
            sidebar_items: items,
            selected_session_id: first_session_id,
        }
    }
}

fn mock_sidebar_items() -> Vec<SidebarItem> {
    vec![
        // Global sessions
        SidebarItem::GlobalSession(Session {
            id: "sess-g1".into(),
            workspace_id: None,
            name: "查天气".into(),
            created_at: 1710950000000,
            updated_at: 1710950000000,
        }),
        SidebarItem::GlobalSession(Session {
            id: "sess-g2".into(),
            workspace_id: None,
            name: "邮件助手".into(),
            created_at: 1710940000000,
            updated_at: 1710940000000,
        }),
        // Workspace: srow-agent
        SidebarItem::Workspace(Workspace {
            id: "ws-1".into(),
            name: "srow-agent".into(),
            path: "/Users/dev/srow-agent".into(),
            expanded: true,
            sessions: vec![
                Session {
                    id: "sess-1a".into(),
                    workspace_id: Some("ws-1".into()),
                    name: "重构引擎".into(),
                    created_at: 1710900000000,
                    updated_at: 1710900000000,
                },
                Session {
                    id: "sess-1b".into(),
                    workspace_id: Some("ws-1".into()),
                    name: "修复 bug".into(),
                    created_at: 1710890000000,
                    updated_at: 1710890000000,
                },
                Session {
                    id: "sess-1c".into(),
                    workspace_id: Some("ws-1".into()),
                    name: "写测试".into(),
                    created_at: 1710880000000,
                    updated_at: 1710880000000,
                },
            ],
            created_at: 1710900000000,
            updated_at: 1710920000000,
        }),
        // Workspace: web-app
        SidebarItem::Workspace(Workspace {
            id: "ws-2".into(),
            name: "web-app".into(),
            path: "/Users/dev/web-app".into(),
            expanded: false,
            sessions: vec![
                Session {
                    id: "sess-2a".into(),
                    workspace_id: Some("ws-2".into()),
                    name: "首页设计".into(),
                    created_at: 1710800000000,
                    updated_at: 1710800000000,
                },
                Session {
                    id: "sess-2b".into(),
                    workspace_id: Some("ws-2".into()),
                    name: "API 对接".into(),
                    created_at: 1710790000000,
                    updated_at: 1710790000000,
                },
            ],
            created_at: 1710800000000,
            updated_at: 1710810000000,
        }),
    ]
}
