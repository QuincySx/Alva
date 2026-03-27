// INPUT:  std::sync::Arc, parking_lot::RwLock, tracing, tracing_subscriber, crate::log_store::{LogQuery, LogQueryResponse, LogRecord, LogStore}
// OUTPUT: pub struct LogCaptureLayer, pub struct LogHandle
// POS:    Tracing subscriber layer that captures log events into an in-memory LogStore with dynamic filtering.
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::log_store::{LogQuery, LogQueryResponse, LogRecord, LogStore};

pub struct LogCaptureLayer {
    store: Arc<RwLock<LogStore>>,
    filter: Arc<RwLock<String>>,
}

#[derive(Clone)]
pub struct LogHandle {
    store: Arc<RwLock<LogStore>>,
    filter: Arc<RwLock<String>>,
}

impl LogCaptureLayer {
    pub fn new(capacity: usize) -> (Self, LogHandle) {
        let store = Arc::new(RwLock::new(LogStore::new(capacity)));
        let filter = Arc::new(RwLock::new("trace".to_string()));
        let layer = Self {
            store: Arc::clone(&store),
            filter: Arc::clone(&filter),
        };
        let handle = LogHandle { store, filter };
        (layer, handle)
    }
}

impl LogHandle {
    pub fn query(&self, params: &LogQuery) -> LogQueryResponse {
        self.store.read().query(params)
    }

    pub fn set_filter(&self, filter_str: &str) {
        let mut f = self.filter.write();
        *f = filter_str.to_string();
    }

    pub fn current_filter(&self) -> String {
        self.filter.read().clone()
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        // Check dynamic filter
        let filter_str = self.filter.read().clone();
        if !should_capture(event, &filter_str) {
            return;
        }

        // Extract message and fields
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Extract span stack
        let mut span_stack = Vec::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                span_stack.push(span.name().to_string());
            }
        }

        let record = LogRecord {
            seq: 0, // assigned by LogStore::push
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
            span_stack,
        };

        self.store.write().push(record);
    }
}

fn should_capture(event: &tracing::Event<'_>, filter_str: &str) -> bool {
    let target = event.metadata().target();
    let event_level = level_order(event.metadata().level());

    for directive in filter_str.split(',') {
        let directive = directive.trim();
        if directive.is_empty() {
            continue;
        }
        if let Some((module, level)) = directive.split_once('=') {
            if target.starts_with(module.trim()) {
                return event_level >= level_order_str(level.trim());
            }
        } else {
            return event_level >= level_order_str(directive);
        }
    }
    true
}

fn level_order(level: &tracing::Level) -> u8 {
    match *level {
        tracing::Level::TRACE => 0,
        tracing::Level::DEBUG => 1,
        tracing::Level::INFO => 2,
        tracing::Level::WARN => 3,
        tracing::Level::ERROR => 4,
    }
}

fn level_order_str(s: &str) -> u8 {
    match s.to_lowercase().as_str() {
        "trace" => 0,
        "debug" => 1,
        "info" => 2,
        "warn" => 3,
        "error" => 4,
        _ => 0,
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: std::collections::HashMap<String, serde_json::Value>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(format!("{:?}", value)),
            );
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn captures_log_events() {
        let (layer, handle) = LogCaptureLayer::new(100);
        let _guard = tracing_subscriber::registry().with(layer).set_default();

        tracing::info!(target: "test_mod", "hello world");
        tracing::warn!(target: "test_mod", server = "fs", "connection lost");

        let result = handle.query(&LogQuery::default());
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records[0].level, "INFO");
        assert_eq!(result.records[1].level, "WARN");
        assert!(result.records[1].fields.contains_key("server"));
    }

    #[test]
    fn captures_span_stack() {
        let (layer, handle) = LogCaptureLayer::new(100);
        let _guard = tracing_subscriber::registry().with(layer).set_default();

        let outer = tracing::info_span!("outer_span");
        let _outer_guard = outer.enter();
        let inner = tracing::info_span!("inner_span");
        let _inner_guard = inner.enter();
        tracing::info!("nested event");

        let result = handle.query(&LogQuery::default());
        assert_eq!(result.total_matches, 1);
        assert_eq!(
            result.records[0].span_stack,
            vec!["outer_span", "inner_span"]
        );
    }

    #[test]
    fn dynamic_filter_change() {
        let (layer, handle) = LogCaptureLayer::new(100);
        let _guard = tracing_subscriber::registry().with(layer).set_default();

        tracing::debug!(target: "test_mod", "debug msg");
        assert_eq!(handle.query(&LogQuery::default()).total_matches, 1);

        handle.set_filter("warn");
        tracing::debug!(target: "test_mod", "should be filtered");
        tracing::warn!(target: "test_mod", "should pass");

        let result = handle.query(&LogQuery::default());
        assert_eq!(result.total_matches, 2); // 1 original debug + 1 warn
    }
}
