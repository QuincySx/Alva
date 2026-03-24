// INPUT:  tokio_rusqlite, crate::error, crate::types, rusqlite
// OUTPUT: MemorySqlite
// POS:    SQLite storage backend for memory: FTS5 full-text search, brute-force vector search, embedding cache, and file/chunk CRUD.
//! SQLite storage backend for the memory subsystem.
//!
//! Tables: `memory_files`, `memory_chunks`, `chunks_fts` (FTS5), `embedding_cache`.

use tokio_rusqlite::Connection;

use crate::error::MemoryError;

use crate::types::{MemoryEntry, MemoryFile};

// ---------------------------------------------------------------------------
// DDL
// ---------------------------------------------------------------------------

const DDL: &str = "
CREATE TABLE IF NOT EXISTS memory_files (
    path   TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    hash   TEXT NOT NULL DEFAULT '',
    mtime  TEXT NOT NULL DEFAULT '',
    size   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS memory_chunks (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    path       TEXT NOT NULL REFERENCES memory_files(path) ON DELETE CASCADE,
    source     TEXT NOT NULL DEFAULT '',
    start_line INTEGER NOT NULL DEFAULT 0,
    end_line   INTEGER NOT NULL DEFAULT 0,
    hash       TEXT NOT NULL DEFAULT '',
    model      TEXT NOT NULL DEFAULT '',
    text       TEXT NOT NULL DEFAULT '',
    embedding  BLOB
);
CREATE INDEX IF NOT EXISTS idx_chunks_path ON memory_chunks(path);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    text,
    content='memory_chunks',
    content_rowid='id'
);

-- Triggers to keep FTS in sync with memory_chunks
CREATE TRIGGER IF NOT EXISTS chunks_fts_ai AFTER INSERT ON memory_chunks BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS chunks_fts_ad AFTER DELETE ON memory_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER IF NOT EXISTS chunks_fts_au AFTER UPDATE ON memory_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE IF NOT EXISTS embedding_cache (
    provider     TEXT NOT NULL DEFAULT '',
    model        TEXT NOT NULL DEFAULT '',
    provider_key TEXT NOT NULL DEFAULT '',
    hash         TEXT NOT NULL,
    embedding    BLOB NOT NULL,
    dims         INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hash, model)
);
";

/// Memory-specific SQLite store.
pub struct MemorySqlite {
    conn: Connection,
}

impl MemorySqlite {
    /// Open or create the memory database at `path`.
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self, MemoryError> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)
            .await
            .map_err(|e| MemoryError::Storage(format!("memory sqlite open: {e}")))?;
        Self::init(conn).await
    }

    /// Open an in-memory database for testing.
    pub async fn open_in_memory() -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory()
            .await
            .map_err(|e| MemoryError::Storage(format!("memory sqlite open memory: {e}")))?;
        Self::init(conn).await
    }

    async fn init(conn: Connection) -> Result<Self, MemoryError> {
        conn.call(|conn| {
            conn.pragma_update(None, "journal_mode", "wal")?;
            conn.pragma_update(None, "foreign_keys", "on")?;
            conn.execute_batch(DDL)?;
            Ok(())
        })
        .await
        .map_err(|e| MemoryError::Storage(format!("memory sqlite init: {e}")))?;
        Ok(Self { conn })
    }

    // -----------------------------------------------------------------------
    // Files
    // -----------------------------------------------------------------------

    /// Upsert a tracked file record.
    pub async fn upsert_file(&self, file: &MemoryFile) -> Result<(), MemoryError> {
        let f = file.clone();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO memory_files (path, source, hash, mtime, size)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(path) DO UPDATE SET
                        source = excluded.source,
                        hash   = excluded.hash,
                        mtime  = excluded.mtime,
                        size   = excluded.size",
                    rusqlite::params![f.path, f.source, f.hash, f.mtime, f.size],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("upsert_file: {e}")))
    }

    /// Get a tracked file by path.
    pub async fn get_file(&self, path: &str) -> Result<Option<MemoryFile>, MemoryError> {
        let path = path.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT path, source, hash, mtime, size FROM memory_files WHERE path = ?1")?;
                let mut rows = stmt.query(rusqlite::params![path])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(MemoryFile {
                        path: row.get(0)?,
                        source: row.get(1)?,
                        hash: row.get(2)?,
                        mtime: row.get(3)?,
                        size: row.get(4)?,
                    }))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("get_file: {e}")))
    }

    // -----------------------------------------------------------------------
    // Chunks
    // -----------------------------------------------------------------------

    /// Insert a chunk and return its auto-generated id.
    pub async fn insert_chunk(
        &self,
        path: &str,
        source: &str,
        start_line: i64,
        end_line: i64,
        hash: &str,
        text: &str,
        embedding: &[f32],
    ) -> Result<i64, MemoryError> {
        let path = path.to_string();
        let source = source.to_string();
        let hash = hash.to_string();
        let text = text.to_string();
        let emb_bytes = embedding_to_bytes(embedding);

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO memory_chunks (path, source, start_line, end_line, hash, text, embedding)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![path, source, start_line, end_line, hash, text, emb_bytes],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("insert_chunk: {e}")))
    }

    /// Delete all chunks for a given file path.
    pub async fn delete_chunks_for_path(&self, path: &str) -> Result<(), MemoryError> {
        let path = path.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM memory_chunks WHERE path = ?1",
                    rusqlite::params![path],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("delete_chunks_for_path: {e}")))
    }

    // -----------------------------------------------------------------------
    // FTS search
    // -----------------------------------------------------------------------

    /// Full-text search across memory chunks.
    pub async fn fts_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let query = query.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT mc.id, mc.path, mc.text, mc.start_line, mc.end_line,
                            bm25(chunks_fts) AS score
                     FROM chunks_fts
                     JOIN memory_chunks mc ON mc.id = chunks_fts.rowid
                     WHERE chunks_fts MATCH ?1
                     ORDER BY score
                     LIMIT ?2",
                )?;
                let mut rows = stmt.query(rusqlite::params![query, max_results as i64])?;
                let mut results = Vec::new();
                while let Some(row) = rows.next()? {
                    results.push(MemoryEntry {
                        chunk_id: row.get(0)?,
                        path: row.get(1)?,
                        text: row.get(2)?,
                        start_line: row.get(3)?,
                        end_line: row.get(4)?,
                        score: row.get(5)?,
                    });
                }
                Ok(results)
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("fts_search: {e}")))
    }

    // -----------------------------------------------------------------------
    // Vector search (placeholder — cosine similarity in Rust)
    // -----------------------------------------------------------------------

    /// Brute-force cosine similarity search against stored embeddings.
    /// This is a placeholder; a production system would use a vector index.
    pub async fn vector_search(
        &self,
        query_embedding: &[f32],
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }
        let qe = query_embedding.to_vec();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, path, text, start_line, end_line, embedding
                     FROM memory_chunks WHERE embedding IS NOT NULL",
                )?;
                let mut rows = stmt.query([])?;
                let mut scored: Vec<MemoryEntry> = Vec::new();
                while let Some(row) = rows.next()? {
                    let emb_bytes: Vec<u8> = row.get(5)?;
                    let emb = bytes_to_embedding(&emb_bytes);
                    if emb.is_empty() {
                        continue;
                    }
                    let sim = cosine_similarity(&qe, &emb);
                    scored.push(MemoryEntry {
                        chunk_id: row.get(0)?,
                        path: row.get(1)?,
                        text: row.get(2)?,
                        start_line: row.get(3)?,
                        end_line: row.get(4)?,
                        score: sim,
                    });
                }
                // Sort descending by similarity.
                scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                scored.truncate(max_results);
                Ok(scored)
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("vector_search: {e}")))
    }

    // -----------------------------------------------------------------------
    // Embedding cache
    // -----------------------------------------------------------------------

    /// Cache an embedding result.
    pub async fn cache_embedding(
        &self,
        model: &str,
        hash: &str,
        embedding: &[f32],
    ) -> Result<(), MemoryError> {
        let model = model.to_string();
        let hash = hash.to_string();
        let emb_bytes = embedding_to_bytes(embedding);
        let dims = embedding.len() as i64;

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO embedding_cache (model, hash, embedding, dims)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![model, hash, emb_bytes, dims],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("cache_embedding: {e}")))
    }

    /// Look up a cached embedding.
    pub async fn get_cached_embedding(
        &self,
        model: &str,
        hash: &str,
    ) -> Result<Option<Vec<f32>>, MemoryError> {
        let model = model.to_string();
        let hash = hash.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT embedding FROM embedding_cache WHERE model = ?1 AND hash = ?2",
                )?;
                let mut rows = stmt.query(rusqlite::params![model, hash])?;
                if let Some(row) = rows.next()? {
                    let bytes: Vec<u8> = row.get(0)?;
                    Ok(Some(bytes_to_embedding(&bytes)))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| MemoryError::Storage(format!("get_cached_embedding: {e}")))
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl crate::backend::MemoryBackend for MemorySqlite {
    async fn upsert_file(&self, file: &MemoryFile) -> Result<(), MemoryError> {
        self.upsert_file(file).await
    }

    async fn get_file(&self, path: &str) -> Result<Option<MemoryFile>, MemoryError> {
        self.get_file(path).await
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
        self.insert_chunk(path, source, start_line, end_line, hash, text, embedding)
            .await
    }

    async fn delete_chunks_for_path(&self, path: &str) -> Result<(), MemoryError> {
        self.delete_chunks_for_path(path).await
    }

    async fn fts_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.fts_search(query, max_results).await
    }

    async fn vector_search(
        &self,
        query_embedding: &[f32],
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.vector_search(query_embedding, max_results).await
    }

    async fn cache_embedding(
        &self,
        model: &str,
        hash: &str,
        embedding: &[f32],
    ) -> Result<(), MemoryError> {
        self.cache_embedding(model, hash, embedding).await
    }
}

// ---------------------------------------------------------------------------
// Embedding serialization helpers
// ---------------------------------------------------------------------------

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_upsert_and_get() {
        let store = MemorySqlite::open_in_memory().await.unwrap();

        let file = MemoryFile {
            path: "/tmp/test.md".into(),
            source: "workspace".into(),
            hash: "abc123".into(),
            mtime: "2025-01-01T00:00:00".into(),
            size: 1024,
        };
        store.upsert_file(&file).await.unwrap();

        let fetched = store.get_file("/tmp/test.md").await.unwrap().unwrap();
        assert_eq!(fetched.hash, "abc123");
        assert_eq!(fetched.size, 1024);

        // upsert with new hash
        let updated = MemoryFile {
            hash: "def456".into(),
            ..file
        };
        store.upsert_file(&updated).await.unwrap();
        let fetched = store.get_file("/tmp/test.md").await.unwrap().unwrap();
        assert_eq!(fetched.hash, "def456");
    }

    #[tokio::test]
    async fn test_chunk_insert_and_fts_search() {
        let store = MemorySqlite::open_in_memory().await.unwrap();

        let file = MemoryFile {
            path: "/tmp/test.md".into(),
            source: "workspace".into(),
            hash: "abc".into(),
            mtime: "2025-01-01".into(),
            size: 100,
        };
        store.upsert_file(&file).await.unwrap();

        store
            .insert_chunk(
                "/tmp/test.md",
                "workspace",
                1,
                10,
                "chunk-hash-1",
                "Rust is a systems programming language",
                &[],
            )
            .await
            .unwrap();

        store
            .insert_chunk(
                "/tmp/test.md",
                "workspace",
                11,
                20,
                "chunk-hash-2",
                "Python is great for data science",
                &[],
            )
            .await
            .unwrap();

        // FTS search
        let results = store.fts_search("Rust programming", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results[0].text.contains("Rust"));
    }

    #[tokio::test]
    async fn test_vector_search() {
        let store = MemorySqlite::open_in_memory().await.unwrap();

        let file = MemoryFile {
            path: "/tmp/v.md".into(),
            source: "ws".into(),
            hash: "h".into(),
            mtime: "2025".into(),
            size: 10,
        };
        store.upsert_file(&file).await.unwrap();

        // Insert two chunks with known embeddings
        store
            .insert_chunk("/tmp/v.md", "ws", 1, 5, "h1", "chunk A", &[1.0, 0.0, 0.0])
            .await
            .unwrap();
        store
            .insert_chunk("/tmp/v.md", "ws", 6, 10, "h2", "chunk B", &[0.0, 1.0, 0.0])
            .await
            .unwrap();

        // Query closer to chunk A
        let results = store.vector_search(&[0.9, 0.1, 0.0], 10).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].text.contains("chunk A"));
    }

    #[tokio::test]
    async fn test_embedding_cache() {
        let store = MemorySqlite::open_in_memory().await.unwrap();

        store
            .cache_embedding("text-embedding-3-small", "hash123", &[0.1, 0.2, 0.3])
            .await
            .unwrap();

        let cached = store
            .get_cached_embedding("text-embedding-3-small", "hash123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached.len(), 3);
        assert!((cached[0] - 0.1).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_delete_chunks_for_path() {
        let store = MemorySqlite::open_in_memory().await.unwrap();

        let file = MemoryFile {
            path: "/tmp/del.md".into(),
            source: "ws".into(),
            hash: "h".into(),
            mtime: "2025".into(),
            size: 10,
        };
        store.upsert_file(&file).await.unwrap();
        store
            .insert_chunk("/tmp/del.md", "ws", 1, 5, "h1", "content", &[])
            .await
            .unwrap();

        store.delete_chunks_for_path("/tmp/del.md").await.unwrap();

        let results = store.fts_search("content", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_cosine_similarity() {
        let a = [1.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-9);

        let c = [0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 1e-9);
    }
}
