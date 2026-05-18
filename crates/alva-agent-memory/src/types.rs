// INPUT:  serde
// OUTPUT: MemoryFile, MemoryChunk, MemoryEntry, SyncReport
// POS:    Domain types for the memory subsystem: tracked files, content chunks, search results, and sync reports.
//! Domain types for the memory subsystem.

use serde::{Deserialize, Serialize};

/// A tracked file in the memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFile {
    pub path: String,
    pub source: String,
    pub hash: String,
    pub mtime: String,
    pub size: i64,
}

/// A chunk of content derived from a memory file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: i64,
    pub path: String,
    pub source: String,
    pub start_line: i64,
    pub end_line: i64,
    pub hash: String,
    pub text: String,
    /// Embedding vector (empty if not yet computed).
    pub embedding: Vec<f32>,
}

/// A search result from the memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub chunk_id: i64,
    pub path: String,
    pub text: String,
    pub score: f64,
    pub start_line: i64,
    pub end_line: i64,
}

/// Report returned after a workspace sync operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_unchanged: usize,
    pub chunks_created: usize,
}

#[cfg(test)]
mod tests {
    //! Tests for SyncReport.
    //!
    //! SyncReport is returned to UI / CLI from sync operations; field
    //! name drift (e.g. `files_added` → `addedFiles`) silently breaks
    //! consumers. Default-zeros lets callers safely initialize and
    //! accumulate.
    //!
    //! MemoryFile / MemoryChunk / MemoryEntry are pure derive — testing
    //! their serde round-trip would just test the derive macro itself,
    //! so we skip them and concentrate on SyncReport (the only type
    //! here with Default + status-bar consumers).
    use super::*;
    use serde_json::{json, Value};

    // -- Default ---------------------------------------------------------

    #[test]
    fn default_initializes_all_counters_to_zero() {
        let r = SyncReport::default();
        assert_eq!(r.files_scanned, 0);
        assert_eq!(r.files_added, 0);
        assert_eq!(r.files_updated, 0);
        assert_eq!(r.files_unchanged, 0);
        assert_eq!(r.chunks_created, 0);
    }

    // -- Serde shape (field names + presence) -----------------------------

    #[test]
    fn serializes_with_snake_case_field_names_all_present() {
        // Pin all 5 snake_case field names. A future rename_all
        // attribute or accidental drop would let consumers (UI status
        // bar, CLI sync report) silently lose stats.
        let r = SyncReport {
            files_scanned: 10,
            files_added: 2,
            files_updated: 1,
            files_unchanged: 7,
            chunks_created: 25,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({
                "files_scanned": 10,
                "files_added": 2,
                "files_updated": 1,
                "files_unchanged": 7,
                "chunks_created": 25,
            })
        );
    }

    #[test]
    fn deserializes_from_snake_case_object() {
        let v = json!({
            "files_scanned": 5,
            "files_added": 5,
            "files_updated": 0,
            "files_unchanged": 0,
            "chunks_created": 12,
        });
        let r: SyncReport = serde_json::from_value(v).unwrap();
        assert_eq!(r.files_scanned, 5);
        assert_eq!(r.files_added, 5);
        assert_eq!(r.chunks_created, 12);
    }

    #[test]
    fn roundtrip_preserves_all_counter_values() {
        let original = SyncReport {
            files_scanned: 100,
            files_added: 30,
            files_updated: 20,
            files_unchanged: 50,
            chunks_created: 450,
        };
        let v = serde_json::to_value(&original).unwrap();
        let back: SyncReport = serde_json::from_value(v).unwrap();
        assert_eq!(back.files_scanned, 100);
        assert_eq!(back.files_added, 30);
        assert_eq!(back.files_updated, 20);
        assert_eq!(back.files_unchanged, 50);
        assert_eq!(back.chunks_created, 450);
    }

    #[test]
    fn serialized_default_is_all_zero_json() {
        // Pin: a fresh-zero SyncReport surfaces as all zeros in JSON
        // (no #[serde(skip_serializing_if = ...)] on counter fields
        // — UI relies on the keys being present even with value 0).
        let r = SyncReport::default();
        let v: Value = serde_json::to_value(&r).unwrap();
        for k in ["files_scanned", "files_added", "files_updated", "files_unchanged", "chunks_created"] {
            assert_eq!(v[k], json!(0), "{k} must be present and 0 even at default");
        }
    }
}
