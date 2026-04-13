//! SQLite persistence for run records and logs.
//!
//! Single table, record + logs stored as JSON blobs.
//! DB file: `alva-eval-runs.db` in the current directory.

use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::log_capture::LogEntry;
use crate::recorder::RunRecord;

pub struct RunStore {
    conn: Mutex<Connection>,
}

/// Summary returned by list_runs (mirrors the API type).
#[derive(serde::Serialize)]
pub struct StoredRunSummary {
    pub run_id: String,
    pub model_id: String,
    pub turns: usize,
    pub total_tokens: u64,
    pub duration_ms: u64,
    pub created_at: String,
}

impl RunStore {
    pub fn open(path: &str) -> Self {
        let conn = Connection::open(path).expect("failed to open SQLite DB");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS runs (
                run_id     TEXT PRIMARY KEY,
                model_id   TEXT NOT NULL,
                turns      INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                record     TEXT NOT NULL,
                logs       TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"
        ).expect("failed to create runs table");
        Self { conn: Mutex::new(conn) }
    }

    /// Save a completed run.
    pub fn save(&self, run_id: &str, record: &RunRecord, logs: &[LogEntry]) {
        let conn = self.conn.lock().unwrap();
        let record_json = serde_json::to_string(record).unwrap_or_default();
        let logs_json = serde_json::to_string(logs).unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO runs (run_id, model_id, turns, total_tokens, duration_ms, record, logs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run_id,
                record.config_snapshot.model_id,
                record.turns.len() as i64,
                (record.total_input_tokens + record.total_output_tokens) as i64,
                record.total_duration_ms as i64,
                record_json,
                logs_json,
            ],
        ).expect("failed to save run");
    }

    /// List all runs (summary only, no full record).
    pub fn list(&self) -> Vec<StoredRunSummary> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT run_id, model_id, turns, total_tokens, duration_ms, created_at FROM runs ORDER BY created_at DESC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(StoredRunSummary {
                run_id: row.get(0)?,
                model_id: row.get(1)?,
                turns: row.get::<_, i64>(2)? as usize,
                total_tokens: row.get::<_, i64>(3)? as u64,
                duration_ms: row.get::<_, i64>(4)? as u64,
                created_at: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Get a full run record.
    pub fn get_record(&self, run_id: &str) -> Option<RunRecord> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT record FROM runs WHERE run_id = ?1",
            params![run_id],
            |row| {
                let json: String = row.get(0)?;
                Ok(serde_json::from_str(&json).ok())
            },
        )
        .ok()
        .flatten()
    }

    /// Get logs for a run.
    pub fn get_logs(&self, run_id: &str) -> Vec<LogEntry> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT logs FROM runs WHERE run_id = ?1",
            params![run_id],
            |row| {
                let json: String = row.get(0)?;
                Ok(serde_json::from_str(&json).unwrap_or_default())
            },
        )
        .unwrap_or_default()
    }

    /// Delete a run.
    pub fn delete(&self, run_id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM runs WHERE run_id = ?1", params![run_id])
            .map(|n| n > 0)
            .unwrap_or(false)
    }
}
