// INPUT:  (no external crate dependencies)
// OUTPUT: pub struct Workspace, pub struct Session
// POS:    Defines workspace and session domain types used across the sidebar and chat subsystems.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub expanded: bool,
    pub sessions: Vec<Session>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub workspace_id: Option<String>,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Display text for task status (e.g. "已完成", "运行中")
    pub status_text: Option<String>,
    /// Display text for task duration (e.g. "6m", "21h")
    pub duration_text: Option<String>,
}
