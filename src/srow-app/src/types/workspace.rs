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
}
