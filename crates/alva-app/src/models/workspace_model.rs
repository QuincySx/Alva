// INPUT:  gpui (Context, EventEmitter), crate::types (Session, Workspace)
// OUTPUT: pub enum SidebarItem, pub struct WorkspaceModel, pub enum WorkspaceModelEvent
// POS:    GPUI model managing sidebar items (workspaces and global sessions), selection, and expand/collapse state.
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
        tracing::info!(session_id = %id, "model_event: session_selected");
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
        // Global sessions (tasks)
        SidebarItem::GlobalSession(Session {
            id: "sess-g1".into(),
            workspace_id: None,
            name: "项目运行环境搭建".into(),
            created_at: 1710950000000,
            updated_at: 1710950000000,
            status_text: Some("已完成".into()),
            duration_text: Some("6m".into()),
        }),
        SidebarItem::GlobalSession(Session {
            id: "sess-g2".into(),
            workspace_id: None,
            name: "英语四级核心词汇整理".into(),
            created_at: 1710940000000,
            updated_at: 1710940000000,
            status_text: Some("已完成".into()),
            duration_text: Some("21h".into()),
        }),
        SidebarItem::GlobalSession(Session {
            id: "sess-g3".into(),
            workspace_id: None,
            name: "数据分析报告生成".into(),
            created_at: 1710930000000,
            updated_at: 1710930000000,
            status_text: Some("运行中".into()),
            duration_text: Some("3m".into()),
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
                    name: "重构引擎架构".into(),
                    created_at: 1710900000000,
                    updated_at: 1710900000000,
                    status_text: Some("已完成".into()),
                    duration_text: Some("45m".into()),
                },
                Session {
                    id: "sess-1b".into(),
                    workspace_id: Some("ws-1".into()),
                    name: "修复 MCP 连接问题".into(),
                    created_at: 1710890000000,
                    updated_at: 1710890000000,
                    status_text: Some("已完成".into()),
                    duration_text: Some("12m".into()),
                },
            ],
            created_at: 1710900000000,
            updated_at: 1710920000000,
        }),
    ]
}
