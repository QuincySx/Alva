// INPUT:  tiny_http::{Header, Response, Server, StatusCode}, serde_json
// OUTPUT: pub(crate) struct HttpServer, pub(crate) fn json_response, pub(crate) fn error_response, pub(crate) fn read_body, pub(crate) fn parse_query_param
// POS:    Low-level HTTP server wrapper and response helpers for the debug API.
use tiny_http::{Header, Response, Server, StatusCode};

pub(crate) struct HttpServer {
    server: Server,
}

impl HttpServer {
    pub fn new(port: u16) -> Result<Self, std::io::Error> {
        let addr = format!("127.0.0.1:{}", port);
        let server = Server::http(&addr)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
        Ok(Self { server })
    }

    pub fn into_inner(self) -> Server {
        self.server
    }
}

pub(crate) fn json_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let len = data.len();
    let cursor = std::io::Cursor::new(data);
    Response::new(
        StatusCode(status),
        vec![Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()],
        cursor,
        Some(len),
        None,
    )
}

pub(crate) fn error_response(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::json!({"error": message}).to_string();
    json_response(status, &body)
}

pub(crate) fn read_body(request: &mut tiny_http::Request) -> Result<String, std::io::Error> {
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

pub(crate) fn parse_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! Tests for server.rs pure-sync HTTP helpers — 6 tests covering
    //! 3 contract families:
    //!
    //! 1. **`json_response` shape**: Content-Type "application/json"
    //!    (drives browser fetch().then(r => r.json()) + CLI auto-parse),
    //!    status code passthrough, data_length == body.len() (non-
    //!    chunked encoding contract).
    //!
    //! 2. **`error_response` wraps in {"error": msg}**: delegates to
    //!    json_response (so Content-Type chains) AND body shape is
    //!    `{"error": "<msg>"}` (NOT `{"message": ...}` — frontend
    //!    reads body.error).
    //!
    //! 3. **`parse_query_param` URL parsing edge cases**: 7 mundane
    //!    edge cases merged into 1 parametric. The SILENT SECURITY
    //!    pin (no URL decoding — adding decode here could mask
    //!    path-traversal sanitisation downstream) kept standalone
    //!    for PR-review visibility.
    use super::*;

    // -- json_response shape pins ---------------------------------------

    #[test]
    fn json_response_sets_application_json_content_type() {
        // CRITICAL: the literal "application/json" Content-Type drives
        // browser fetch().then(r => r.json()) and the CLI's auto-parse
        // path. A change to "application/x-json" or dropping the header
        // would silently fall back to text rendering.
        let resp = json_response(200, "{}");
        let ct = resp
            .headers()
            .iter()
            .find(|h| {
                h.field
                    .as_str()
                    .as_str()
                    .eq_ignore_ascii_case("content-type")
            })
            .expect("must set Content-Type header");
        assert_eq!(ct.value.as_str(), "application/json");
    }

    #[test]
    fn json_response_status_code_passed_through_verbatim() {
        for code in [200u16, 201, 400, 404, 500] {
            let resp = json_response(code, "{}");
            assert_eq!(resp.status_code(), tiny_http::StatusCode(code));
        }
    }

    #[test]
    fn json_response_data_length_matches_body_byte_count_including_empty() {
        // Pin: data_length = body.len() so tiny_http writes
        // Content-Length (non-chunked encoding contract). Empty body
        // → 0 length. A refactor that dropped len would surface as
        // chunked-encoding churn in integration.rs.
        assert_eq!(json_response(200, "abc12345").data_length(), Some(8));
        assert_eq!(json_response(204, "").data_length(), Some(0));
    }

    // -- error_response wraps in {"error": msg} -------------------------

    #[test]
    fn error_response_inherits_content_type_and_body_wraps_message_in_error_key() {
        // Two contracts in one call: (a) error_response delegates to
        // json_response so Content-Type stays "application/json" (a
        // special-case to text/plain would break frontend JSON.parse);
        // (b) body shape is `{"error": "<msg>"}` (NOT `{"message": ...}`
        // — frontend reads body.error; a rename surfaces `undefined`).
        let resp = error_response(400, "bad request");
        let ct = resp
            .headers()
            .iter()
            .find(|h| {
                h.field
                    .as_str()
                    .as_str()
                    .eq_ignore_ascii_case("content-type")
            })
            .expect("must set Content-Type header via json_response chain");
        assert_eq!(ct.value.as_str(), "application/json");
        let expected_body = serde_json::json!({"error": "bad request"}).to_string();
        assert_eq!(resp.data_length(), Some(expected_body.len()));
    }

    // -- parse_query_param: 7 edge cases merged + 1 standalone -----------

    #[test]
    fn query_param_parses_edge_cases_per_table() {
        // Each row pins one parser edge case. Inline comment names
        // the contract each row enforces.
        let cases: &[(&str, &str, Option<&str>, &str)] = &[
            // ── no `?` returns None (3 sub-cases: real path / empty / bare `/`)
            ("/path/no/query", "key", None, "no ? in url"),
            ("", "key", None, "empty url"),
            ("/", "key", None, "just slash"),
            // ── basic single-key lookup
            ("/path?name=alva", "name", Some("alva"), "single key"),
            // ── multi-param: find middle + tail
            (
                "/path?a=1&name=alva&b=2",
                "name",
                Some("alva"),
                "multi: middle key",
            ),
            ("/path?a=1&name=alva&b=2", "b", Some("2"), "multi: tail key"),
            // ── missing key in non-empty query
            ("/path?a=1&b=2", "missing", None, "missing key"),
            // ── `?key=` → Some("") distinguishes "set but empty" from "absent"
            (
                "/path?flag=",
                "flag",
                Some(""),
                "empty value (boolean-flag style)",
            ),
            // ── `?bareflag` (no `=`) → None because splitn(2, '=') destructure
            //    yields only one Some. A refactor handling bare flags as
            //    Some("") would silently change semantics.
            ("/path?bareflag", "bareflag", None, "key without ="),
            // ── first-match-wins on duplicate keys
            (
                "/path?key=first&key=second",
                "key",
                Some("first"),
                "duplicate keys: first wins",
            ),
        ];
        for (url, key, expected, label) in cases {
            assert_eq!(
                parse_query_param(url, key),
                expected.map(|s| s.to_string()),
                "case {label:?} failed: parse_query_param({url:?}, {key:?})"
            );
        }
    }

    #[test]
    fn query_param_does_not_url_decode_percent_escapes() {
        // SILENT SECURITY CONTRACT: parse_query_param returns RAW
        // (un-decoded) values. A caller that wanted `/etc/passwd`
        // from `%2Fetc%2Fpasswd` would see literal `%2Fetc%2Fpasswd`
        // and have to decode itself. A refactor that added decode
        // here could MASK path-traversal sanitisation in downstream
        // handlers that rely on seeing the raw `%2F` form. Kept
        // standalone (not folded into the parametric table) for
        // PR-review visibility on security-relevant behavior.
        assert_eq!(
            parse_query_param("/path?p=%2Fetc%2Fpasswd", "p"),
            Some("%2Fetc%2Fpasswd".to_string())
        );
        assert_eq!(
            parse_query_param("/path?q=hello%20world", "q"),
            Some("hello%20world".to_string())
        );
    }
}
