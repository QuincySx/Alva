// INPUT:  dispatcher::HostHandler, protocol::{methods, error_codes, HostLogParams, ...}
// OUTPUT: AlvaHostHandler
// POS:    Phase 3.5 — the real host-side handler for plugin → host reverse calls.

//! The non-noop [`HostHandler`] implementation used by the real
//! subprocess loader.
//!
//! Phase 3.5 scope:
//!
//! - `host/log` — route to `tracing` at the level the plugin picked,
//!   with `target = "aep.plugin.host_log"` and a `plugin` field
//! - `host/notify` — same idea, `target = "aep.plugin.notify"`
//! - `host/emit_metric` — tracing with structured fields; real metric
//!   routing is a Phase 6 item
//!
//! Deliberately **not** in scope here:
//!
//! - `host/state.*` — requires carrying an `AgentState` handle
//!   through the host's event dispatch, which is an invasive change
//!   to `alva-agent-core::extension::events`. Methods return
//!   `METHOD_NOT_FOUND` until Phase 5 wires this up.
//! - `host/memory.*` — same reason, plus needs access to the current
//!   memory backend through the bus. Phase 6.
//! - `host/request_approval` — needs the approval channel from
//!   `ApprovalExtension`, which we do not have a reference to here.
//!   Phase 6 once we wire the approval bridge.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dispatcher::HostHandler;
use crate::protocol::{error_codes, HostLogParams, LogLevel, RpcError};

/// The production host handler wired in by `SubprocessLoaderExtension`.
///
/// Stateless for now — all it does is translate plugin RPC calls
/// into `tracing` events. Hold-everything-in-one-struct keeps Phase 3.5
/// diff-small; each future capability becomes a field here.
#[derive(Debug, Clone, Default)]
pub struct AlvaHostHandler {
    plugin_name: String,
}

impl AlvaHostHandler {
    pub fn new(plugin_name: impl Into<String>) -> Self {
        Self {
            plugin_name: plugin_name.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostNotifyParams {
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEmitMetricParams {
    pub name: String,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Value>,
}

#[async_trait]
impl HostHandler for AlvaHostHandler {
    async fn handle_request(
        &self,
        method: String,
        params: Option<Value>,
    ) -> Result<Value, RpcError> {
        match method.as_str() {
            "host/log" => {
                let params: HostLogParams = parse_params(params)?;
                self.do_log(params);
                Ok(Value::Object(Default::default()))
            }
            "host/notify" => {
                let params: HostNotifyParams = parse_params(params)?;
                self.do_notify(params);
                Ok(Value::Object(Default::default()))
            }
            "host/emit_metric" => {
                let params: HostEmitMetricParams = parse_params(params)?;
                self.do_emit_metric(params);
                Ok(Value::Object(Default::default()))
            }
            // Explicitly unimplemented — return a clear error so plugin
            // authors know what is coming vs what is broken.
            "host/state.get_messages"
            | "host/state.get_metadata"
            | "host/state.count_tokens" => Err(RpcError::new(
                error_codes::METHOD_NOT_FOUND,
                format!("{} is not yet implemented (Phase 5)", method),
            )),
            "host/memory.read" | "host/memory.write" => Err(RpcError::new(
                error_codes::METHOD_NOT_FOUND,
                format!("{} is not yet implemented (Phase 6)", method),
            )),
            "host/request_approval" => Err(RpcError::new(
                error_codes::METHOD_NOT_FOUND,
                format!("{} is not yet implemented (Phase 6)", method),
            )),
            _ => Err(RpcError::new(
                error_codes::METHOD_NOT_FOUND,
                format!("unknown host method: {}", method),
            )),
        }
    }

    async fn handle_notification(&self, method: String, _params: Option<Value>) {
        tracing::debug!(
            plugin = %self.plugin_name,
            method = %method,
            "host ignoring plugin notification"
        );
    }
}

impl AlvaHostHandler {
    fn do_log(&self, params: HostLogParams) {
        let plugin = self.plugin_name.as_str();
        let msg = params.message.as_str();
        let fields = params.fields;
        match params.level {
            LogLevel::Trace => tracing::trace!(
                target: "aep.plugin.host_log",
                plugin = plugin,
                fields = ?fields,
                "{msg}"
            ),
            LogLevel::Debug => tracing::debug!(
                target: "aep.plugin.host_log",
                plugin = plugin,
                fields = ?fields,
                "{msg}"
            ),
            LogLevel::Info => tracing::info!(
                target: "aep.plugin.host_log",
                plugin = plugin,
                fields = ?fields,
                "{msg}"
            ),
            LogLevel::Warn => tracing::warn!(
                target: "aep.plugin.host_log",
                plugin = plugin,
                fields = ?fields,
                "{msg}"
            ),
            LogLevel::Error => tracing::error!(
                target: "aep.plugin.host_log",
                plugin = plugin,
                fields = ?fields,
                "{msg}"
            ),
        }
    }

    fn do_notify(&self, params: HostNotifyParams) {
        let plugin = self.plugin_name.as_str();
        let msg = params.message.as_str();
        match params.level {
            LogLevel::Error => tracing::error!(
                target: "aep.plugin.notify",
                plugin = plugin,
                "NOTIFY: {msg}"
            ),
            LogLevel::Warn => tracing::warn!(
                target: "aep.plugin.notify",
                plugin = plugin,
                "NOTIFY: {msg}"
            ),
            _ => tracing::info!(
                target: "aep.plugin.notify",
                plugin = plugin,
                "NOTIFY: {msg}"
            ),
        }
    }

    fn do_emit_metric(&self, params: HostEmitMetricParams) {
        tracing::info!(
            target: "aep.plugin.metric",
            plugin = %self.plugin_name,
            metric = %params.name,
            value = params.value,
            labels = ?params.labels,
            "plugin metric"
        );
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(
    params: Option<Value>,
) -> Result<T, RpcError> {
    let params = params.unwrap_or(Value::Null);
    serde_json::from_value(params)
        .map_err(|e| RpcError::new(error_codes::INVALID_PARAMS, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn log_round_trip() {
        let h = AlvaHostHandler::new("demo");
        let params = serde_json::json!({
            "level": "info",
            "message": "hello",
            "fields": {"k": 1}
        });
        let result = h.handle_request("host/log".to_string(), Some(params)).await;
        assert!(result.is_ok(), "got: {:?}", result);
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let h = AlvaHostHandler::new("demo");
        let result = h.handle_request("host/nope".to_string(), None).await;
        match result {
            Err(e) if e.code == error_codes::METHOD_NOT_FOUND => {}
            other => panic!("expected METHOD_NOT_FOUND, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn state_methods_return_phase5_hint() {
        let h = AlvaHostHandler::new("demo");
        let params = serde_json::json!({"handle": "x"});
        let result = h
            .handle_request("host/state.get_messages".to_string(), Some(params))
            .await;
        match result {
            Err(e) => {
                assert_eq!(e.code, error_codes::METHOD_NOT_FOUND);
                assert!(e.message.contains("Phase 5"));
            }
            other => panic!("expected Phase 5 hint, got {:?}", other),
        }
    }
}
