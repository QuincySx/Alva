//! SSE (Server-Sent Events) transport for MCP.
//!
//! Connects to an MCP server via HTTP SSE endpoint. This transport
//! is used for MCP servers that expose an HTTP-based interface rather
//! than stdio-based communication.

use std::collections::HashMap;

/// SSE transport configuration and connection state for MCP.
///
/// Connects to an MCP server via an HTTP SSE endpoint. The server
/// sends events via SSE and accepts JSON-RPC requests via HTTP POST.
///
/// Note: This is a transport configuration struct. The actual transport
/// implementation (implementing `McpTransport` trait) requires the `reqwest`
/// dependency which is gated behind the `native` feature in the main crate.
pub struct SseTransport {
    /// SSE endpoint URL (e.g., "http://127.0.0.1:3000/sse")
    url: String,
    /// Optional POST endpoint URL (discovered from SSE messages)
    post_url: Option<String>,
    /// Extra HTTP headers (e.g., Authorization)
    headers: HashMap<String, String>,
}

impl SseTransport {
    /// Create a new SSE transport targeting the given URL.
    pub fn new(url: String) -> Self {
        Self {
            url,
            post_url: None,
            headers: HashMap::new(),
        }
    }

    /// Add a custom HTTP header (builder pattern).
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Add multiple headers at once.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Get the SSE endpoint URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Get the POST endpoint URL (discovered after SSE connection).
    pub fn post_url(&self) -> Option<&str> {
        self.post_url.as_deref()
    }

    /// Set the POST endpoint URL (typically discovered from SSE messages).
    pub fn set_post_url(&mut self, url: String) {
        self.post_url = Some(url);
    }

    /// Get configured headers.
    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }
}

/// WebSocket transport configuration for MCP.
///
/// Connects to an MCP server via WebSocket. This is an alternative
/// to SSE for bidirectional communication.
pub struct WebSocketTransport {
    /// WebSocket endpoint URL (e.g., "ws://127.0.0.1:3000/ws")
    url: String,
    /// Extra HTTP headers for the WebSocket upgrade request.
    headers: HashMap<String, String>,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport targeting the given URL.
    pub fn new(url: String) -> Self {
        Self {
            url,
            headers: HashMap::new(),
        }
    }

    /// Add a custom HTTP header for the WebSocket upgrade request (builder pattern).
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Add multiple headers at once.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Get the WebSocket endpoint URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Get configured headers.
    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SseTransport ───────────────────────────────────────────────────

    #[test]
    fn sse_transport_new() {
        let transport = SseTransport::new("http://localhost:3000/sse".into());
        assert_eq!(transport.url(), "http://localhost:3000/sse");
        assert!(transport.post_url().is_none());
        assert!(transport.headers().is_empty());
    }

    #[test]
    fn sse_transport_with_header() {
        let transport = SseTransport::new("http://localhost/sse".into())
            .with_header("Authorization".into(), "Bearer token".into());

        assert_eq!(transport.headers()["Authorization"], "Bearer token");
    }

    #[test]
    fn sse_transport_with_multiple_headers() {
        let headers = HashMap::from([
            ("Authorization".into(), "Bearer tok".into()),
            ("X-Custom".into(), "value".into()),
        ]);
        let transport = SseTransport::new("http://localhost/sse".into()).with_headers(headers);

        assert_eq!(transport.headers().len(), 2);
    }

    #[test]
    fn sse_transport_set_post_url() {
        let mut transport = SseTransport::new("http://localhost/sse".into());
        assert!(transport.post_url().is_none());

        transport.set_post_url("http://localhost/messages".into());
        assert_eq!(transport.post_url(), Some("http://localhost/messages"));
    }

    // ── WebSocketTransport ─────────────────────────────────────────────

    #[test]
    fn ws_transport_new() {
        let transport = WebSocketTransport::new("ws://localhost:3000/ws".into());
        assert_eq!(transport.url(), "ws://localhost:3000/ws");
        assert!(transport.headers().is_empty());
    }

    #[test]
    fn ws_transport_with_header() {
        let transport = WebSocketTransport::new("ws://localhost/ws".into())
            .with_header("Authorization".into(), "Bearer token".into());

        assert_eq!(transport.headers()["Authorization"], "Bearer token");
    }

    #[test]
    fn ws_transport_with_multiple_headers() {
        let headers = HashMap::from([
            ("Authorization".into(), "Bearer tok".into()),
            ("X-Api-Key".into(), "key123".into()),
        ]);
        let transport = WebSocketTransport::new("ws://localhost/ws".into()).with_headers(headers);

        assert_eq!(transport.headers().len(), 2);
        assert_eq!(transport.headers()["X-Api-Key"], "key123");
    }
}
