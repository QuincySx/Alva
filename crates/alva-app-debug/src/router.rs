// INPUT:  std::sync, std::time, tiny_http, serde_json, crate::{ActionRegistry, Inspectable, LogHandle, LogQuery, server}
// OUTPUT: pub(crate) struct Router
// POS:    HTTP request router that dispatches debug API endpoints to log, inspect, action, and lifecycle handlers.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::action_registry::ActionRegistry;
use crate::inspect::Inspectable;
use crate::log_layer::LogHandle;
use crate::log_store::LogQuery;
use crate::server::{error_response, json_response, parse_query_param, read_body};

pub(crate) struct Router {
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
    action_registry: Option<Arc<ActionRegistry>>,
    shutdown_flag: Arc<AtomicBool>,
    start_time: Instant,
}

impl Router {
    pub fn new(
        log_handle: Option<LogHandle>,
        inspector: Option<Arc<dyn Inspectable>>,
        action_registry: Option<Arc<ActionRegistry>>,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Self {
        Self {
            log_handle,
            inspector,
            action_registry,
            shutdown_flag,
            start_time: Instant::now(),
        }
    }

    pub fn handle(&self, mut request: tiny_http::Request) {
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or(&url);
        let method = request.method().as_str();

        let response = match (method, path) {
            ("GET", "/api/health") => self.handle_health(),
            ("GET", "/api/logs") => self.handle_get_logs(&url),
            ("GET", "/api/logs/level") => self.handle_get_log_level(),
            ("PUT", "/api/logs/level") => self.handle_set_log_level(&mut request),
            ("GET", "/api/inspect/tree") => self.handle_inspect_tree(),
            ("POST", "/api/action") => self.handle_action(&mut request),
            ("GET", "/api/inspect/state") => self.handle_inspect_state(&url),
            ("GET", "/api/inspect/views") => self.handle_inspect_views(),
            ("POST", "/api/screenshot") => self.handle_screenshot(),
            ("POST", "/api/shutdown") => self.handle_shutdown(),
            _ => error_response(404, &format!("unknown endpoint: {} {}", method, path)),
        };
        let _ = request.respond(response);
    }

    fn handle_health(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let uptime = self.start_time.elapsed().as_secs();
        let body = serde_json::json!({"status": "ok", "uptime_secs": uptime}).to_string();
        json_response(200, &body)
    }

    fn handle_get_logs(&self, url: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref handle) = self.log_handle else {
            return error_response(503, "log system not registered");
        };
        let query = LogQuery {
            level: parse_query_param(url, "level"),
            module: parse_query_param(url, "module"),
            since: parse_query_param(url, "since").and_then(|s| s.parse().ok()),
            cursor: parse_query_param(url, "cursor").and_then(|s| s.parse().ok()),
            keyword: parse_query_param(url, "keyword"),
            limit: parse_query_param(url, "limit").and_then(|s| s.parse().ok()),
        };
        let result = handle.query(&query);
        let body = serde_json::to_string(&result).unwrap();
        json_response(200, &body)
    }

    fn handle_get_log_level(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref handle) = self.log_handle else {
            return error_response(503, "log system not registered");
        };
        let filter = handle.current_filter();
        let body = serde_json::json!({"filter": filter}).to_string();
        json_response(200, &body)
    }

    fn handle_set_log_level(
        &self,
        request: &mut tiny_http::Request,
    ) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref handle) = self.log_handle else {
            return error_response(503, "log system not registered");
        };
        let body = match read_body(request) {
            Ok(b) => b,
            Err(_) => return error_response(400, "failed to read request body"),
        };
        let parsed: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return error_response(400, "malformed JSON body"),
        };
        let Some(filter) = parsed.get("filter").and_then(|v| v.as_str()) else {
            return error_response(400, "missing 'filter' field in JSON body");
        };
        handle.set_filter(filter);
        let body = serde_json::json!({"ok": true, "filter": filter}).to_string();
        json_response(200, &body)
    }

    fn handle_inspect_tree(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref inspector) = self.inspector else {
            return error_response(503, "inspector not registered");
        };
        let tree = inspector.inspect();
        let body = serde_json::to_string(&tree).unwrap();
        json_response(200, &body)
    }

    fn handle_action(
        &self,
        request: &mut tiny_http::Request,
    ) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref registry) = self.action_registry else {
            return error_response(503, "action registry not registered");
        };
        let body = match read_body(request) {
            Ok(b) => b,
            Err(_) => return error_response(400, "failed to read request body"),
        };
        let parsed: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return error_response(400, "malformed JSON body"),
        };
        let Some(target) = parsed.get("target").and_then(|v| v.as_str()) else {
            return error_response(400, "missing 'target' field in JSON body");
        };
        let Some(method) = parsed.get("method").and_then(|v| v.as_str()) else {
            return error_response(400, "missing 'method' field in JSON body");
        };
        let args = parsed.get("args").cloned().unwrap_or(serde_json::Value::Object(Default::default()));

        match registry.dispatch(target, method, args) {
            Ok(result) => {
                let body = serde_json::json!({"ok": true, "result": result}).to_string();
                json_response(200, &body)
            }
            Err(e) => {
                let error_type = if e.contains("not registered") {
                    "target_not_found"
                } else if e.contains("not found") {
                    "method_not_found"
                } else {
                    "execution_failed"
                };
                let body = serde_json::json!({"ok": false, "error": error_type, "message": e}).to_string();
                json_response(400, &body)
            }
        }
    }

    fn handle_inspect_state(&self, url: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref registry) = self.action_registry else {
            return error_response(503, "action registry not registered");
        };
        let Some(view) = parse_query_param(url, "view") else {
            return error_response(400, "missing 'view' query parameter");
        };

        match registry.get_state(&view) {
            Ok(state) => {
                let body = serde_json::json!({"view": view, "state": state}).to_string();
                json_response(200, &body)
            }
            Err(e) => {
                let body = serde_json::json!({"ok": false, "error": "state_error", "message": e}).to_string();
                json_response(400, &body)
            }
        }
    }

    fn handle_inspect_views(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref registry) = self.action_registry else {
            return error_response(503, "action registry not registered");
        };
        let views: Vec<serde_json::Value> = registry
            .list_views()
            .into_iter()
            .map(|(id, methods)| serde_json::json!({"id": id, "methods": methods}))
            .collect();
        let body = serde_json::json!({"views": views}).to_string();
        json_response(200, &body)
    }

    fn handle_screenshot(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let path = format!("/tmp/alva-debug-screenshot-{}.png", timestamp);

        let status = std::process::Command::new("screencapture")
            .args(["-x", &path])
            .status();

        match status {
            Ok(s) if s.success() => {
                let body = serde_json::json!({"ok": true, "path": path}).to_string();
                json_response(200, &body)
            }
            _ => {
                let body = serde_json::json!({"ok": false, "error": "screenshot_failed"}).to_string();
                json_response(500, &body)
            }
        }
    }

    fn handle_shutdown(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        let body = serde_json::json!({"ok": true}).to_string();
        json_response(200, &body)
    }
}
