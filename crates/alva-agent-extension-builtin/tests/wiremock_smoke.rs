// Smoke test for the wiremock dev-dep harness.
//
// PURPOSE: Prove that wiremock + reqwest are wired correctly inside the
// `alva-agent-extension-builtin` crate's test profile, so that future
// HTTP-mock tests for `internet_search` and `read_url` can rely on this
// foundation without each one rediscovering setup issues (port binding,
// async runtime flavor, feature gating, etc.).
//
// SCOPE: Intentionally minimal. The only invariants this file pins are:
//   - wiremock::MockServer::start() works on the test runtime
//   - Mock(method+path) → respond_with(...) reaches a reqwest client
//   - The mock body comes back verbatim
//
// IF THIS BREAKS: don't write content tests until this is green again —
// it would mean wiremock / reqwest / tokio versions or features have
// drifted out of compatibility, and any tool-level test would fail for
// the same reason but with noisier output.

// Guarded on the `web` feature because reqwest is itself web-feature-only
// in this crate. The whole point of wiremock here is to test web tools.
#![cfg(feature = "web")]

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn wiremock_serves_a_mock_response_to_reqwest() {
    // Start an ephemeral mock server on a random free port
    let server = MockServer::start().await;

    // Register: GET /smoke → 200 "pong"
    Mock::given(method("GET"))
        .and(path("/smoke"))
        .respond_with(ResponseTemplate::new(200).set_body_string("pong"))
        .mount(&server)
        .await;

    // Fire a real reqwest request at the mock's URI
    let url = format!("{}/smoke", server.uri());
    let body = reqwest::get(&url)
        .await
        .expect("reqwest GET should succeed against the mock server")
        .text()
        .await
        .expect("response body should decode as text");

    assert_eq!(
        body, "pong",
        "wiremock harness wiring is broken — mocked body did not reach reqwest"
    );
}
