//! In-memory MemoryBackend — the default backend. Zero external dependencies,
//! no filesystem, no embedded database. State is lost when the process exits.
//!
//! For persistent storage, use `alva-app-extension-memory::MemorySqlite` or
//! write your own backend.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::backend::MemoryBackend;
use crate::error::MemoryError;
use crate::types::{MemoryEntry, MemoryFile};

#[derive(Default)]
pub struct InMemoryBackend {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    files: HashMap<String, MemoryFile>,
    chunks: Vec<StoredChunk>,
    next_chunk_id: i64,
    embeddings: HashMap<(String, String), Vec<f32>>,
}

#[derive(Clone)]
struct StoredChunk {
    id: i64,
    path: String,
    #[allow(dead_code)]
    source: String,
    start_line: i64,
    end_line: i64,
    #[allow(dead_code)]
    hash: String,
    text: String,
    embedding: Vec<f32>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MemoryBackend for InMemoryBackend {
    async fn upsert_file(&self, file: &MemoryFile) -> Result<(), MemoryError> {
        let mut inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        inner.files.insert(file.path.clone(), file.clone());
        Ok(())
    }

    async fn get_file(&self, path: &str) -> Result<Option<MemoryFile>, MemoryError> {
        let inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        Ok(inner.files.get(path).cloned())
    }

    async fn insert_chunk(
        &self,
        path: &str,
        source: &str,
        start_line: i64,
        end_line: i64,
        hash: &str,
        text: &str,
        embedding: &[f32],
    ) -> Result<i64, MemoryError> {
        let mut inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        let id = inner.next_chunk_id;
        inner.next_chunk_id += 1;
        inner.chunks.push(StoredChunk {
            id,
            path: path.to_string(),
            source: source.to_string(),
            start_line,
            end_line,
            hash: hash.to_string(),
            text: text.to_string(),
            embedding: embedding.to_vec(),
        });
        Ok(id)
    }

    async fn delete_chunks_for_path(&self, path: &str) -> Result<(), MemoryError> {
        let mut inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        inner.chunks.retain(|c| c.path != path);
        Ok(())
    }

    async fn fts_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        let q = query.to_lowercase();
        let results: Vec<MemoryEntry> = inner
            .chunks
            .iter()
            .filter(|c| c.text.to_lowercase().contains(&q))
            .take(max_results)
            .map(|c| MemoryEntry {
                chunk_id: c.id,
                path: c.path.clone(),
                text: c.text.clone(),
                score: 1.0,
                start_line: c.start_line,
                end_line: c.end_line,
            })
            .collect();
        Ok(results)
    }

    async fn vector_search(
        &self,
        query_embedding: &[f32],
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        let mut scored: Vec<(f32, &StoredChunk)> = inner
            .chunks
            .iter()
            .map(|c| (cosine_sim(query_embedding, &c.embedding), c))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let results: Vec<MemoryEntry> = scored
            .into_iter()
            .take(max_results)
            .map(|(score, c)| MemoryEntry {
                chunk_id: c.id,
                path: c.path.clone(),
                text: c.text.clone(),
                score: score as f64,
                start_line: c.start_line,
                end_line: c.end_line,
            })
            .collect();
        Ok(results)
    }

    async fn cache_embedding(
        &self,
        model: &str,
        hash: &str,
        embedding: &[f32],
    ) -> Result<(), MemoryError> {
        let mut inner = self.inner.lock().expect("InMemoryBackend mutex poisoned");
        inner
            .embeddings
            .insert((model.to_string(), hash.to_string()), embedding.to_vec());
        Ok(())
    }
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_then_get() {
        let backend = InMemoryBackend::new();
        let file = MemoryFile {
            path: "foo.md".into(),
            source: "user".into(),
            hash: "h".into(),
            mtime: "now".into(),
            size: 1,
        };
        backend.upsert_file(&file).await.unwrap();
        let got = backend.get_file("foo.md").await.unwrap().unwrap();
        assert_eq!(got.path, "foo.md");
    }

    #[tokio::test]
    async fn insert_chunk_returns_increasing_id() {
        let backend = InMemoryBackend::new();
        let id0 = backend
            .insert_chunk("p", "s", 0, 1, "h0", "hello world", &[])
            .await
            .unwrap();
        let id1 = backend
            .insert_chunk("p", "s", 1, 2, "h1", "world peace", &[])
            .await
            .unwrap();
        assert!(id1 > id0);
    }

    #[tokio::test]
    async fn fts_finds_matching_text() {
        let backend = InMemoryBackend::new();
        backend
            .insert_chunk("p", "s", 0, 1, "h", "hello world", &[])
            .await
            .unwrap();
        backend
            .insert_chunk("p", "s", 1, 2, "h", "goodbye moon", &[])
            .await
            .unwrap();
        let hits = backend.fts_search("hello", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "hello world");
    }
}
