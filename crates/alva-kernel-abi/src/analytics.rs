// INPUT:  serde, std::time, std::path
// OUTPUT: AnalyticsEvent, AnalyticsSink, NoopAnalyticsSink
// POS:    Cross-crate event channel for telemetry. The trait lives here so
//         kernel-core can emit LLM-call events without depending on app-core,
//         while concrete sinks (JSONL, OTLP, …) live further up the stack.

//! Analytics event channel.
//!
//! `AnalyticsSink` is a bus capability that any layer (kernel-core,
//! middleware, host) can publish events into. The bus discovery means a
//! caller only needs to do `bus.get::<dyn AnalyticsSink>()` — if no sink
//! is registered, calls are no-ops and the agent path stays unaffected.
//!
//! Design notes:
//! - `record()` is sync + non-blocking; sinks must not perform I/O on the
//!   caller thread. Buffer + flush async if needed.
//! - Errors are swallowed inside sinks. Telemetry must never break the
//!   agent loop.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// One observable event in the agent's lifecycle. Tagged enum for
/// JSON-friendly serialization (each variant becomes a discriminator).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnalyticsEvent {
    SessionStart {
        session_id: String,
        workspace: PathBuf,
        ts: SystemTime,
    },
    SessionEnd {
        session_id: String,
        duration_ms: u64,
        ts: SystemTime,
    },
    ToolCallStart {
        session_id: String,
        tool: String,
        call_id: String,
        ts: SystemTime,
    },
    ToolCallEnd {
        session_id: String,
        tool: String,
        call_id: String,
        latency_ms: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        ts: SystemTime,
    },
    LlmCall {
        session_id: String,
        provider: String,
        model: String,
        #[serde(default)]
        input_tokens: u32,
        #[serde(default)]
        output_tokens: u32,
        #[serde(default)]
        cache_read: u32,
        #[serde(default)]
        cache_write: u32,
        #[serde(default)]
        cost_usd: f64,
        latency_ms: u64,
        ts: SystemTime,
    },
}

/// Bus Capability: telemetry sink. Multiple producers (kernel-core,
/// middleware, host) record events; one sink consumes them.
///
/// **Provider**: outer app via an `Extension` (e.g. `AnalyticsExtension`
/// in `alva-app-core`) that publishes a concrete impl on the bus.
/// **Consumers**: every emit point that wants to be observed must be
/// tolerant to the sink being absent — `bus.get::<dyn AnalyticsSink>()`
/// returns `None` and the call site no-ops.
/// **Why bus**: trait lives in `kernel-abi` so kernel-core can emit
/// without compile-depending on app-core where the JSONL sink lives.
#[crate::bus_cap]
pub trait AnalyticsSink: Send + Sync {
    /// Record an event. MUST be non-blocking. Sinks that buffer/flush
    /// asynchronously should hand off and return immediately. Failures
    /// are swallowed by the sink — telemetry must not break the agent.
    fn record(&self, event: AnalyticsEvent);
}

/// Sink that drops every event. Useful for tests and as the implicit
/// default when no concrete sink is on the bus (callers should just
/// not emit at all in that case, but the noop sink is sometimes handy).
pub struct NoopAnalyticsSink;

impl AnalyticsSink for NoopAnalyticsSink {
    fn record(&self, _event: AnalyticsEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> SystemTime {
        SystemTime::now()
    }

    #[test]
    fn session_start_round_trip() {
        let ev = AnalyticsEvent::SessionStart {
            session_id: "s1".into(),
            workspace: PathBuf::from("/w"),
            ts: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"session_start\""));
        let back: AnalyticsEvent = serde_json::from_str(&json).unwrap();
        match back {
            AnalyticsEvent::SessionStart { session_id, workspace, .. } => {
                assert_eq!(session_id, "s1");
                assert_eq!(workspace, PathBuf::from("/w"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn session_end_round_trip() {
        let ev = AnalyticsEvent::SessionEnd {
            session_id: "s2".into(),
            duration_ms: 1234,
            ts: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"session_end\""));
        assert!(json.contains("\"duration_ms\":1234"));
    }

    #[test]
    fn tool_call_round_trip() {
        let start = AnalyticsEvent::ToolCallStart {
            session_id: "s".into(),
            tool: "edit_file".into(),
            call_id: "c1".into(),
            ts: now(),
        };
        let end = AnalyticsEvent::ToolCallEnd {
            session_id: "s".into(),
            tool: "edit_file".into(),
            call_id: "c1".into(),
            latency_ms: 42,
            ok: true,
            error: None,
            ts: now(),
        };
        let _ = serde_json::to_string(&start).unwrap();
        let json = serde_json::to_string(&end).unwrap();
        let back: AnalyticsEvent = serde_json::from_str(&json).unwrap();
        match back {
            AnalyticsEvent::ToolCallEnd { ok, latency_ms, .. } => {
                assert!(ok);
                assert_eq!(latency_ms, 42);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn llm_call_round_trip() {
        let ev = AnalyticsEvent::LlmCall {
            session_id: "s".into(),
            provider: "anthropic".into(),
            model: "claude-opus-4-7".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read: 800,
            cache_write: 200,
            cost_usd: 0.012,
            latency_ms: 700,
            ts: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: AnalyticsEvent = serde_json::from_str(&json).unwrap();
        match back {
            AnalyticsEvent::LlmCall { provider, input_tokens, cost_usd, .. } => {
                assert_eq!(provider, "anthropic");
                assert_eq!(input_tokens, 1000);
                assert!((cost_usd - 0.012).abs() < 1e-9);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn missing_optional_fields_default() {
        let json = r#"{"type":"llm_call","session_id":"s","provider":"p","model":"m","latency_ms":100,"ts":{"secs_since_epoch":0,"nanos_since_epoch":0}}"#;
        let back: AnalyticsEvent = serde_json::from_str(json).unwrap();
        match back {
            AnalyticsEvent::LlmCall { input_tokens, cache_read, cost_usd, .. } => {
                assert_eq!(input_tokens, 0);
                assert_eq!(cache_read, 0);
                assert_eq!(cost_usd, 0.0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn noop_sink_is_safe() {
        let s = NoopAnalyticsSink;
        s.record(AnalyticsEvent::SessionStart {
            session_id: "x".into(),
            workspace: PathBuf::new(),
            ts: now(),
        });
        // No assertions — must just not panic.
    }
}
