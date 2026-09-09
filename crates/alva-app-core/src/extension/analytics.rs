// INPUT:  std::sync, std::time, alva_agent_abi::{AnalyticsEvent, AnalyticsSink}, alva_kernel_abi::{BusHandle, ToolCall, ToolContent, ToolOutput, Message}, alva_kernel_core::{middleware::{Middleware, LlmCallFn, MiddlewareContext, MiddlewareError}, state::AgentState}, async_trait
// OUTPUT: AnalyticsPlugin, AnalyticsMiddleware
// POS:    Telemetry pipeline. Extension publishes a JsonlSink on the bus and installs
//         a middleware that emits ToolCallStart/ToolCallEnd around every tool call
//         and one LlmCall event per LLM turn (ticket 15: 自内核 run 循环迁来，
//         与工具埋点同为中间件采集，采集字段逐字段一致).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use async_trait::async_trait;

use alva_agent_abi::{AnalyticsEvent, AnalyticsSink};
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::{BusHandle, ToolCall, ToolContent, ToolOutput};
use alva_kernel_core::middleware::{LlmCallFn, Middleware, MiddlewareContext, MiddlewareError};
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
                // 同一份 middleware 覆盖工具埋点（before/after tool call）
                // 与 LLM 埋点（wrap_llm_call，ticket 15 迁入）。以原始
                // middleware 注册——此前只经 PhaseHandler 编译 before/after
                // 阶段，wrap 钩子从未被调用，生产插件一轮 LLM 后埋点为空
                //（R02）。单一注册点也保证工具事件不因双份注册而重复。
                // （AnalyticsPhaseHandler 与两份 phase_handler 注册已删除：
                //  它们只是把同一 recorder 的同名钩子再包一层。）
                let recorder = Arc::new(AnalyticsMiddleware::with_sink(recorder_sink));
                r.middleware(recorder);
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
        state: &AgentState,
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
        state: &AgentState,
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
        state: &AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        self.record_before_tool_call(state, tool_call).await
    }

    async fn after_tool_call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        self.record_after_tool_call(state, tool_call, result).await
    }

    async fn wrap_llm_call(
        &self,
        state: &AgentState,
        messages: Vec<Message>,
        next: &dyn LlmCallFn,
    ) -> Result<Message, MiddlewareError> {
        // LLM 埋点（ticket 15 自内核迁来）：耗时从 wrap 入口计时，usage 取
        // 自响应，字段与原先内核直发的 LlmCall 埋点逐字段一致。内核循环
        // 出错路径不产埋点——本钩子在 next 出错时同样直接透传错误，不发
        // 事件，行为一致。
        let started = Instant::now();
        let response = next.call(state, messages).await?;
        if let Some(sink) = self.sink() {
            let usage = response.usage.as_ref();
            sink.record(AnalyticsEvent::LlmCall {
                session_id: self.session_id(state),
                provider: state.model.provider_id().to_string(),
                model: state.model.model_id().to_string(),
                input_tokens: usage.map(|u| u.input_tokens).unwrap_or(0),
                output_tokens: usage.map(|u| u.output_tokens).unwrap_or(0),
                cache_read: usage.and_then(|u| u.cache_read_input_tokens).unwrap_or(0),
                cache_write: usage
                    .and_then(|u| u.cache_creation_input_tokens)
                    .unwrap_or(0),
                cost_usd: 0.0,
                latency_ms: started.elapsed().as_millis() as u64,
                // The analytics ABI field is std::time::SystemTime, whose
                // `now()` panics on wasm32. Take "now" from web_time and
                // rebase it onto the std type — identical to the previous
                // kernel-side emission.
                ts: std::time::SystemTime::UNIX_EPOCH
                    + web_time::SystemTime::now()
                        .duration_since(web_time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default(),
            });
        }
        Ok(response)
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
        let state = agent.state().lock().await;
        config
            .middleware
            .run_before_tool_call(&state, &tool_call)
            .await
            .expect("before phase should run");
        config
            .middleware
            .run_after_tool_call(&state, &tool_call, &mut result)
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
        let blocks = [ToolContent::text("first"), ToolContent::text("second")];
        let joined = blocks
            .iter()
            .filter_map(ToolContent::as_text)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(joined, "first second");
    }

    /// Ticket 15 acceptance: one full turn must produce an `LlmCall` event
    /// with every field identical to what the kernel loop used to emit
    /// directly (session/provider/model/tokens/cache/cost/latency/ts) —
    /// proving the move to the middleware hook lost no data.
    #[tokio::test]
    async fn llm_call_event_fields_match_the_previous_kernel_emission() {
        use std::pin::Pin;
        use std::sync::Arc as StdArc;

        use alva_agent_core::agent_session::InMemoryAgentSession;
        use alva_kernel_abi::base::stream::StreamEvent;
        use alva_kernel_abi::model::LanguageModel;
        use alva_kernel_abi::tool::Tool;
        use alva_kernel_abi::ConversationLog;
        use alva_kernel_abi::{
            AgentError, AgentMessage, CompletionResponse, ModelConfig, UsageMetadata,
        };
        use alva_kernel_core::run::run_agent;
        use alva_kernel_core::state::{AgentConfig, AgentState};
        use async_trait::async_trait as at;
        use futures::Stream;

        struct RecordingSink(StdArc<Mutex<Vec<AnalyticsEvent>>>);
        impl AnalyticsSink for RecordingSink {
            fn record(&self, event: AnalyticsEvent) {
                self.0.lock().unwrap().push(event);
            }
        }

        /// Model whose complete() returns a message carrying full usage.
        struct UsageModel;
        #[at]
        impl LanguageModel for UsageModel {
            fn model_id(&self) -> &str {
                "usage-model"
            }
            fn provider_id(&self) -> &str {
                "test-provider"
            }
            async fn complete(
                &self,
                _messages: &[alva_kernel_abi::Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Result<CompletionResponse, AgentError> {
                let mut message = alva_kernel_abi::Message {
                    id: "usage-msg".into(),
                    role: alva_kernel_abi::MessageRole::Assistant,
                    content: vec![alva_kernel_abi::ContentBlock::Text {
                        text: "done".into(),
                    }],
                    tool_call_id: None,
                    usage: None,
                    timestamp: 0,
                };
                message.usage = Some(UsageMetadata {
                    input_tokens: 100,
                    output_tokens: 20,
                    cache_creation_input_tokens: Some(3),
                    cache_read_input_tokens: Some(7),
                    total_tokens: 120,
                });
                Ok(CompletionResponse { message, raw: None })
            }
            fn stream(
                &self,
                _messages: &[alva_kernel_abi::Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
                // Empty stream → kernel falls back to complete(), exactly the
                // path the old kernel emission could take.
                Box::pin(tokio_stream::empty())
            }
        }

        let events: StdArc<Mutex<Vec<AnalyticsEvent>>> = Default::default();
        let mut stack = alva_kernel_core::middleware::MiddlewareStack::new();
        stack.push(Arc::new(AnalyticsMiddleware::with_sink(Arc::new(
            RecordingSink(events.clone()),
        ))));

        let session = StdArc::new(InMemoryAgentSession::new());
        let state = AgentState {
            model: StdArc::new(UsageModel),
            tools: vec![],
            session: session.clone(),
            extensions: alva_kernel_core::shared::Extensions::new(),
        };
        let config = AgentConfig {
            middleware: stack,
            system_prompt: vec![],
            max_iterations: 5,
            model_config: ModelConfig::default(),
            context_window: 0,
            workspace: None,
            bus: None,
            context_system: None,
            context_token_budget: None,
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        run_agent(
            &state,
            &config,
            alva_kernel_abi::CancellationToken::new(),
            vec![AgentMessage::Standard(alva_kernel_abi::Message::user("hi"))],
            tx,
        )
        .await
        .expect("run completes");
        while rx.recv().await.is_some() {}

        let guard = events.lock().unwrap();
        let llm = guard
            .iter()
            .find_map(|event| match event {
                AnalyticsEvent::LlmCall {
                    session_id,
                    provider,
                    model,
                    input_tokens,
                    output_tokens,
                    cache_read,
                    cache_write,
                    cost_usd,
                    latency_ms,
                    ts,
                } => Some((
                    session_id.clone(),
                    provider.clone(),
                    model.clone(),
                    *input_tokens,
                    *output_tokens,
                    *cache_read,
                    *cache_write,
                    *cost_usd,
                    *latency_ms,
                    *ts,
                )),
                _ => None,
            })
            .expect("one LlmCall event per turn must reach the sink");

        let (sess, provider, model, input, output, cache_read, cache_write, cost, latency, ts) =
            llm;
        assert_eq!(sess, session.session_id());
        assert_eq!(provider, "test-provider");
        assert_eq!(model, "usage-model");
        assert_eq!(input, 100);
        assert_eq!(output, 20);
        assert_eq!(cache_read, 7);
        assert_eq!(cache_write, 3);
        assert_eq!(cost, 0.0, "cost stays 0.0 — the kernel never knew prices");
        assert!(latency <= 30_000, "latency is a sane millisecond figure");
        assert!(
            ts > std::time::SystemTime::UNIX_EPOCH,
            "timestamp is a real wall-clock instant"
        );
    }
}
