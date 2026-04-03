// INPUT:  super::events::AnalyticsEvent, std::sync, std::path, std::fs, serde_json
// OUTPUT: AnalyticsSink, FileAnalyticsSink, AnalyticsService
// POS:    Analytics sink infrastructure — trait, file-based sink, and multiplexing service.

//! Analytics sink infrastructure — trait, file-based sink, and multiplexing service.
//!
//! The [`AnalyticsService`] implements a queue-then-flush pattern: events logged
//! before any sink is attached are buffered and replayed when the first sink
//! arrives. This ensures no events are lost during startup.

use super::events::AnalyticsEvent;
use std::sync::{Arc, Mutex};

/// Trait for analytics event sinks
pub trait AnalyticsSink: Send + Sync {
    fn log(&self, event: &AnalyticsEvent);
    fn flush(&self);
}

/// File-based analytics sink (writes to JSONL file)
pub struct FileAnalyticsSink {
    path: std::path::PathBuf,
    buffer: Mutex<Vec<AnalyticsEvent>>,
    max_buffer_size: usize,
}

impl FileAnalyticsSink {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            buffer: Mutex::new(Vec::new()),
            max_buffer_size: 100,
        }
    }
}

impl AnalyticsSink for FileAnalyticsSink {
    fn log(&self, event: &AnalyticsEvent) {
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.push(event.clone());
            if buffer.len() >= self.max_buffer_size {
                self.flush_buffer(&mut buffer);
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut buffer) = self.buffer.lock() {
            self.flush_buffer(&mut buffer);
        }
    }
}

impl FileAnalyticsSink {
    fn flush_buffer(&self, buffer: &mut Vec<AnalyticsEvent>) {
        if buffer.is_empty() {
            return;
        }

        // Best-effort write (fail-open)
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            use std::io::Write;
            for event in buffer.iter() {
                if let Ok(line) = serde_json::to_string(event) {
                    let _ = writeln!(file, "{}", line);
                }
            }
        }

        buffer.clear();
    }
}

impl Drop for FileAnalyticsSink {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Analytics service with queue pattern and multiplexing
pub struct AnalyticsService {
    sinks: Vec<Arc<dyn AnalyticsSink>>,
    queue: Mutex<Vec<AnalyticsEvent>>,
}

impl AnalyticsService {
    pub fn new() -> Self {
        Self {
            sinks: Vec::new(),
            queue: Mutex::new(Vec::new()),
        }
    }

    /// Attach a sink (queued events are flushed to it)
    pub fn attach_sink(&mut self, sink: Arc<dyn AnalyticsSink>) {
        // Flush queued events to new sink
        if let Ok(mut queue) = self.queue.lock() {
            for event in queue.drain(..) {
                sink.log(&event);
            }
        }
        self.sinks.push(sink);
    }

    /// Log an event (fail-open: if no sink, events are queued)
    pub fn log_event(&self, event: AnalyticsEvent) {
        if self.sinks.is_empty() {
            if let Ok(mut queue) = self.queue.lock() {
                queue.push(event);
            }
            return;
        }

        for sink in &self.sinks {
            sink.log(&event);
        }
    }

    /// Convenience method to log an event by name
    pub fn log(&self, name: &str, session_id: &str) {
        self.log_event(AnalyticsEvent::new(name, session_id));
    }

    /// Flush all sinks
    pub fn flush(&self) {
        for sink in &self.sinks {
            sink.flush();
        }
    }
}

impl Default for AnalyticsService {
    fn default() -> Self {
        Self::new()
    }
}
