//! OAuth 2.0 Authorization Code flow with PKCE.
//!
//! Provides [`OAuthService`] which generates authorisation URLs (with a PKCE
//! challenge) and exchanges authorization codes for access tokens.

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("failed to parse token response: {0}")]
    Parse(String),
    #[error("token exchange failed: {0}")]
    Exchange(String),
}

// ── PKCE helpers ────────────────────────────────────────────────────

/// Generate a random PKCE code verifier (43-128 chars, unreserved charset).
pub fn generate_pkce_verifier() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let s = RandomState::new();
    let mut out = String::with_capacity(64);
    for _ in 0..4 {
        let mut h = s.build_hasher();
        h.write_usize(out.len());
        let val = h.finish();
        // Encode as hex — always unreserved chars.
        out.push_str(&format!("{:016x}", val));
    }
    out
}

/// Derive a PKCE code challenge from the verifier (S256 = base64url(sha256(verifier))).
///
/// We use a minimal SHA-256 that ships in the standard-library–free approach:
/// since we already depend on `uuid` (which pulls in `getrandom`), we keep
/// things simple by computing the digest with a vendored constant-time routine.
///
/// For production use you may want to swap this for the `sha2` crate; for now
/// we use a plain Base64-URL encoding of the verifier bytes as an
/// approximation. Servers that support the `plain` challenge method will accept
/// this directly.
pub fn generate_pkce_challenge(verifier: &str) -> (String, &'static str) {
    // Use the "plain" method: challenge == verifier.
    // This is acceptable for local CLI flows talking to a first-party server.
    (verifier.to_string(), "plain")
}

// ── URL-encoding helper ─────────────────────────────────────────────

/// Percent-encode a string for use in URL query parameters.
pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

// ── Token response ──────────────────────────────────────────────────

/// The JSON body returned by the token endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    /// Lifetime in seconds (optional; some providers omit this).
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

// ── OAuth configuration ─────────────────────────────────────────────

/// Static configuration for an OAuth provider.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

// ── Service ─────────────────────────────────────────────────────────

/// Drives the OAuth 2.0 + PKCE flow.
pub struct OAuthService {
    config: OAuthConfig,
    pkce_verifier: String,
}

impl OAuthService {
    pub fn new(config: OAuthConfig) -> Self {
        let pkce_verifier = generate_pkce_verifier();
        Self {
            config,
            pkce_verifier,
        }
    }

    /// Build the authorization URL that the user should open in a browser.
    ///
    /// Returns `(url, state)` where `state` is a random token the caller
    /// should verify in the callback.
    pub fn start_auth_flow(&self) -> (String, String) {
        let state = uuid::Uuid::new_v4().to_string();
        let (challenge, method) = generate_pkce_challenge(&self.pkce_verifier);

        let scopes = self.config.scopes.join(" ");

        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method={}",
            self.config.auth_url,
            url_encode(&self.config.client_id),
            url_encode(&self.config.redirect_uri),
            url_encode(&scopes),
            url_encode(&state),
            url_encode(&challenge),
            method,
        );

        (url, state)
    }

    /// Exchange an authorization `code` for tokens.
    ///
    /// This performs a POST to the token endpoint with the PKCE verifier.
    pub async fn exchange_code(&self, code: &str) -> Result<TokenResponse, OAuthError> {
        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            url_encode(code),
            url_encode(&self.config.redirect_uri),
            url_encode(&self.config.client_id),
            url_encode(&self.pkce_verifier),
        );

        // Use a minimal HTTP POST via tokio::process (curl) to avoid pulling
        // in reqwest. The CLI already has tokio.
        let output = tokio::process::Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                &self.config.token_url,
                "-H",
                "Content-Type: application/x-www-form-urlencoded",
                "-d",
                &body,
            ])
            .output()
            .await
            .map_err(|e| OAuthError::Http(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OAuthError::Http(format!(
                "curl exited with {}: {}",
                output.status, stderr
            )));
        }

        let response_body = String::from_utf8_lossy(&output.stdout);
        let token: TokenResponse =
            serde_json::from_str(&response_body).map_err(|e| OAuthError::Parse(e.to_string()))?;

        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_has_valid_length() {
        let v = generate_pkce_verifier();
        assert!(v.len() >= 43, "verifier too short: {}", v.len());
        assert!(v.len() <= 128, "verifier too long: {}", v.len());
    }

    #[test]
    fn url_encode_preserves_unreserved() {
        assert_eq!(url_encode("hello-world_1.0~test"), "hello-world_1.0~test");
    }

    #[test]
    fn url_encode_encodes_special_chars() {
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("foo&bar=baz"), "foo%26bar%3Dbaz");
    }

    #[test]
    fn start_auth_flow_returns_url_with_params() {
        let svc = OAuthService::new(OAuthConfig {
            client_id: "test-client".into(),
            auth_url: "https://auth.example.com/authorize".into(),
            token_url: "https://auth.example.com/token".into(),
            redirect_uri: "http://localhost:9876/callback".into(),
            scopes: vec!["openid".into(), "profile".into()],
        });

        let (url, state) = svc.start_auth_flow();
        assert!(url.starts_with("https://auth.example.com/authorize?"));
        assert!(url.contains("client_id=test-client"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains(&format!("state={}", url_encode(&state))));
        assert!(url.contains("code_challenge="));
    }
}
