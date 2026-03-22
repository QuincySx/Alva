use std::sync::Arc;
use std::time::Instant;

use crate::inspect::Inspectable;
use crate::log_layer::LogHandle;
use crate::log_store::LogQuery;
use crate::server::{error_response, json_response, parse_query_param, read_body};

pub(crate) struct Router {
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
    start_time: Instant,
}

impl Router {
    pub fn new(log_handle: Option<LogHandle>, inspector: Option<Arc<dyn Inspectable>>) -> Self {
        Self {
            log_handle,
            inspector,
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
}
