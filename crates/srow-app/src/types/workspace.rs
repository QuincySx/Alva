#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}
