//! File-based checkpoint manager for code rollback.
//!
//! Layout:
//!   .alva/checkpoints/
//!   └── {checkpoint_id}/
//!       ├── meta.json
//!       └── files/
//!           └── {relative_path}  (backup of original file before modification)

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub id: String,
    pub created_at: i64,
    pub description: String,
    pub files: Vec<String>,
}

pub struct CheckpointManager {
    checkpoints_dir: PathBuf,
    workspace: PathBuf,
}

impl CheckpointManager {
    pub fn new(workspace: &Path) -> Self {
        Self {
            checkpoints_dir: workspace.join(".alva").join("checkpoints"),
            workspace: workspace.to_path_buf(),
        }
    }

    /// Create a checkpoint for the given files (saves their current content).
    /// Returns the checkpoint ID.
    pub fn create(
        &self,
        description: &str,
        file_paths: &[PathBuf],
    ) -> std::io::Result<String> {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let checkpoint_dir = self.checkpoints_dir.join(&id);
        let files_dir = checkpoint_dir.join("files");
        fs::create_dir_all(&files_dir)?;

        let mut saved_files = Vec::new();

        for path in file_paths {
            let abs_path = if path.is_absolute() {
                path.clone()
            } else {
                self.workspace.join(path)
            };

            if abs_path.exists() {
                // Compute relative path from workspace
                let rel = abs_path
                    .strip_prefix(&self.workspace)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();

                let backup_path = files_dir.join(&rel);
                if let Some(parent) = backup_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&abs_path, &backup_path)?;
                saved_files.push(rel);
            }
        }

        let meta = CheckpointMeta {
            id: id.clone(),
            created_at: chrono::Utc::now().timestamp_millis(),
            description: description.to_string(),
            files: saved_files,
        };
        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(checkpoint_dir.join("meta.json"), meta_json)?;

        Ok(id)
    }

    /// List all checkpoints, most recent first.
    pub fn list(&self) -> Vec<CheckpointMeta> {
        let mut checkpoints = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.checkpoints_dir) {
            for entry in entries.flatten() {
                let meta_path = entry.path().join("meta.json");
                if let Ok(data) = fs::read_to_string(&meta_path) {
                    if let Ok(meta) = serde_json::from_str::<CheckpointMeta>(&data) {
                        checkpoints.push(meta);
                    }
                }
            }
        }
        checkpoints.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        checkpoints
    }

    /// Rewind: restore files from a checkpoint.
    /// Returns the list of restored file paths.
    pub fn rewind(&self, checkpoint_id: &str) -> std::io::Result<Vec<String>> {
        let checkpoint_dir = self.checkpoints_dir.join(checkpoint_id);
        let meta_path = checkpoint_dir.join("meta.json");
        let data = fs::read_to_string(&meta_path)?;
        let meta: CheckpointMeta = serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let files_dir = checkpoint_dir.join("files");
        let mut restored = Vec::new();

        for rel_path in &meta.files {
            let backup = files_dir.join(rel_path);
            let target = self.workspace.join(rel_path);
            if backup.exists() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&backup, &target)?;
                restored.push(rel_path.clone());
            }
        }

        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_and_list_checkpoint() {
        let tmp = tempdir().unwrap();
        let workspace = tmp.path();
        let mgr = CheckpointManager::new(workspace);

        // Create a file to checkpoint
        let file = workspace.join("test.txt");
        fs::write(&file, "original content").unwrap();

        let id = mgr.create("before edit", &[file.clone()]).unwrap();
        let list = mgr.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].description, "before edit");
        assert_eq!(list[0].files, vec!["test.txt"]);
    }

    #[test]
    fn rewind_restores_files() {
        let tmp = tempdir().unwrap();
        let workspace = tmp.path();
        let mgr = CheckpointManager::new(workspace);

        let file = workspace.join("test.txt");
        fs::write(&file, "original").unwrap();

        let id = mgr.create("before edit", &[file.clone()]).unwrap();

        // Modify the file
        fs::write(&file, "modified").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "modified");

        // Rewind
        let restored = mgr.rewind(&id).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn list_empty() {
        let tmp = tempdir().unwrap();
        let mgr = CheckpointManager::new(tmp.path());
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn multiple_checkpoints_ordered() {
        let tmp = tempdir().unwrap();
        let workspace = tmp.path();
        let mgr = CheckpointManager::new(workspace);

        let file = workspace.join("test.txt");
        fs::write(&file, "v1").unwrap();
        let _id1 = mgr.create("checkpoint 1", &[file.clone()]).unwrap();

        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        fs::write(&file, "v2").unwrap();
        let id2 = mgr.create("checkpoint 2", &[file.clone()]).unwrap();

        let list = mgr.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2, "most recent should be first");
    }

    #[test]
    fn rewind_nested_files() {
        let tmp = tempdir().unwrap();
        let workspace = tmp.path();
        let mgr = CheckpointManager::new(workspace);

        // Create nested file
        let nested_dir = workspace.join("src").join("lib");
        fs::create_dir_all(&nested_dir).unwrap();
        let file = nested_dir.join("mod.rs");
        fs::write(&file, "original mod").unwrap();

        let id = mgr.create("before refactor", &[file.clone()]).unwrap();

        fs::write(&file, "refactored mod").unwrap();

        let restored = mgr.rewind(&id).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "original mod");
    }
}
