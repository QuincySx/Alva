// INPUT:  std::path, crate::error, crate::{embedding, sqlite, sync, types}
// OUTPUT: MemoryService
// POS:    Unified memory service combining FTS + vector hybrid search with weighted score fusion.
//! MemoryService — unified entry point for memory CRUD + hybrid search.

use std::path::Path;

use crate::error::MemoryError;

use crate::embedding::EmbeddingProvider;
use crate::sqlite::MemorySqlite;
use crate::sync;
use crate::types::{MemoryEntry, SyncReport};

/// Weights for hybrid search scoring.
const FTS_WEIGHT: f64 = 0.4;
const VEC_WEIGHT: f64 = 0.6;

/// High-level memory service combining FTS + vector search.
pub struct MemoryService {
    store: MemorySqlite,
    embedder: Box<dyn EmbeddingProvider>,
}

impl MemoryService {
    /// Create a new `MemoryService` with the given SQLite store and embedding provider.
    pub fn new(store: MemorySqlite, embedder: Box<dyn EmbeddingProvider>) -> Self {
        Self { store, embedder }
    }

    /// Store a key-value memory entry.
    ///
    /// The `category` is used as the `source` field for the file record.
    /// Each entry is stored as a single-chunk file keyed by `key`.
    pub async fn store_entry(
        &self,
        key: &str,
        content: &str,
        category: &str,
    ) -> Result<(), MemoryError> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let hash = format!("{:016x}", hasher.finish());

        let file = crate::types::MemoryFile {
            path: key.to_string(),
            source: category.to_string(),
            hash: hash.clone(),
            mtime: chrono::Utc::now().to_rfc3339(),
            size: content.len() as i64,
        };

        // Delete old chunks for this key, then upsert.
        self.store.delete_chunks_for_path(key).await?;
        self.store.upsert_file(&file).await?;

        // Compute embedding
        let embeddings = self
            .embedder
            .embed(&[content.to_string()])
            .await
            .unwrap_or_else(|_| vec![Vec::new()]);
        let embedding = embeddings.into_iter().next().unwrap_or_default();

        // Cache embedding if non-empty
        if !embedding.is_empty() {
            let _ = self
                .store
                .cache_embedding(self.embedder.model(), &hash, &embedding)
                .await;
        }

        let line_count = content.lines().count().max(1) as i64;
        self.store
            .insert_chunk(key, category, 1, line_count, &hash, content, &embedding)
            .await?;

        Ok(())
    }

    /// Hybrid search: FTS keyword match + vector cosine similarity, weighted fusion.
    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        // Step 1: FTS search
        let fts_results = self.store.fts_search(query, max_results * 2).await?;

        // Step 2: Vector search (if embedder is available)
        let embeddings = self
            .embedder
            .embed(&[query.to_string()])
            .await
            .unwrap_or_else(|_| vec![Vec::new()]);
        let query_emb = embeddings.into_iter().next().unwrap_or_default();
        let vec_results = self
            .store
            .vector_search(&query_emb, max_results * 2)
            .await?;

        // Step 3: Merge results with weighted fusion
        let merged = merge_results(&fts_results, &vec_results, max_results);

        Ok(merged)
    }

    /// Synchronize workspace MEMORY.md files into the store.
    pub async fn sync_workspace(&self, workspace_path: &Path) -> Result<SyncReport, MemoryError> {
        sync::sync_workspace(workspace_path, &self.store, self.embedder.as_ref()).await
    }

    /// Direct access to the underlying store (for advanced queries).
    pub fn store(&self) -> &MemorySqlite {
        &self.store
    }
}

// ---------------------------------------------------------------------------
// Merge logic
// ---------------------------------------------------------------------------

fn merge_results(
    fts: &[MemoryEntry],
    vec: &[MemoryEntry],
    max_results: usize,
) -> Vec<MemoryEntry> {
    use std::collections::HashMap;

    // Normalize FTS scores: BM25 returns negative values (lower = better).
    // Convert to 0..1 range where 1 = best match.
    let fts_scores = normalize_scores_inverted(fts);
    let vec_scores = normalize_scores(vec);

    // Combine by chunk_id
    let mut combined: HashMap<i64, (MemoryEntry, f64)> = HashMap::new();

    for (entry, score) in fts.iter().zip(fts_scores.iter()) {
        combined
            .entry(entry.chunk_id)
            .or_insert_with(|| (entry.clone(), 0.0))
            .1 += score * FTS_WEIGHT;
    }

    for (entry, score) in vec.iter().zip(vec_scores.iter()) {
        combined
            .entry(entry.chunk_id)
            .and_modify(|(_, s)| *s += score * VEC_WEIGHT)
            .or_insert_with(|| (entry.clone(), score * VEC_WEIGHT));
    }

    let mut results: Vec<MemoryEntry> = combined
        .into_values()
        .map(|(mut entry, score)| {
            entry.score = score;
            entry
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(max_results);
    results
}

/// Normalize scores to [0, 1] range (higher is better).
fn normalize_scores(entries: &[MemoryEntry]) -> Vec<f64> {
    if entries.is_empty() {
        return Vec::new();
    }
    let max = entries
        .iter()
        .map(|e| e.score)
        .fold(f64::NEG_INFINITY, f64::max);
    let min = entries
        .iter()
        .map(|e| e.score)
        .fold(f64::INFINITY, f64::min);
    let range = max - min;
    if range == 0.0 {
        return vec![1.0; entries.len()];
    }
    entries.iter().map(|e| (e.score - min) / range).collect()
}

/// Normalize inverted scores (BM25: lower/more-negative = better).
fn normalize_scores_inverted(entries: &[MemoryEntry]) -> Vec<f64> {
    if entries.is_empty() {
        return Vec::new();
    }
    let max = entries
        .iter()
        .map(|e| e.score)
        .fold(f64::NEG_INFINITY, f64::max);
    let min = entries
        .iter()
        .map(|e| e.score)
        .fold(f64::INFINITY, f64::min);
    let range = max - min;
    if range == 0.0 {
        return vec![1.0; entries.len()];
    }
    // Invert: lowest raw score (most negative) gets highest normalized score.
    entries.iter().map(|e| (max - e.score) / range).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::NoopEmbeddingProvider;

    async fn make_service() -> MemoryService {
        let store = MemorySqlite::open_in_memory().await.unwrap();
        let embedder = Box::new(NoopEmbeddingProvider::new());
        MemoryService::new(store, embedder)
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let svc = make_service().await;

        svc.store_entry("key1", "Rust is a systems programming language", "note")
            .await
            .unwrap();
        svc.store_entry("key2", "Python is popular for machine learning", "note")
            .await
            .unwrap();
        svc.store_entry("key3", "JavaScript runs in the browser", "note")
            .await
            .unwrap();

        // FTS-only search (since embedder is noop)
        let results = svc.search("Rust programming", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results[0].text.contains("Rust"));
    }

    #[tokio::test]
    async fn test_store_entry_overwrite() {
        let svc = make_service().await;

        svc.store_entry("key1", "version 1", "note").await.unwrap();
        svc.store_entry("key1", "version 2", "note").await.unwrap();

        let results = svc.search("version", 10).await.unwrap();
        // Should only have the latest version, not both.
        assert_eq!(results.len(), 1);
        assert!(results[0].text.contains("version 2"));
    }

    #[tokio::test]
    async fn test_sync_workspace() {
        use tempfile::TempDir;

        let svc = make_service().await;
        let tmp = TempDir::new().unwrap();
        let memory_md = tmp.path().join("MEMORY.md");
        tokio::fs::write(&memory_md, "# Project Notes\n\nImportant context here.")
            .await
            .unwrap();

        let report = svc.sync_workspace(tmp.path()).await.unwrap();
        assert_eq!(report.files_added, 1);
        assert_eq!(report.chunks_created, 1);
    }

    #[test]
    fn test_normalize_scores() {
        let entries = vec![
            MemoryEntry {
                chunk_id: 1,
                path: "a".into(),
                text: "a".into(),
                score: 0.2,
                start_line: 1,
                end_line: 1,
            },
            MemoryEntry {
                chunk_id: 2,
                path: "b".into(),
                text: "b".into(),
                score: 0.8,
                start_line: 1,
                end_line: 1,
            },
        ];
        let normed = normalize_scores(&entries);
        assert!((normed[0] - 0.0).abs() < 1e-9);
        assert!((normed[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_normalize_scores_inverted() {
        let entries = vec![
            MemoryEntry {
                chunk_id: 1,
                path: "a".into(),
                text: "a".into(),
                score: -5.0, // best BM25 match
                start_line: 1,
                end_line: 1,
            },
            MemoryEntry {
                chunk_id: 2,
                path: "b".into(),
                text: "b".into(),
                score: -1.0, // worse BM25 match
                start_line: 1,
                end_line: 1,
            },
        ];
        let normed = normalize_scores_inverted(&entries);
        // -5.0 should get highest normalized score (1.0)
        assert!((normed[0] - 1.0).abs() < 1e-9);
        assert!((normed[1] - 0.0).abs() < 1e-9);
    }
}
