//! Tracing log capture layer — captures structured tracing events per run.
//!
//! This is a `tracing_subscriber::Layer` that buffers log entries keyed by run_id.
//! It lives purely at the eval app layer — no changes to Provider or Agent-core needed.
//! The providers already emit debug/info events; this layer captures them for the UI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

// ---------------------------------------------------------------------------
// Log entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Shared log store
// ---------------------------------------------------------------------------

/// Thread-safe store for captured log entries, keyed by run_id.
#[derive(Clone)]
pub struct LogStore {
    /// Current active run_id — set before agent run starts, cleared after.
    active_run: Arc<Mutex<Option<String>>>,
    /// Captured logs per run_id.
    logs: Arc<Mutex<HashMap<String, Vec<LogEntry>>>>,
}

impl LogStore {
    pub fn new() -> Self {
        Self {
            active_run: Arc::new(Mutex::new(None)),
            logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set the active run_id. All subsequent log events will be captured under this id.
    pub fn start_capture(&self, run_id: &str) {
        *self.active_run.lock().unwrap() = Some(run_id.to_string());
    }

    /// Stop capturing logs for the current run.
    pub fn stop_capture(&self) {
        *self.active_run.lock().unwrap() = None;
    }

    /// Get captured logs for a run.
    pub fn get_logs(&self, run_id: &str) -> Vec<LogEntry> {
        self.logs
            .lock()
            .unwrap()
            .get(run_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Push a log entry to the active run.
    fn push(&self, entry: LogEntry) {
        let run_id = self.active_run.lock().unwrap().clone();
        if let Some(run_id) = run_id {
            self.logs
                .lock()
                .unwrap()
                .entry(run_id)
                .or_default()
                .push(entry);
        }
    }
}

// ---------------------------------------------------------------------------
// Field visitor — extracts tracing fields into HashMap<String, String>
// ---------------------------------------------------------------------------

struct FieldVisitor {
    fields: HashMap<String, String>,
    message: String,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            fields: HashMap::new(),
            message: String::new(),
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value).trim_matches('"').to_string();
        } else {
            self.fields
                .insert(field.name().to_string(), format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

// ---------------------------------------------------------------------------
// Tracing Layer implementation
// ---------------------------------------------------------------------------

/// A tracing Layer that captures events from LLM providers and agent-core
/// into a per-run log buffer.
///
/// Only captures events from relevant targets (alva_llm_provider, alva_agent_core).
pub struct LogCaptureLayer {
    store: LogStore,
}

impl LogCaptureLayer {
    pub fn new(store: LogStore) -> Self {
        Self { store }
    }
}

impl<S: Subscriber> Layer<S> for LogCaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();

        // Only capture events from our crates
        if !target.starts_with("alva_llm_provider")
            && !target.starts_with("alva_agent_core")
        {
            return;
        }

        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level: meta.level().to_string(),
            target: target.to_string(),
            message: visitor.message,
            fields: visitor.fields,
        };

        self.store.push(entry);
    }
}
