// INPUT:  job-owned host path, guest AuditEvent, kernel middleware tool hooks, chrono, serde_json
// OUTPUT: JobToolLogger, JobToolLogMiddleware, JOB_TOOLS_LOG_ENV, JOB_TOOLS_LOG_FILE
// POS:    CLI host-side JSONL audit sink shared by native middleware and versioned WASIp1 guest events.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alva_kernel_abi::{ToolCall, ToolContent, ToolOutput};
use alva_kernel_core::{AgentState, Middleware, MiddlewareError, MiddlewarePriority};
use alva_sandbox_wasm::AuditEvent;
use async_trait::async_trait;
use serde::Serialize;

pub(crate) const JOB_TOOLS_LOG_ENV: &str = "ALVA_JOB_TOOLS_LOG";
pub(crate) const JOB_TOOLS_LOG_FILE: &str = "tools.jsonl";
const MAX_RESULT_SUMMARY_CHARS: usize = 512;

#[derive(Debug, Serialize)]
struct ToolLogEntry<'a> {
    timestamp_ms: i64,
    kind: &'a str,
    tool_call_id: &'a str,
    tool_name: &'a str,
    is_error: bool,
    result_summary: String,
}

/// Host-owned append-only logger. The `kind` discriminator deliberately
/// leaves room for future elevation-request/elevation-decision entries once
/// that channel exists; Ticket 11 records only completed tool calls.
pub(crate) struct JobToolLogger {
    path: PathBuf,
    seen_tool_calls: Mutex<HashSet<String>>,
}

impl JobToolLogger {
    pub(crate) fn from_env() -> Option<Arc<Self>> {
        std::env::var_os(JOB_TOOLS_LOG_ENV).map(|path| {
            Arc::new(Self {
                path: PathBuf::from(path),
                seen_tool_calls: Mutex::new(HashSet::new()),
            })
        })
    }

    #[cfg(test)]
    fn new(path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            path,
            seen_tool_calls: Mutex::new(HashSet::new()),
        })
    }

    pub(crate) fn record_output(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        result: &ToolOutput,
    ) -> std::io::Result<()> {
        self.record(tool_call_id, tool_name, result.is_error, &result.content)
    }

    pub(crate) fn record_event(&self, event: AuditEvent) -> std::io::Result<()> {
        self.record_summary(
            &event.kind,
            &event.tool_call_id,
            &event.tool_name,
            event.is_error,
            event.result_summary,
        )
    }

    fn record(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        is_error: bool,
        content: &[ToolContent],
    ) -> std::io::Result<()> {
        self.record_summary(
            "tool_call",
            tool_call_id,
            tool_name,
            is_error,
            summarize(content),
        )
    }

    fn record_summary(
        &self,
        kind: &str,
        tool_call_id: &str,
        tool_name: &str,
        is_error: bool,
        result_summary: String,
    ) -> std::io::Result<()> {
        let mut seen = self
            .seen_tool_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dedupe_key = format!("{kind}\0{tool_name}\0{tool_call_id}");
        if !seen.insert(dedupe_key.clone()) {
            return Ok(());
        }
        let entry = ToolLogEntry {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            kind,
            tool_call_id,
            tool_name,
            is_error,
            result_summary,
        };
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            serde_json::to_writer(&mut file, &entry)?;
            file.write_all(b"\n")?;
            file.flush()
        })();
        if write_result.is_err() {
            seen.remove(&dedupe_key);
        }
        write_result
    }
}

fn summarize(content: &[ToolContent]) -> String {
    let raw = content
        .iter()
        .map(ToolContent::to_model_string)
        .collect::<Vec<_>>()
        .join("\n");
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let summary = chars
        .by_ref()
        .take(MAX_RESULT_SUMMARY_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{summary}…")
    } else {
        summary
    }
}

pub(crate) struct JobToolLogMiddleware {
    logger: Arc<JobToolLogger>,
}

impl JobToolLogMiddleware {
    pub(crate) fn new(logger: Arc<JobToolLogger>) -> Self {
        Self { logger }
    }
}

#[async_trait]
impl Middleware for JobToolLogMiddleware {
    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        if let Err(error) = self
            .logger
            .record_output(&tool_call.id, &tool_call.name, result)
        {
            tracing::warn!(error = %error, "failed to append job tool log");
        }
        Ok(())
    }

    fn priority(&self) -> i32 {
        MiddlewarePriority::OBSERVATION
    }

    fn name(&self) -> &str {
        "job-tool-log"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_output_uses_extensible_jsonl_shape_and_bounded_summary() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(JOB_TOOLS_LOG_FILE);
        let logger = JobToolLogger::new(path.clone());
        let output = ToolOutput::text(format!("line one\n{}", "界".repeat(600)));

        logger
            .record_output("call-1", "read_file", &output)
            .unwrap();

        let line = std::fs::read_to_string(path).unwrap();
        let entry: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(entry["kind"], "tool_call");
        assert_eq!(entry["tool_call_id"], "call-1");
        assert_eq!(entry["tool_name"], "read_file");
        assert_eq!(entry["is_error"], false);
        assert!(entry["timestamp_ms"].as_i64().unwrap() > 0);
        let summary = entry["result_summary"].as_str().unwrap();
        assert!(summary.starts_with("line one "));
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), MAX_RESULT_SUMMARY_CHARS + 1);
    }

    #[test]
    fn guest_events_use_the_same_shape_and_deduplicate_call_ids() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(JOB_TOOLS_LOG_FILE);
        let logger = JobToolLogger::new(path.clone());
        let event = AuditEvent::tool_call("call-write", "create_file", false, "created b.txt");

        logger.record_event(event.clone()).unwrap();
        logger.record_event(event).unwrap();

        let lines = std::fs::read_to_string(path).unwrap();
        let entries = lines.lines().collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let entry: serde_json::Value = serde_json::from_str(entries[0]).unwrap();
        assert_eq!(entry["tool_name"], "create_file");
        assert_eq!(entry["result_summary"], "created b.txt");
    }
}
