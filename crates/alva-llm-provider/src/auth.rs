//! Authentication resolution — converts user-facing auth inputs into unified headers.
//!
//! The external API accepts two mutually exclusive auth modes:
//! - **API Key**: convenience for the user; we convert it to the standard header.
//! - **Custom Headers**: full control; sent as-is.
//!
//! After resolution, the rest of the system only deals with `HashMap<String, String>` headers.

use std::collections::HashMap;
use reqwest::RequestBuilder;

/// Industry-standard auth schemes for converting an API key to a header.
#[derive(Debug, Clone, Copy)]
pub enum AuthScheme {
    /// `Authorization: Bearer <key>` — OpenAI, most OpenAI-compatible services.
    Bearer,
    /// `x-api-key: <key>` — Anthropic.
    XApiKey,
}

impl AuthScheme {
    /// Convert an API key to the standard header pair for this scheme.
    fn to_header(&self, api_key: &str) -> (&'static str, String) {
        match self {
            AuthScheme::Bearer => ("Authorization", format!("Bearer {}", api_key)),
            AuthScheme::XApiKey => ("x-api-key", api_key.to_string()),
        }
    }
}

/// Resolve user-facing auth inputs into a unified set of headers.
///
/// Rules:
/// 1. `custom_headers` non-empty → use them, ignore `api_key`.
/// 2. `api_key` non-empty → convert via `scheme` to the standard header.
/// 3. Both empty → no auth headers (e.g. Ollama local).
pub fn resolve_auth_headers(
    api_key: &str,
    custom_headers: &HashMap<String, String>,
    scheme: AuthScheme,
) -> HashMap<String, String> {
    if !custom_headers.is_empty() {
        return custom_headers.clone();
    }
    let mut headers = HashMap::new();
    if !api_key.is_empty() {
        let (name, value) = scheme.to_header(api_key);
        headers.insert(name.to_string(), value);
    }
    headers
}

/// Apply resolved headers to a request builder.
///
/// This is the only place HTTP headers are set — providers call this with
/// the pre-resolved headers from `resolve_auth_headers`.
pub(crate) fn apply_headers(mut req: RequestBuilder, headers: &HashMap<String, String>) -> RequestBuilder {
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    req
}
