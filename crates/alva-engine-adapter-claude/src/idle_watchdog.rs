// INPUT:  ClaudeAdapterConfig (optional override), env ALVA_CLAUDE_BRIDGE_IDLE_TIMEOUT_MS
// OUTPUT: resolve_bridge_idle_timeout, IdleActivityGate, schedule_idle, idle trip message
// POS:    Host-side bridge idle watchdog — last-line defense when the Node/SDK child goes silent.

//! Bridge idle watchdog (cindy `upstream_response_idle_timeout`).
//!
//! # Layering
//!
//! cindy splits recovery into two layers:
//! 1. **In-process** (cc-code): `CLAUDE_ENABLE_STREAM_WATCHDOG` (~300s) +
//!    `API_TIMEOUT_MS` non-stream fallback — transparent to the host.
//! 2. **Host** (this module): end-to-end silence on bridge stdout while the
//!    ball is with the upstream model. Default **30 minutes**, above the
//!    ~20min in-process budget so normal recovery is not pre-empted.
//!
//! # When the timer runs
//!
//! Only while the client is waiting on the model:
//! - open tools / pending permission requests → **pause** (ball is local/UI)
//! - any other time after session start → **arm**
//!
//! Tool-arg silence during a long Bash/MCP call must not trip the watchdog.

use std::collections::HashSet;
use std::pin::Pin;
use std::time::Duration;

use tokio::time::{Instant, Sleep};

use alva_engine_runtime::RuntimeEvent;

use crate::config::ClaudeAdapterConfig;

/// Default host idle timeout (30 min). Matches cindy maker default.
pub const DEFAULT_BRIDGE_IDLE_TIMEOUT_MS: u64 = 1_800_000;

/// Env override (milliseconds). `0` disables the host watchdog.
pub const BRIDGE_IDLE_TIMEOUT_ENV: &str = "ALVA_CLAUDE_BRIDGE_IDLE_TIMEOUT_MS";

/// Resolve idle timeout from config, then env, then default.
///
/// Returns [`Duration::ZERO`] when disabled.
pub fn resolve_bridge_idle_timeout(config: &ClaudeAdapterConfig) -> Duration {
    let ms = config
        .bridge_idle_timeout_ms
        .unwrap_or_else(parse_idle_timeout_ms_from_env);
    Duration::from_millis(ms)
}

fn parse_idle_timeout_ms_from_env() -> u64 {
    parse_idle_timeout_ms(std::env::var(BRIDGE_IDLE_TIMEOUT_ENV).ok().as_deref())
}

/// Parse raw ms string. Invalid / negative → default. Empty → default.
pub fn parse_idle_timeout_ms(raw: Option<&str>) -> u64 {
    match raw {
        None | Some("") => DEFAULT_BRIDGE_IDLE_TIMEOUT_MS,
        Some(s) => match s.parse::<u64>() {
            Ok(n) => n,
            Err(_) => DEFAULT_BRIDGE_IDLE_TIMEOUT_MS,
        },
    }
}

/// Tracks whether the "ball" is with the upstream model.
#[derive(Debug, Default)]
pub struct IdleActivityGate {
    open_tools: HashSet<String>,
    pending_permissions: HashSet<String>,
}

impl IdleActivityGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when host should count silence against the idle budget.
    pub fn waiting_on_upstream(&self) -> bool {
        self.open_tools.is_empty() && self.pending_permissions.is_empty()
    }

    pub fn open_tool_count(&self) -> usize {
        self.open_tools.len()
    }

    pub fn pending_permission_count(&self) -> usize {
        self.pending_permissions.len()
    }

    /// Update gate from a mapped runtime event.
    pub fn observe(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::ToolStart { id, .. } => {
                self.open_tools.insert(id.clone());
                // Stream resumed past any permission prompt.
                self.pending_permissions.clear();
            }
            RuntimeEvent::ToolEnd { id, .. } => {
                self.open_tools.remove(id);
                self.pending_permissions.clear();
            }
            RuntimeEvent::PermissionRequest { request_id, .. } => {
                self.pending_permissions.insert(request_id.clone());
            }
            RuntimeEvent::Message { .. }
            | RuntimeEvent::MessageDelta { .. }
            | RuntimeEvent::SessionStarted { .. }
            | RuntimeEvent::Completed { .. }
            | RuntimeEvent::Error { .. } => {
                // Productive bridge traffic after a permission wait.
                self.pending_permissions.clear();
            }
        }
    }
}

/// Far-future deadline used to pause a [`Sleep`] without dropping it.
pub fn far_future_deadline() -> Instant {
    Instant::now() + Duration::from_secs(86_400 * 365)
}

/// Arm or pause the idle sleep based on timeout + activity gate.
pub fn schedule_idle(idle: &mut Pin<Box<Sleep>>, timeout: Duration, waiting_on_upstream: bool) {
    if timeout.is_zero() || !waiting_on_upstream {
        idle.as_mut().reset(far_future_deadline());
    } else {
        idle.as_mut().reset(Instant::now() + timeout);
    }
}

/// Fatal error message when the host idle watchdog trips.
pub fn build_idle_timeout_message(idle: Duration) -> String {
    let secs = idle.as_secs().max(1);
    format!(
        "upstream_response_idle_timeout: bridge produced no progress for {secs}s while waiting \
         on the model; session interrupted to avoid an indefinite hang. Send another message to continue."
    )
}

/// Inject cindy/cc-code in-process stream recovery env if the caller has not
/// already set them (first layer of the two-layer watchdog).
pub fn ensure_in_process_stream_watchdog_env(env: &mut std::collections::HashMap<String, String>) {
    env.entry("CLAUDE_ENABLE_STREAM_WATCHDOG".into())
        .or_insert_with(|| "true".into());
    // 900s non-stream fallback budget after the ~300s stream inactivity trip.
    env.entry("API_TIMEOUT_MS".into())
        .or_insert_with(|| "900000".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::ToolOutput;
    use serde_json::json;

    #[test]
    fn parse_idle_timeout_defaults_and_zero_disable() {
        assert_eq!(
            parse_idle_timeout_ms(None),
            DEFAULT_BRIDGE_IDLE_TIMEOUT_MS
        );
        assert_eq!(
            parse_idle_timeout_ms(Some("")),
            DEFAULT_BRIDGE_IDLE_TIMEOUT_MS
        );
        assert_eq!(parse_idle_timeout_ms(Some("0")), 0);
        assert_eq!(parse_idle_timeout_ms(Some("5000")), 5000);
        assert_eq!(
            parse_idle_timeout_ms(Some("nope")),
            DEFAULT_BRIDGE_IDLE_TIMEOUT_MS
        );
    }

    #[test]
    fn gate_pauses_during_tools_and_permissions() {
        let mut gate = IdleActivityGate::new();
        assert!(gate.waiting_on_upstream());

        gate.observe(&RuntimeEvent::PermissionRequest {
            request_id: "p1".into(),
            tool_name: "Bash".into(),
            tool_input: json!({}),
            description: None,
        });
        assert!(!gate.waiting_on_upstream());
        assert_eq!(gate.pending_permission_count(), 1);

        gate.observe(&RuntimeEvent::ToolStart {
            id: "t1".into(),
            name: "Bash".into(),
            input: json!({}),
        });
        assert!(!gate.waiting_on_upstream());
        assert_eq!(gate.pending_permission_count(), 0);
        assert_eq!(gate.open_tool_count(), 1);

        gate.observe(&RuntimeEvent::ToolEnd {
            id: "t1".into(),
            name: "Bash".into(),
            result: ToolOutput::text("ok"),
            duration_ms: None,
        });
        assert!(gate.waiting_on_upstream());
    }

    #[test]
    fn message_clears_pending_permission() {
        let mut gate = IdleActivityGate::new();
        gate.observe(&RuntimeEvent::PermissionRequest {
            request_id: "p1".into(),
            tool_name: "AskUserQuestion".into(),
            tool_input: json!({}),
            description: None,
        });
        assert!(!gate.waiting_on_upstream());
        gate.observe(&RuntimeEvent::Message {
            id: "m".into(),
            role: alva_kernel_abi::MessageRole::Assistant,
            content: vec![],
        });
        assert!(gate.waiting_on_upstream());
    }

    #[test]
    fn ensure_stream_watchdog_env_is_append_only() {
        let mut env = std::collections::HashMap::new();
        env.insert("API_TIMEOUT_MS".into(), "1".into());
        ensure_in_process_stream_watchdog_env(&mut env);
        assert_eq!(env.get("API_TIMEOUT_MS").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("CLAUDE_ENABLE_STREAM_WATCHDOG").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn idle_message_names_reason() {
        let msg = build_idle_timeout_message(Duration::from_secs(1800));
        assert!(msg.contains("upstream_response_idle_timeout"));
        assert!(msg.contains("1800s"));
    }
}
