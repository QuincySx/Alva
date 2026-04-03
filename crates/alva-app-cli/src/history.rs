use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub display: String,
    pub timestamp: u64,
    pub project: String,
    pub session_id: Option<String>,
}

pub struct History {
    path: PathBuf,
    pending: Vec<HistoryEntry>,
    project: String,
    session_id: String,
}

impl History {
    pub fn new(home_dir: &std::path::Path, project: String, session_id: String) -> Self {
        let path = home_dir.join(".alva").join("history.jsonl");
        Self {
            path,
            pending: Vec::new(),
            project,
            session_id,
        }
    }

    /// Add an entry to history
    pub fn add(&mut self, display: String) {
        let entry = HistoryEntry {
            display,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            project: self.project.clone(),
            session_id: Some(self.session_id.clone()),
        };
        self.pending.push(entry);
    }

    /// Flush pending entries to disk
    pub async fn flush(&mut self) -> Result<(), std::io::Error> {
        if self.pending.is_empty() {
            return Ok(());
        }

        // Ensure directory exists
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;

        for entry in self.pending.drain(..) {
            let line = serde_json::to_string(&entry).unwrap_or_default();
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        file.flush().await?;
        Ok(())
    }

    /// Load history entries (most recent first, max 100)
    pub async fn load(&self) -> Vec<HistoryEntry> {
        let content = match tokio::fs::read_to_string(&self.path).await {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut entries: Vec<HistoryEntry> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        // Most recent first
        entries.reverse();

        // Deduplicate by display text
        let mut seen = std::collections::HashSet::new();
        entries.retain(|e| seen.insert(e.display.clone()));

        // Limit to 100
        entries.truncate(100);
        entries
    }

    /// Remove the last entry
    pub fn remove_last(&mut self) {
        self.pending.pop();
    }
}
