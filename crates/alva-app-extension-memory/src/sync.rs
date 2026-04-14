// INPUT:  std::collections, std::hash, std::path, crate::error, crate::{embedding, sqlite, types}, walkdir, chrono, tokio::fs
// OUTPUT: sync_workspace
// POS:    Scans workspace for MEMORY.md files, chunks content, computes embeddings, and indexes into MemorySqlite.
//! Workspace file synchronization — scan MEMORY.md files, chunk, embed, store.

use std::collections::HashSet;
use std::path::Path;

use alva_agent_memory::error::MemoryError;

use alva_agent_memory::backend::MemoryBackend;
use alva_agent_memory::embedding::EmbeddingProvider;
use alva_agent_memory::types::{MemoryFile, SyncReport};

/// Configuration for workspace synchronization.
pub struct SyncConfig {
    /// Lines per chunk (default: 50).
    pub chunk_size: usize,
    /// Maximum directory depth to walk (default: 10).
    pub max_depth: usize,
    /// Directory names to skip (default: hidden dirs, node_modules, target).
    pub skip_dirs: HashSet<String>,
    /// File name to index (default: "MEMORY.md").
    pub index_filename: String,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            chunk_size: 50,
            max_depth: 10,
            skip_dirs: ["node_modules", "target"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            index_filename: "MEMORY.md".to_string(),
        }
    }
}

/// Synchronize a workspace directory into the memory store.
///
/// Scans for memory files, compares hashes, and re-indexes changed files.
/// Use `SyncConfig::default()` for standard behavior.
pub async fn sync_workspace(
    workspace: &Path,
    store: &dyn MemoryBackend,
    embedder: &dyn EmbeddingProvider,
) -> Result<SyncReport, MemoryError> {
    sync_workspace_with_config(workspace, store, embedder, &SyncConfig::default()).await
}

/// Synchronize with custom configuration.
pub async fn sync_workspace_with_config(
    workspace: &Path,
    store: &dyn MemoryBackend,
    embedder: &dyn EmbeddingProvider,
    config: &SyncConfig,
) -> Result<SyncReport, MemoryError> {
    let mut report = SyncReport::default();

    let skip_dirs = config.skip_dirs.clone();
    let walker = walkdir::WalkDir::new(workspace)
        .follow_links(true)
        .max_depth(config.max_depth)
        .into_iter()
        .filter_entry(move |e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && !skip_dirs.contains(name.as_ref())
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy();
        if file_name != config.index_filename {
            continue;
        }

        report.files_scanned += 1;
        let path_str = entry.path().to_string_lossy().to_string();

        // Read file content
        let content = match tokio::fs::read_to_string(entry.path()).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let metadata = tokio::fs::metadata(entry.path()).await.ok();
        let size = metadata.as_ref().map(|m| m.len() as i64).unwrap_or(0);
        let mtime = metadata
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            })
            .unwrap_or_default();

        let hash = compute_hash(&content);

        // Check if file has changed
        if let Some(existing) = store.get_file(&path_str).await? {
            if existing.hash == hash {
                report.files_unchanged += 1;
                continue;
            }
            // File changed — re-index
            store.delete_chunks_for_path(&path_str).await?;
            report.files_updated += 1;
        } else {
            report.files_added += 1;
        }

        // Upsert file record
        let mem_file = MemoryFile {
            path: path_str.clone(),
            source: "workspace".into(),
            hash: hash.clone(),
            mtime,
            size,
        };
        store.upsert_file(&mem_file).await?;

        // Chunk the content
        let chunks = chunk_text(&content, config.chunk_size);

        // Compute embeddings in batch
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        let embeddings = embedder.embed(&texts).await.unwrap_or_else(|_| {
            texts.iter().map(|_| Vec::new()).collect()
        });

        for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
            let chunk_hash = compute_hash(&chunk.text);
            store
                .insert_chunk(
                    &path_str,
                    "workspace",
                    chunk.start_line as i64,
                    chunk.end_line as i64,
                    &chunk_hash,
                    &chunk.text,
                    embedding,
                )
                .await?;
            report.chunks_created += 1;
        }
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// Chunking
// ---------------------------------------------------------------------------

struct TextChunk {
    text: String,
    start_line: usize,
    end_line: usize,
}

fn chunk_text(content: &str, chunk_size: usize) -> Vec<TextChunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let end = (i + chunk_size).min(lines.len());
        let text = lines[i..end].join("\n");
        chunks.push(TextChunk {
            text,
            start_line: i + 1, // 1-based
            end_line: end,
        });
        i = end;
    }
    chunks
}

fn compute_hash(content: &str) -> String {
    alva_agent_memory::hash::compute_hash(content)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::MemorySqlite;

    #[test]
    fn test_chunk_text() {
        let content = (1..=120)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        let chunks = chunk_text(&content, 50);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 50);
        assert_eq!(chunks[1].start_line, 51);
        assert_eq!(chunks[1].end_line, 100);
        assert_eq!(chunks[2].start_line, 101);
        assert_eq!(chunks[2].end_line, 120);
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 50);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_compute_hash() {
        let h1 = compute_hash("hello");
        let h2 = compute_hash("hello");
        let h3 = compute_hash("world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[tokio::test]
    async fn test_sync_workspace() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let memory_md = tmp.path().join("MEMORY.md");
        tokio::fs::write(&memory_md, "# Test Memory\n\nSome content here.")
            .await
            .unwrap();

        let store = MemorySqlite::open_in_memory().await.unwrap();
        let embedder = alva_agent_memory::embedding::NoopEmbeddingProvider::new();

        let report = sync_workspace(tmp.path(), &store, &embedder)
            .await
            .unwrap();

        assert_eq!(report.files_scanned, 1);
        assert_eq!(report.files_added, 1);
        assert_eq!(report.chunks_created, 1);

        // Second sync — should be unchanged
        let report2 = sync_workspace(tmp.path(), &store, &embedder)
            .await
            .unwrap();
        assert_eq!(report2.files_unchanged, 1);
        assert_eq!(report2.files_added, 0);
        assert_eq!(report2.chunks_created, 0);
    }
}
