// INPUT:  std::sync, std::time, alva_kernel_abi::{AnalyticsEvent, AnalyticsSink, BusHandle, ToolCall, ToolOutput}, alva_kernel_core::{middleware::{Middleware, MiddlewareContext, MiddlewareError}, state::AgentState}, async_trait
// OUTPUT: AnalyticsPlugin, AnalyticsMiddleware
// POS:    Telemetry pipeline. Extension publishes a JsonlSink on the bus and installs
//         a middleware that emits ToolCallStart/ToolCallEnd events around every tool call.
//         The trait + event types live in kernel-abi so kernel-core can also emit
//         (e.g. LlmCall events from run.rs); this module is the storage + tool-level
//         observation layer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use async_trait::async_trait;

use alva_agent_core::extension::{PhaseContribution, PhaseHandler, PhaseOrder};
use alva_kernel_abi::{
    AnalyticsEvent, AnalyticsSink, BusHandle, Phase, PhaseEffect, ToolCall, ToolContent, ToolOutput,
};
use alva_kernel_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_kernel_core::state::AgentState;

use crate::analytics::JsonlSink;

use super::{Plugin, Registrar};

/// Telemetry extension. Owns a `JsonlSink` writing to
/// `<workspace>/.alva/analytics.jsonl` (override via [`Self::with_path`])
/// and an `AnalyticsMiddleware` that records tool-call latency.
pub struct AnalyticsPlugin {
    path_override: Option<PathBuf>,
    sink: OnceLock<Arc<JsonlSink>>,
}

impl AnalyticsPlugin {
    pub fn new() -> Self {
        Self {
            path_override: None,
            sink: OnceLock::new(),
        }
    }

    /// Override the JSONL output path (default:
    /// `<workspace>/.alva/analytics.jsonl`).
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path_override = Some(path.into());
        self
    }
}

impl Default for AnalyticsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for AnalyticsPlugin {
    fn name(&self) -> &str {
        "analytics"
    }

    fn description(&self) -> &str {
        "JSONL telemetry sink + tool-call latency middleware"
    }

    async fn register(&self, r: &Registrar) {
        // Build + provide the sink (was `configure()`).
        let path = self
            .path_override
            .clone()
            .unwrap_or_else(|| r.workspace().join(".alva").join("analytics.jsonl"));
        match JsonlSink::new(&path) {
            Ok(sink) => {
                let arc = Arc::new(sink);
                let recorder_sink: Arc<dyn AnalyticsSink> = arc.clone();
                r.provide::<dyn AnalyticsSink>(arc.clone());
                let _ = self.sink.set(arc);
                let recorder = Arc::new(AnalyticsMiddleware::with_sink(recorder_sink));
                r.phase_handler(Arc::new(AnalyticsPhaseHandler::new(
                    PhaseContribution::new(
                        "analytics-tool-start",
                        Phase::BeforeToolCall,
                        PhaseEffect::Observe,
                        PhaseOrder::Telemetry,
                    ),
                    recorder.clone(),
                )));
                r.phase_handler(Arc::new(AnalyticsPhaseHandler::new(
                    PhaseContribution::new(
                        "analytics-tool-end",
                        Phase::AfterToolCall,
                        PhaseEffect::Observe,
                        PhaseOrder::Telemetry,
                    ),
                    recorder,
                )));
            }
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "analytics sink open failed");
            }
        }
    }
}

/// Middleware that emits `ToolCallStart` / `ToolCallEnd` around every
/// tool execution. State is keyed by `tool_call.id` (start time + tool
/// name) so concurrent calls are tracked independently.
///
/// Reads `dyn AnalyticsSink` from the bus on first use; if absent, all
/// emits are no-ops. Sink failures are swallowed inside the sink so the
/// agent loop never breaks.
pub struct AnalyticsMiddleware {
    bus: OnceLock<BusHandle>,
    direct_sink: Option<Arc<dyn AnalyticsSink>>,
    starts: Mutex<HashMap<String, StartEntry>>,
}

struct StartEntry {
    instant: Instant,
    tool: String,
}

impl AnalyticsMiddleware {
    pub fn new() -> Self {
        Self {
            bus: OnceLock::new(),
            direct_sink: None,
            starts: Mutex::new(HashMap::new()),
        }
    }

    fn with_sink(sink: Arc<dyn AnalyticsSink>) -> Self {
        Self {
            bus: OnceLock::new(),
            direct_sink: Some(sink),
            starts: Mutex::new(HashMap::new()),
        }
    }

    fn sink(&self) -> Option<Arc<dyn AnalyticsSink>> {
        if let Some(sink) = self.direct_sink.as_ref() {
            return Some(sink.clone());
        }
        self.bus.get().and_then(|b| b.get::<dyn AnalyticsSink>())
    }

    fn session_id(&self, state: &AgentState) -> String {
        state.session.session_id().to_string()
    }

    async fn record_before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        let session_id = self.session_id(state);
        let now = Instant::now();
        {
            let mut s = self.starts.lock().unwrap_or_else(|e| e.into_inner());
            s.insert(
                tool_call.id.clone(),
                StartEntry {
                    instant: now,
                    tool: tool_call.name.clone(),
                },
            );
        }
        if let Some(sink) = self.sink() {
            sink.record(AnalyticsEvent::ToolCallStart {
                session_id,
                tool: tool_call.name.clone(),
                call_id: tool_call.id.clone(),
                ts: SystemTime::now(),
            });
        }
        Ok(())
    }

    async fn record_after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        let session_id = self.session_id(state);
        let entry = {
            let mut s = self.starts.lock().unwrap_or_else(|e| e.into_inner());
            s.remove(&tool_call.id)
        };
        let (latency_ms, tool_name) = match entry {
            Some(e) => (e.instant.elapsed().as_millis() as u64, e.tool),
            // Shouldn't happen, but be defensive — emit with 0 latency
            // so we don't lose the End event.
            None => (0, tool_call.name.clone()),
        };
        if let Some(sink) = self.sink() {
            let (ok, error) = match result.is_error {
                false => (true, None),
                true => (
                    false,
                    Some(
                        result
                            .content
                            .iter()
                            .filter_map(ToolContent::as_text)
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                ),
            };
            sink.record(AnalyticsEvent::ToolCallEnd {
                session_id,
                tool: tool_name,
                call_id: tool_call.id.clone(),
                latency_ms,
                ok,
                error,
                ts: SystemTime::now(),
            });
        }
        Ok(())
    }
}

impl Default for AnalyticsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for AnalyticsMiddleware {
    fn configure(&self, ctx: &MiddlewareContext) {
        if let Some(bus) = ctx.bus.clone() {
            let _ = self.bus.set(bus);
        }
    }

    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        self.record_before_tool_call(state, tool_call).await
    }

    async fn after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        self.record_after_tool_call(state, tool_call, result).await
    }
}

struct AnalyticsPhaseHandler {
    contribution: PhaseContribution,
    recorder: Arc<AnalyticsMiddleware>,
}

impl AnalyticsPhaseHandler {
    fn new(contribution: PhaseContribution, recorder: Arc<AnalyticsMiddleware>) -> Self {
        Self {
            contribution,
            recorder,
        }
    }
}

#[async_trait]
impl PhaseHandler for AnalyticsPhaseHandler {
    fn contribution(&self) -> PhaseContribution {
        self.contribution.clone()
    }

    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        self.recorder
            .record_before_tool_call(state, tool_call)
            .await
    }

    async fn after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        self.recorder
            .record_after_tool_call(state, tool_call, result)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::ToolContent;

    #[test]
    fn extension_metadata() {
        let e = AnalyticsPlugin::new();
        assert_eq!(e.name(), "analytics");
        assert!(!e.description().is_empty());
    }

    #[test]
    fn middleware_handles_missing_sink() {
        // No bus configured — sink() returns None — record paths are no-ops.
        let mw = AnalyticsMiddleware::new();
        assert!(mw.sink().is_none());
    }

    #[tokio::test]
    async fn plugin_registers_tool_latency_as_phase_handler() {
        let dir = tempfile::tempdir().expect("tempdir");
        let analytics_path = dir.path().join("analytics.jsonl");
        let model = Arc::new(alva_test::mock_provider::MockLanguageModel::new());

        let agent = alva_agent_core::Agent::builder()
            .workspace(dir.path())
            .model(model)
            .plugin(Box::new(AnalyticsPlugin::new().with_path(&analytics_path)))
            .build()
            .await
            .expect("agent should build");

        let snapshot = agent.assembly_snapshot();
        let plugin = snapshot
            .plugins
            .iter()
            .find(|plugin| plugin.name == "analytics")
            .expect("analytics plugin snapshot");

        assert_eq!(
            plugin.phase_contribution_names,
            vec!["analytics-tool-start", "analytics-tool-end"]
        );
        assert!(
            snapshot
                .middleware_names
                .iter()
                .any(|name| name == "phase:analytics-tool-start"),
            "start phase handler should compile into middleware during transition: {:?}",
            snapshot.middleware_names
        );
        assert!(
            snapshot
                .middleware_names
                .iter()
                .any(|name| name == "phase:analytics-tool-end"),
            "phase handler should compile into middleware during transition: {:?}",
            snapshot.middleware_names
        );
        assert!(
            !plugin
                .middleware_names
                .iter()
                .any(|name| name == "analytics"),
            "analytics should be registered semantically as a phase contribution"
        );

        let tool_call = ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({ "path": "README.md" }),
        };
        let mut result = ToolOutput::text("ok");
        let config = agent.config().await;
        let mut state = agent.state().lock().await;
        config
            .middleware
            .run_before_tool_call(&mut state, &tool_call)
            .await
            .expect("before phase should run");
        config
            .middleware
            .run_after_tool_call(&mut state, &tool_call, &mut result)
            .await
            .expect("after phase should run");
        drop(state);

        let jsonl = std::fs::read_to_string(&analytics_path).expect("analytics jsonl");
        assert!(jsonl.contains("\"type\":\"tool_call_start\""));
        assert!(jsonl.contains("\"type\":\"tool_call_end\""));
        assert!(jsonl.contains("\"call_id\":\"call-1\""));
    }

    #[test]
    fn middleware_tracks_start_then_clears() {
        let mw = AnalyticsMiddleware::new();
        {
            let mut s = mw.starts.lock().unwrap();
            s.insert(
                "c1".into(),
                StartEntry {
                    instant: Instant::now(),
                    tool: "edit".into(),
                },
            );
        }
        // Simulating after_tool_call cleanup
        let mut s = mw.starts.lock().unwrap();
        let entry = s.remove("c1");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().tool, "edit");
        assert!(s.is_empty());
    }

    /// Text blocks pass through the error-extraction join.
    #[test]
    fn error_text_extraction() {
        let blocks = vec![ToolContent::text("first"), ToolContent::text("second")];
        let joined = blocks
            .iter()
            .filter_map(ToolContent::as_text)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(joined, "first second");
    }
}
