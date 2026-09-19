// INPUT:  workspace paths chosen by the user, display names
// OUTPUT: durable Project aggregate — a working directory plus its sessions
// POS:    H04. A Project exists independently of whether it has sessions yet;
//       deleting a Project detaches sessions but never touches the
//         filesystem or the sessions' own workspace rows. Migration 18 is
//         purely additive (new table + nullable column), so pre-18 readers
//         refuse cleanly via the schema-version gate instead of corrupting.

use rusqlite::{params, OptionalExtension};

use super::{now_unix_ms, HubStore};

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ProjectSummary {
    pub project_id: String,
    pub display_name: String,
    /// The path exactly as the user added it (UI-facing).
    pub workspace_path: String,
    /// Canonical absolute path used for identity/deduplication.
    pub workspace_canonical: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_count: i64,
}

impl HubStore {
    /// Register a working directory as a Project. Idempotent on the canonical
    /// path: adding the same directory twice (alias, symlink, trailing
    /// slash) returns the existing project instead of duplicating it. The
    /// directory must exist right now — Hub never creates or guesses user
    /// directories. Non-Git directories are accepted (worktree discovery
    /// reports "not a git repository" as a state, not an error).
    pub fn create_project(
        &self,
        workspace_path: &str,
        display_name: Option<&str>,
    ) -> Result<ProjectSummary, String> {
        let trimmed = workspace_path.trim();
        if trimmed.is_empty() {
            return Err("project workspace path must not be empty".into());
        }
        let path = std::path::Path::new(trimmed);
        // User-chosen directories may legitimately be symlinks (mounted
        // volumes, ~/dev aliases): identity is the canonical target, decided
        // by fs::metadata (follows links) + canonicalize below. The
        // symlink-refusing rule stays with HUB-managed directories.
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("project directory does not exist: {error}"))?;
        if !metadata.is_dir() {
            return Err("project workspace must be a directory".into());
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("resolve project directory: {error}"))?
            .to_string_lossy()
            .into_owned();
        let display = display_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| trimmed.to_string())
            });

        let connection = self.lock()?;
        // Dedupe by canonical path (aliases converge).
        if let Some(existing) = query_project_by_canonical(&connection, &canonical)? {
            return Ok(existing);
        }
        let now = now_unix_ms();
        let project_id = format!("project-{}", uuid::Uuid::new_v4());
        connection
            .execute(
                "INSERT INTO projects
                   (project_id, display_name, workspace_path, workspace_canonical,
                    created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    project_id,
                    bounded_name(&display),
                    trimmed,
                    canonical,
                    now
                ],
            )
            .map_err(|error| format!("insert project: {error}"))?;
        query_project(&connection, &project_id)?
            .ok_or_else(|| "project vanished immediately after insert".to_string())
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT p.project_id, p.display_name, p.workspace_path,
                        p.workspace_canonical, p.created_at, p.updated_at,
                        (SELECT COUNT(*) FROM hub_sessions hs
                          WHERE hs.project_id = p.project_id)
                   FROM projects p
                  ORDER BY p.created_at ASC",
            )
            .map_err(|error| format!("prepare project list: {error}"))?;
        let rows = statement
            .query_map([], decode_project)
            .map_err(|error| format!("query projects: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read projects: {error}"))
    }

    pub fn rename_project(&self, project_id: &str, display_name: &str) -> Result<(), String> {
        let trimmed = display_name.trim();
        if trimmed.is_empty() {
            return Err("project display name must not be empty".into());
        }
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE projects SET display_name = ?2, updated_at = ?3
                  WHERE project_id = ?1",
                params![project_id, bounded_name(trimmed), now_unix_ms()],
            )
            .map_err(|error| format!("rename project: {error}"))?;
        if changed == 0 {
            return Err(format!("unknown project '{project_id}'"));
        }
        Ok(())
    }

    /// Detach every session, then forget the Project. Sessions keep their own
    /// workspace path and history; the directory on disk is untouched.
    pub fn delete_project(&self, project_id: &str) -> Result<(), String> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE hub_sessions SET project_id = NULL WHERE project_id = ?1",
                params![project_id],
            )
            .map_err(|error| format!("detach project sessions: {error}"))?;
        let changed = connection
            .execute(
                "DELETE FROM projects WHERE project_id = ?1",
                params![project_id],
            )
            .map_err(|error| format!("delete project: {error}"))?;
        if changed == 0 {
            return Err(format!("unknown project '{project_id}'"));
        }
        Ok(())
    }

    /// Attach or detach one session. Attaching requires the project to exist;
    /// the session's own workspace/cwd rows are never rewritten.
    pub fn set_session_project(
        &self,
        product_session_id: &str,
        project_id: Option<&str>,
    ) -> Result<(), String> {
        let connection = self.lock()?;
        if let Some(id) = project_id {
            let known: Option<String> = connection
                .query_row(
                    "SELECT project_id FROM projects WHERE project_id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| format!("query project '{id}': {error}"))?;
            if known.is_none() {
                return Err(format!("unknown project '{id}'"));
            }
        }
        let changed = connection
            .execute(
                "UPDATE hub_sessions SET project_id = ?2, updated_at = ?3
                  WHERE product_session_id = ?1",
                params![product_session_id, project_id, now_unix_ms()],
            )
            .map_err(|error| format!("attach session to project: {error}"))?;
        if changed == 0 {
            return Err(format!("unknown Hub session '{product_session_id}'"));
        }
        Ok(())
    }
}

fn bounded_name(name: &str) -> String {
    name.chars().take(200).collect()
}

fn query_project_by_canonical(
    connection: &rusqlite::Connection,
    canonical: &str,
) -> Result<Option<ProjectSummary>, String> {
    let mut statement = connection
        .prepare(
            "SELECT p.project_id, p.display_name, p.workspace_path,
                    p.workspace_canonical, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM hub_sessions hs
                      WHERE hs.project_id = p.project_id)
               FROM projects p
              WHERE p.workspace_canonical = ?1",
        )
        .map_err(|error| format!("prepare project lookup: {error}"))?;
    statement
        .query_row([canonical], decode_project)
        .optional()
        .map_err(|error| format!("query project by path: {error}"))
}

fn query_project(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> Result<Option<ProjectSummary>, String> {
    let mut statement = connection
        .prepare(
            "SELECT p.project_id, p.display_name, p.workspace_path,
                    p.workspace_canonical, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM hub_sessions hs
                      WHERE hs.project_id = p.project_id)
               FROM projects p
              WHERE p.project_id = ?1",
        )
        .map_err(|error| format!("prepare project query: {error}"))?;
    statement
        .query_row([project_id], decode_project)
        .optional()
        .map_err(|error| format!("query project '{project_id}': {error}"))
}

fn decode_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSummary> {
    Ok(ProjectSummary {
        project_id: row.get(0)?,
        display_name: row.get(1)?,
        workspace_path: row.get(2)?,
        workspace_canonical: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        session_count: row.get(6)?,
    })
}
