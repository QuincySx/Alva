//! Tracing log capture layer — captures structured tracing events per run.
//!
//! This is a `tracing_subscriber::Layer` that buffers log entries keyed by run_id.
//! It lives purely at the eval app layer — no changes to Provider or Agent-core needed.
//! The providers already emit debug/info events; this layer captures them for the UI.

use std::collections::{HashMap, HashSet};
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
///
/// Supports multiple concurrent active runs (e.g., compare mode).
/// Events are broadcast to ALL active runs since tracing events are global.
#[derive(Clone)]
pub struct LogStore {
    /// Currently active run IDs — events are captured for all of them.
    active_runs: Arc<Mutex<HashSet<String>>>,
    /// Captured logs per run_id.
    logs: Arc<Mutex<HashMap<String, Vec<LogEntry>>>>,
}

impl LogStore {
    pub fn new() -> Self {
        Self {
            active_runs: Arc::new(Mutex::new(HashSet::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start capturing logs for a run. Multiple runs can be active simultaneously.
    pub fn start_capture(&self, run_id: &str) {
        self.active_runs.lock().unwrap().insert(run_id.to_string());
    }

    /// Stop capturing logs for a specific run.
    pub fn stop_capture(&self, run_id: &str) {
        self.active_runs.lock().unwrap().remove(run_id);
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

    /// Remove logs for a completed run (free memory after persistence).
    pub fn remove_logs(&self, run_id: &str) {
        self.logs.lock().unwrap().remove(run_id);
    }

    /// Push a log entry to all active runs.
    fn push(&self, entry: LogEntry) {
        let active = self.active_runs.lock().unwrap().clone();
        if active.is_empty() {
            return;
        }
        let mut logs = self.logs.lock().unwrap();
        for run_id in &active {
            logs.entry(run_id.clone()).or_default().push(entry.clone());
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

/// A tracing Layer that captures events from agent crates into per-run log buffers.
///
/// Captures events from: alva_llm_provider, alva_kernel_core, alva_host_native,
/// alva_agent_extension_builtin, alva_agent_security, alva_app_core, alva_app_eval.
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
            && !target.starts_with("alva_kernel_core")
            && !target.starts_with("alva_host_native")
            && !target.starts_with("alva_agent_extension_builtin")
            && !target.starts_with("alva_agent_security")
            && !target.starts_with("alva_app_core")
            && !target.starts_with("alva_app_eval")
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
