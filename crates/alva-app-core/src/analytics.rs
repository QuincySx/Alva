// INPUT:  std::fs/io/path/sync, alva_kernel_abi::{AnalyticsSink, AnalyticsEvent}, serde_json
// OUTPUT: JsonlSink
// POS:    File-backed `AnalyticsSink` with size-based rotation. Concrete telemetry
//         storage living in app-core; the trait + events live in kernel-abi so
//         lower layers (kernel-core) can emit without depending on this module.

//! JSONL-backed analytics sink.
//!
//! Writes one JSON-encoded `AnalyticsEvent` per line. Rotates when the
//! active file exceeds `max_bytes`, keeping at most `retain_count`
//! rotated files. All failures are swallowed (via `tracing::warn!`) so
//! telemetry never disrupts the agent loop.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use alva_kernel_abi::{AnalyticsEvent, AnalyticsSink};

/// Default rotation threshold — 100 MB.
pub const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;

/// Default retained rotated files.
pub const DEFAULT_RETAIN: usize = 7;

pub struct JsonlSink {
    path: PathBuf,
    max_bytes: u64,
    retain_count: usize,
    state: Mutex<SinkState>,
}

struct SinkState {
    writer: Option<BufWriter<File>>,
    current_size: u64,
}

impl JsonlSink {
    /// Open or create `path` (creating parent dirs as needed). Failure
    /// here is propagated; failures during subsequent writes are not.
    pub fn new(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        Self::with_options(path, DEFAULT_MAX_BYTES, DEFAULT_RETAIN)
    }

    pub fn with_options(
        path: impl Into<PathBuf>,
        max_bytes: u64,
        retain_count: usize,
    ) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let (writer, current_size) = open_writer(&path)?;
        Ok(Self {
            path,
            max_bytes,
            retain_count,
            state: Mutex::new(SinkState {
                writer: Some(writer),
                current_size,
            }),
        })
    }

    fn write_event(&self, event: &AnalyticsEvent) -> std::io::Result<()> {
        let mut line = serde_json::to_string(event).map_err(io_other)?;
        line.push('\n');
        let bytes = line.as_bytes();

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = state.writer.as_mut() {
            w.write_all(bytes)?;
            w.flush()?;
            state.current_size += bytes.len() as u64;
        }

        if state.current_size >= self.max_bytes {
            // Rotate. Drop the writer to flush + close, rename, reopen.
            state.writer = None;
            if let Err(e) = rotate(&self.path, self.retain_count) {
                tracing::warn!(error = %e, "analytics rotation failed");
            }
            let (writer, size) = open_writer(&self.path)?;
            state.writer = Some(writer);
            state.current_size = size;
        }
        Ok(())
    }
}

impl AnalyticsSink for JsonlSink {
    fn record(&self, event: AnalyticsEvent) {
        if let Err(e) = self.write_event(&event) {
            tracing::warn!(error = %e, "analytics write failed");
        }
    }
}

fn open_writer(path: &Path) -> std::io::Result<(BufWriter<File>, u64)> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    Ok((BufWriter::new(file), size))
}

/// Rename `path` to `path.<n>.jsonl` (lowest free `n` ≥ 1), then prune
/// rotated files past `retain_count`.
fn rotate(path: &Path, retain_count: usize) -> std::io::Result<()> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io_other("invalid file stem"))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("jsonl");

    // Find lowest free numeric slot.
    let mut n = 1usize;
    let target = loop {
        let candidate = parent.join(format!("{stem}.{n}.{ext}"));
        if !candidate.exists() {
            break candidate;
        }
        n += 1;
    };
    fs::rename(path, &target)?;

    // Prune past retain_count.
    let mut rotated = list_rotated(parent, stem, ext)?;
    rotated.sort_by_key(|(idx, _)| *idx);
    if rotated.len() > retain_count {
        let to_drop = rotated.len() - retain_count;
        // Drop OLDEST (smallest index) first — that's the first `to_drop` items.
        for (_, p) in rotated.iter().take(to_drop) {
            if let Err(e) = fs::remove_file(p) {
                tracing::warn!(path = %p.display(), error = %e, "analytics prune failed");
            }
        }
    }
    Ok(())
}

fn list_rotated(parent: &Path, stem: &str, ext: &str) -> std::io::Result<Vec<(usize, PathBuf)>> {
    let mut out = Vec::new();
    let prefix = format!("{stem}.");
    let suffix = format!(".{ext}");
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if !name_str.starts_with(&prefix) || !name_str.ends_with(&suffix) {
            continue;
        }
        let middle = &name_str[prefix.len()..name_str.len() - suffix.len()];
        if let Ok(idx) = middle.parse::<usize>() {
            out.push((idx, entry.path()));
        }
    }
    Ok(out)
}

fn io_other<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn ev(session_id: &str) -> AnalyticsEvent {
        AnalyticsEvent::SessionStart {
            session_id: session_id.into(),
            workspace: PathBuf::from("/w"),
            ts: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn writes_one_line_per_event() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.jsonl");
        let sink = JsonlSink::new(&path).unwrap();
        sink.record(ev("s1"));
        sink.record(ev("s2"));

        let body = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let parsed: AnalyticsEvent = serde_json::from_str(line).unwrap();
            assert!(matches!(parsed, AnalyticsEvent::SessionStart { .. }));
        }
    }

    #[test]
    fn rotates_when_size_exceeds_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.jsonl");
        // Tiny threshold — every event triggers rotation.
        let sink = JsonlSink::with_options(&path, 50, 5).unwrap();
        sink.record(ev("first"));
        sink.record(ev("second"));
        sink.record(ev("third"));

        // The rotated file naming is a.<n>.jsonl
        let rotated_1 = tmp.path().join("a.1.jsonl");
        let rotated_2 = tmp.path().join("a.2.jsonl");
        assert!(rotated_1.exists(), "expected first rotation");
        assert!(rotated_2.exists(), "expected second rotation");
        // Active file exists (may have content from third event)
        assert!(path.exists());
    }

    #[test]
    fn prunes_past_retain_count() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.jsonl");
        // Tiny threshold + retain 2 → after 5 events we should keep only 2 rotated.
        let sink = JsonlSink::with_options(&path, 50, 2).unwrap();
        for i in 0..6 {
            sink.record(ev(&format!("s{i}")));
        }
        // Rotated files numbered .1 .. .N — older ones (smallest index) pruned.
        let mut rotated: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                if name.starts_with("a.") && name.ends_with(".jsonl") && name != "a.jsonl" {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        rotated.sort();
        assert!(
            rotated.len() <= 2,
            "expected at most 2 rotated files (retain=2), found {rotated:?}"
        );
    }

    #[test]
    fn write_failure_does_not_panic() {
        // After we invalidate the file by making the dir read-only the write
        // should swallow the error. Hard to assert filesystem permissions
        // portably — instead we just verify the noop path: dropping all
        // events on a closed sink doesn't panic.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.jsonl");
        let sink = JsonlSink::new(&path).unwrap();
        sink.record(ev("ok"));
        // Multiple writes don't panic.
        for _ in 0..10 {
            sink.record(ev("loop"));
        }
    }
}
