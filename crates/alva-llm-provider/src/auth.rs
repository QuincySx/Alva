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
    /// `x-goog-api-key: <key>` — Google Gemini / Vertex AI.
    GoogApiKey,
}

impl AuthScheme {
    /// Convert an API key to the standard header pair for this scheme.
    fn to_header(&self, api_key: &str) -> (&'static str, String) {
        match self {
            AuthScheme::Bearer => ("Authorization", format!("Bearer {}", api_key)),
            AuthScheme::XApiKey => ("x-api-key", api_key.to_string()),
            AuthScheme::GoogApiKey => ("x-goog-api-key", api_key.to_string()),
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

#[cfg(test)]
mod tests {
    //! Tests for AuthScheme + resolve_auth_headers.
    //!
    //! Wire-format pins: dropping "Bearer " prefix or swapping the
    //! header name between providers (x-api-key vs x-goog-api-key)
    //! silently breaks every authenticated request — provider would
    //! see no/wrong auth and return 401.
    //!
    //! Priority pin: custom_headers wins over api_key. A refactor
    //! that merged them would silently send both, potentially with
    //! conflicting auth.
    use super::*;

    // -- AuthScheme::to_header per scheme ---------------------------------

    #[test]
    fn bearer_scheme_uses_authorization_header_with_bearer_prefix() {
        // OpenAI / OpenAI-compatible expects "Authorization: Bearer <key>".
        // Dropping the "Bearer " prefix would silently break every
        // OpenAI request with a 401.
        let (name, value) = AuthScheme::Bearer.to_header("sk-test-123");
        assert_eq!(name, "Authorization");
        assert_eq!(value, "Bearer sk-test-123");
    }

    #[test]
    fn xapikey_scheme_uses_lowercase_header_name_no_prefix() {
        // Anthropic uses "x-api-key: <key>" — NO prefix, lowercase
        // header. Capitalizing or adding "Bearer " breaks Anthropic
        // requests silently.
        let (name, value) = AuthScheme::XApiKey.to_header("sk-ant-test");
        assert_eq!(name, "x-api-key");
        assert_eq!(value, "sk-ant-test");
    }

    #[test]
    fn googapikey_scheme_uses_goog_namespaced_header() {
        // Gemini / Vertex AI uses "x-goog-api-key: <key>". Pinned
        // separately from XApiKey so a refactor that "simplified" both
        // to the same header would fail this test loudly.
        let (name, value) = AuthScheme::GoogApiKey.to_header("AIza-test");
        assert_eq!(name, "x-goog-api-key");
        assert_eq!(value, "AIza-test");
    }

    // -- resolve_auth_headers: three paths --------------------------------

    #[test]
    fn empty_inputs_yield_no_auth_headers_for_ollama_local() {
        // Pinned: empty api_key AND empty custom_headers → empty
        // returned map. Use case: local Ollama / proxies that
        // require no auth. A refactor that defaulted to sending
        // something would break unauthenticated local flows.
        let result = resolve_auth_headers("", &HashMap::new(), AuthScheme::Bearer);
        assert!(result.is_empty(), "empty inputs must yield no auth headers: {result:?}");
    }

    #[test]
    fn api_key_only_produces_single_header_per_scheme() {
        let result = resolve_auth_headers("sk-test", &HashMap::new(), AuthScheme::Bearer);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("Authorization").map(|s| s.as_str()), Some("Bearer sk-test"));
    }

    #[test]
    fn custom_headers_non_empty_takes_priority_over_api_key() {
        // Priority pin: custom_headers MUST win when both are
        // non-empty — the resolved set is `custom_headers.clone()`
        // verbatim, api_key is ignored. Without this, a refactor
        // that merged them would send conflicting auth.
        let mut custom = HashMap::new();
        custom.insert("X-Custom-Auth".to_string(), "magic-token".to_string());
        let result = resolve_auth_headers("sk-ignored-because-custom-wins", &custom, AuthScheme::Bearer);
        assert_eq!(result.len(), 1);
        // The custom header was used verbatim.
        assert_eq!(
            result.get("X-Custom-Auth").map(|s| s.as_str()),
            Some("magic-token")
        );
        // The api_key did NOT leak into the output as an Authorization
        // header — the OpenAI-style auth must be absent.
        assert!(
            !result.contains_key("Authorization"),
            "api_key must NOT produce an Authorization header when custom_headers is non-empty: {result:?}"
        );
    }

    #[test]
    fn custom_headers_with_multiple_entries_all_pass_through() {
        // Pin: full custom map is passed through (not just one entry).
        let mut custom = HashMap::new();
        custom.insert("Authorization".to_string(), "Bearer custom-1".to_string());
        custom.insert("X-Org-Id".to_string(), "org-42".to_string());
        custom.insert("X-Trace-Id".to_string(), "trace-xyz".to_string());
        let result = resolve_auth_headers("", &custom, AuthScheme::Bearer);
        assert_eq!(result.len(), 3);
        assert_eq!(result.get("Authorization").map(|s| s.as_str()), Some("Bearer custom-1"));
        assert_eq!(result.get("X-Org-Id").map(|s| s.as_str()), Some("org-42"));
        assert_eq!(result.get("X-Trace-Id").map(|s| s.as_str()), Some("trace-xyz"));
    }

    #[test]
    fn api_key_with_xapikey_scheme_uses_correct_header_name() {
        // Cross-check: when scheme=XApiKey, the resolved header is
        // x-api-key (NOT Authorization). Pin so a refactor that
        // hardcoded Authorization for all schemes would fail here.
        let result = resolve_auth_headers("sk-ant", &HashMap::new(), AuthScheme::XApiKey);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("x-api-key").map(|s| s.as_str()), Some("sk-ant"));
        assert!(!result.contains_key("Authorization"));
    }
}
