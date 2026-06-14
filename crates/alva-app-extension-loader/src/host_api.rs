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
//!   through to the plugin bridge, which is an invasive change to the
//!   middleware-hook plumbing. Methods return `METHOD_NOT_FOUND` until
//!   Phase 5 wires this up.
//! - `host/memory.*` — same reason, plus needs access to the current
//!   memory backend through the bus. Phase 6.
//! - `host/request_approval` — needs the approval channel from
//!   `ApprovalPlugin`, which we do not have a reference to here.
//!   Phase 6 once we wire the approval bridge.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dispatcher::HostHandler;
use crate::protocol::{error_codes, HostLogParams, LogLevel, RpcError};

/// The production host handler wired in by `SubprocessLoaderPlugin`.
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

    // -- Gap-fill (Loop 144): parse_params + host/notify + host/emit_metric +
    //    Phase 5 siblings + Phase 6 trio + handle_notification + ctor -----

    #[tokio::test]
    async fn malformed_params_return_invalid_params_code_not_method_not_found() {
        // CRITICAL: parse_params failure MUST be INVALID_PARAMS (-32602,
        // JSON-RPC 2.0 spec) NOT METHOD_NOT_FOUND (-32601). A typo'd
        // params shape that surfaced as METHOD_NOT_FOUND would mislead
        // plugin authors into thinking the method doesn't exist and
        // retrying with the same broken payload.
        let h = AlvaHostHandler::new("demo");
        let bad = serde_json::json!({"this": "is not HostLogParams"});
        let result = h
            .handle_request("host/log".to_string(), Some(bad))
            .await;
        match result {
            Err(e) => {
                assert_eq!(
                    e.code,
                    error_codes::INVALID_PARAMS,
                    "must be INVALID_PARAMS for malformed params, not METHOD_NOT_FOUND"
                );
                assert_eq!(e.code, -32602, "JSON-RPC 2.0 spec literal pin");
            }
            other => panic!("expected INVALID_PARAMS, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn host_notify_happy_path_returns_empty_object() {
        let h = AlvaHostHandler::new("demo");
        let params = serde_json::json!({"level": "warn", "message": "ping"});
        let result = h
            .handle_request("host/notify".to_string(), Some(params))
            .await;
        assert!(result.is_ok(), "got: {:?}", result);
        // Contract: success result is an empty JSON object (NOT null,
        // NOT a string). Plugins parse `result` per JSON-RPC spec.
        let v = result.unwrap();
        assert!(
            v.as_object().map(|o| o.is_empty()).unwrap_or(false),
            "host/notify success result must be {{}}: got {v}"
        );
    }

    #[tokio::test]
    async fn host_emit_metric_happy_path_returns_empty_object() {
        let h = AlvaHostHandler::new("demo");
        let params = serde_json::json!({
            "name": "requests_total",
            "value": 42.0,
            "labels": {"endpoint": "/v1/chat"}
        });
        let result = h
            .handle_request("host/emit_metric".to_string(), Some(params))
            .await;
        assert!(result.is_ok(), "got: {:?}", result);
        let v = result.unwrap();
        assert!(
            v.as_object().map(|o| o.is_empty()).unwrap_or(false),
            "host/emit_metric success result must be {{}}: got {v}"
        );
    }

    #[tokio::test]
    async fn host_emit_metric_labels_field_is_optional() {
        // Pin: labels is Option<Value> with skip_serializing_if; sending
        // without labels MUST succeed (not require the field).
        let h = AlvaHostHandler::new("demo");
        let params = serde_json::json!({"name": "x", "value": 1.0});
        let result = h
            .handle_request("host/emit_metric".to_string(), Some(params))
            .await;
        assert!(
            result.is_ok(),
            "host/emit_metric without labels must succeed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn state_get_metadata_returns_phase5_hint() {
        // Sibling of state_methods_return_phase5_hint (which only
        // covers get_messages). Pin each Phase 5 endpoint separately
        // — a refactor that handled one but not the others would
        // silently change the discriminator.
        let h = AlvaHostHandler::new("demo");
        let result = h
            .handle_request("host/state.get_metadata".to_string(), None)
            .await;
        match result {
            Err(e) => {
                assert_eq!(e.code, error_codes::METHOD_NOT_FOUND);
                assert!(e.message.contains("Phase 5"), "missing Phase 5 hint: {}", e.message);
                assert!(e.message.contains("host/state.get_metadata"), "must name method: {}", e.message);
            }
            other => panic!("expected Phase 5 hint, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn state_count_tokens_returns_phase5_hint() {
        let h = AlvaHostHandler::new("demo");
        let result = h
            .handle_request("host/state.count_tokens".to_string(), None)
            .await;
        match result {
            Err(e) => {
                assert_eq!(e.code, error_codes::METHOD_NOT_FOUND);
                assert!(e.message.contains("Phase 5"), "missing Phase 5 hint: {}", e.message);
            }
            other => panic!("expected Phase 5 hint, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn host_memory_read_returns_phase6_hint() {
        // Phase 6 trio (memory.read/write + request_approval) is
        // entirely 0-test. Pin each independently — the source code
        // uses 2 separate match arms for memory.* + request_approval,
        // a refactor that consolidated them might drop one.
        let h = AlvaHostHandler::new("demo");
        let result = h
            .handle_request("host/memory.read".to_string(), None)
            .await;
        match result {
            Err(e) => {
                assert_eq!(e.code, error_codes::METHOD_NOT_FOUND);
                assert!(e.message.contains("Phase 6"), "missing Phase 6 hint: {}", e.message);
            }
            other => panic!("expected Phase 6 hint, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn host_memory_write_returns_phase6_hint() {
        let h = AlvaHostHandler::new("demo");
        let result = h
            .handle_request("host/memory.write".to_string(), None)
            .await;
        match result {
            Err(e) => {
                assert_eq!(e.code, error_codes::METHOD_NOT_FOUND);
                assert!(e.message.contains("Phase 6"));
            }
            other => panic!("expected Phase 6 hint, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn host_request_approval_returns_phase6_hint() {
        let h = AlvaHostHandler::new("demo");
        let result = h
            .handle_request("host/request_approval".to_string(), None)
            .await;
        match result {
            Err(e) => {
                assert_eq!(e.code, error_codes::METHOD_NOT_FOUND);
                assert!(e.message.contains("Phase 6"));
            }
            other => panic!("expected Phase 6 hint, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn handle_notification_silently_drops_without_panic() {
        // Pin: notifications return () regardless of method or
        // params. AlvaHostHandler is installed on hot paths and must
        // never panic on bad notifications (no auth, no validation,
        // just log + drop).
        let h = AlvaHostHandler::new("demo");
        h.handle_notification("any/method".into(), None).await;
        h.handle_notification("garbage".into(), Some(serde_json::json!(null))).await;
        h.handle_notification(
            "host/log".into(),
            Some(serde_json::json!({"this": "is malformed"})),
        )
        .await;
        // Reaching here = silent-drop contract holds.
    }

    #[test]
    fn ctor_stores_plugin_name_default_is_empty() {
        // Pin: AlvaHostHandler::new(name) stores `name`; Default
        // produces empty plugin_name (suitable for cases where the
        // handler hasn't been bound to a plugin yet). A refactor
        // that swapped one for the other would silently change log
        // tagging.
        let h = AlvaHostHandler::new("my-plugin");
        assert_eq!(h.plugin_name, "my-plugin");

        let d = AlvaHostHandler::default();
        assert_eq!(d.plugin_name, "", "Default must yield empty plugin_name");
    }
}
