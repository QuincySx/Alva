// INPUT:  alva_kernel_abi, async_trait, reqwest, schemars, serde, serde_json
// OUTPUT: InternetSearchTool
// POS:    Searches the internet using DuckDuckGo Instant Answer API with domain filtering
//         and progress tracking.
//! internet_search — search the internet using DuckDuckGo Instant Answer API

use alva_kernel_abi::{AgentError, ProgressEvent, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The search query.
    query: String,
    /// Maximum number of results to return (default: 5).
    #[serde(default)]
    max_results: Option<usize>,
    /// Only return results from these domains (e.g., ['docs.rs', 'github.com']).
    #[serde(default)]
    allowed_domains: Option<Vec<String>>,
    /// Exclude results from these domains.
    #[serde(default)]
    blocked_domains: Option<Vec<String>>,
}

/// DuckDuckGo API response (partial).
///
/// **Field name quirk**: DDG returns `AbstractURL` and `FirstURL` with
/// the acronym in ALL CAPS. serde's `rename_all = "PascalCase"` would
/// only match `AbstractUrl` / `FirstUrl`, so those two fields need
/// explicit `#[serde(rename = ...)]` overrides — otherwise the URLs
/// silently deserialize as empty strings and every search result loses
/// its link. See test `ddg_response_pascal_case_deserialization`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DdgResponse {
    #[serde(default)]
    abstract_text: String,
    #[serde(default)]
    abstract_source: String,
    #[serde(default, rename = "AbstractURL")]
    abstract_url: String,
    #[serde(default)]
    heading: String,
    #[serde(default)]
    related_topics: Vec<DdgRelatedTopic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DdgRelatedTopic {
    #[serde(default)]
    text: String,
    #[serde(default, rename = "FirstURL")]
    first_url: String,
}

/// Extract domain from a URL for filtering purposes.
fn extract_domain_from_url(url: &str) -> Option<String> {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = url.split('/').next()?;
    let domain = host.split(':').next()?;
    Some(domain.to_lowercase())
}

/// Check if a URL's domain matches any of the given domains.
fn domain_matches(url: &str, domains: &[String]) -> bool {
    if let Some(url_domain) = extract_domain_from_url(url) {
        domains.iter().any(|d| {
            let d_lower = d.to_lowercase();
            url_domain == d_lower || url_domain.ends_with(&format!(".{}", d_lower))
        })
    } else {
        false
    }
}

#[derive(Tool)]
#[tool(
    name = "internet_search",
    description = "Search the internet for information. Returns search results with titles, snippets, and URLs. \
        Supports domain filtering (allowed/blocked) for focused searches.",
    input = Input,
    read_only,
)]
pub struct InternetSearchTool;

impl InternetSearchTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let max_results = params.max_results.unwrap_or(5);

        // Report search start progress
        ctx.report_progress(ProgressEvent::Status {
            message: format!("Searching: {}", params.query),
        });

        // Use DuckDuckGo Instant Answer API (JSON, no auth required)
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencoding(&params.query)
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("SrowAgent/0.1")
            .build()
            .map_err(|e| AgentError::ToolError {
                tool_name: "internet_search".into(),
                message: format!("HTTP client error: {e}"),
            })?;

        // Report progress: sending request
        ctx.report_progress(ProgressEvent::Status {
            message: "Sending search request...".into(),
        });

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "internet_search".into(),
                message: format!("HTTP request failed: {e}"),
            })?;

        let status = resp.status();
        if !status.is_success() {
            return Err(AgentError::ToolError {
                tool_name: "internet_search".into(),
                message: format!("Search API returned status {}", status),
            });
        }

        // Report progress: parsing results
        ctx.report_progress(ProgressEvent::Status {
            message: "Parsing search results...".into(),
        });

        let ddg: DdgResponse = resp.json().await.map_err(|e| AgentError::ToolError {
            tool_name: "internet_search".into(),
            message: format!("Failed to parse response: {e}"),
        })?;

        // Build results
        let mut results: Vec<Value> = Vec::new();

        // Add abstract (instant answer) if available
        if !ddg.abstract_text.is_empty() {
            let result_url = &ddg.abstract_url;
            if should_include_result(result_url, &params.allowed_domains, &params.blocked_domains) {
                results.push(json!({
                    "title": ddg.heading,
                    "snippet": ddg.abstract_text,
                    "url": result_url,
                    "source": ddg.abstract_source,
                }));
            }
        }

        // Add related topics
        for topic in ddg.related_topics.iter() {
            if results.len() >= max_results {
                break;
            }
            if topic.text.is_empty() {
                continue;
            }
            if !should_include_result(
                &topic.first_url,
                &params.allowed_domains,
                &params.blocked_domains,
            ) {
                continue;
            }
            results.push(json!({
                "snippet": topic.text,
                "url": topic.first_url,
            }));
        }

        // Report completion progress
        ctx.report_progress(ProgressEvent::Status {
            message: format!("Found {} results", results.len()),
        });

        let output = if results.is_empty() {
            json!({
                "query": params.query,
                "results": [],
                "message": "No results found. Try rephrasing your search query."
            })
        } else {
            json!({
                "query": params.query,
                "results": results,
            })
        };

        Ok(ToolOutput::text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        ))
    }
}

/// Check if a result URL should be included based on domain filters.
fn should_include_result(
    url: &str,
    allowed_domains: &Option<Vec<String>>,
    blocked_domains: &Option<Vec<String>>,
) -> bool {
    // Check allowed domains (if set, only those domains pass)
    if let Some(ref allowed) = allowed_domains {
        if !allowed.is_empty() && !domain_matches(url, allowed) {
            return false;
        }
    }

    // Check blocked domains
    if let Some(ref blocked) = blocked_domains {
        if !blocked.is_empty() && domain_matches(url, blocked) {
            return false;
        }
    }

    true
}

/// Simple URL encoding for the query parameter
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            b' ' => result.push('+'),
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", b));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    //! Pure-logic tests for internet_search helpers. We do NOT exercise
    //! `execute_impl` because its `reqwest::Client` URL is hard-coded to
    //! DDG — testing the request path would require either a refactor
    //! (inject base URL) or hitting a real network (CI-flaky + rate
    //! limit). The helpers cover the domain-filtering and URL-encoding
    //! logic, which is where the business behavior actually lives.
    use super::*;

    // ─── urlencoding ──────────────────────────────────────────────────

    #[test]
    fn urlencoding_preserves_unreserved_chars() {
        // RFC 3986 unreserved: A-Z a-z 0-9 - _ . ~
        assert_eq!(urlencoding("AZaz09-_.~"), "AZaz09-_.~");
    }

    #[test]
    fn urlencoding_encodes_space_as_plus() {
        // Form-encoding convention used by DDG's query param
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(urlencoding(" "), "+");
    }

    #[test]
    fn urlencoding_percent_encodes_special_bytes() {
        // `&` `=` `?` `#` `/` etc. must be percent-encoded or DDG will
        // misparse the query.
        assert_eq!(urlencoding("a&b"), "a%26b");
        assert_eq!(urlencoding("a=b"), "a%3Db");
        assert_eq!(urlencoding("a?b"), "a%3Fb");
        assert_eq!(urlencoding("a/b"), "a%2Fb");
    }

    #[test]
    fn urlencoding_handles_utf8_multibyte() {
        // UTF-8 encoded "中" = 0xE4 0xB8 0xAD → "%E4%B8%AD"
        assert_eq!(urlencoding("中"), "%E4%B8%AD");
    }

    // ─── extract_domain_from_url ──────────────────────────────────────

    #[test]
    fn extract_domain_strips_https_scheme() {
        assert_eq!(
            extract_domain_from_url("https://example.com/path?q=1").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn extract_domain_strips_http_scheme() {
        assert_eq!(
            extract_domain_from_url("http://docs.rs/foo/bar").as_deref(),
            Some("docs.rs")
        );
    }

    #[test]
    fn extract_domain_drops_port() {
        assert_eq!(
            extract_domain_from_url("http://localhost:8080/api").as_deref(),
            Some("localhost")
        );
    }

    #[test]
    fn extract_domain_lowercases() {
        // Domains are case-insensitive — pin the normalisation so
        // `Example.COM` matches a configured allow-list of `example.com`.
        assert_eq!(
            extract_domain_from_url("https://Example.COM/").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn extract_domain_rejects_url_without_scheme() {
        // No http:// or https:// prefix → None. Pinned so a future
        // "smart" URL parser change doesn't silently start accepting
        // schemeless inputs (which the rest of the code path doesn't
        // expect).
        assert!(extract_domain_from_url("example.com/path").is_none());
        assert!(extract_domain_from_url("ftp://example.com/").is_none());
    }

    // ─── domain_matches ───────────────────────────────────────────────

    #[test]
    fn domain_matches_exact() {
        assert!(domain_matches(
            "https://docs.rs/foo",
            &["docs.rs".to_string()]
        ));
        assert!(!domain_matches(
            "https://docs.rs/foo",
            &["example.com".to_string()]
        ));
    }

    #[test]
    fn domain_matches_includes_subdomains() {
        // `github.com` configured → should match `api.github.com`,
        // `raw.githubusercontent.com` does NOT (different root).
        assert!(domain_matches(
            "https://api.github.com/repos",
            &["github.com".to_string()]
        ));
        assert!(!domain_matches(
            "https://raw.githubusercontent.com/foo",
            &["github.com".to_string()]
        ));
    }

    #[test]
    fn domain_matches_is_case_insensitive_in_both_directions() {
        // URL has uppercase; allow-list has mixed case.
        assert!(domain_matches(
            "https://Docs.RS/foo",
            &["DOCS.rs".to_string()]
        ));
    }

    #[test]
    fn domain_matches_returns_false_for_unparseable_url() {
        // No scheme → extract_domain_from_url returns None → no match
        assert!(!domain_matches("example.com", &["example.com".to_string()]));
    }

    // ─── should_include_result ────────────────────────────────────────

    #[test]
    fn should_include_no_filters_accepts_everything() {
        assert!(should_include_result(
            "https://anywhere.example.com/x",
            &None,
            &None
        ));
    }

    #[test]
    fn should_include_allowed_filters_in() {
        let allowed = Some(vec!["docs.rs".to_string()]);
        assert!(should_include_result(
            "https://docs.rs/foo",
            &allowed,
            &None
        ));
        assert!(!should_include_result(
            "https://example.com/foo",
            &allowed,
            &None
        ));
    }

    #[test]
    fn should_include_blocked_filters_out() {
        let blocked = Some(vec!["spam.example".to_string()]);
        assert!(!should_include_result(
            "https://spam.example/x",
            &None,
            &blocked
        ));
        assert!(should_include_result(
            "https://safe.example/x",
            &None,
            &blocked
        ));
    }

    #[test]
    fn should_include_blocked_overrides_allowed() {
        // Same domain in both lists — blocked must win. Pin this so a
        // future refactor doesn't accidentally invert the precedence
        // (which would silently let blocked content through when the
        // user added the domain to both lists by mistake).
        let allowed = Some(vec!["evil.com".to_string()]);
        let blocked = Some(vec!["evil.com".to_string()]);
        assert!(!should_include_result(
            "https://evil.com/x",
            &allowed,
            &blocked
        ));
    }

    #[test]
    fn should_include_empty_lists_are_no_op() {
        // `Some(vec![])` for allowed must NOT block everything — only
        // a non-empty allowed list filters.
        let allowed_empty = Some(Vec::<String>::new());
        let blocked_empty = Some(Vec::<String>::new());
        assert!(should_include_result(
            "https://anything.example/",
            &allowed_empty,
            &blocked_empty
        ));
    }

    // ─── DdgResponse JSON deserialization ─────────────────────────────

    #[test]
    fn ddg_response_pascal_case_deserialization() {
        // DDG returns PascalCase fields; we use #[serde(rename_all =
        // "PascalCase")]. Pin a representative payload so a future
        // typo on `rename_all` would surface here, not in production.
        let json = r#"{
            "AbstractText": "Rust is a language",
            "AbstractSource": "Wikipedia",
            "AbstractURL": "https://en.wikipedia.org/wiki/Rust",
            "Heading": "Rust (programming language)",
            "RelatedTopics": [
                {"Text": "Cargo - Rust's package manager", "FirstURL": "https://crates.io"}
            ]
        }"#;
        let parsed: DdgResponse = serde_json::from_str(json).expect("DDG-shaped json must parse");
        assert_eq!(parsed.abstract_text, "Rust is a language");
        assert_eq!(parsed.abstract_source, "Wikipedia");
        // AbstractURL has all-caps URL — covered by the explicit
        // #[serde(rename = "AbstractURL")] (T11 fix). Without it the
        // PascalCase rule produces `AbstractUrl` and DDG's payload
        // wouldn't match → empty string in production.
        assert_eq!(parsed.abstract_url, "https://en.wikipedia.org/wiki/Rust");
        assert_eq!(parsed.heading, "Rust (programming language)");
        assert_eq!(parsed.related_topics.len(), 1);
        assert_eq!(
            parsed.related_topics[0].text,
            "Cargo - Rust's package manager"
        );
        assert_eq!(parsed.related_topics[0].first_url, "https://crates.io");
    }

    #[test]
    fn ddg_response_handles_missing_fields_with_defaults() {
        // Empty {} should parse to all-defaults thanks to #[serde(default)].
        let parsed: DdgResponse = serde_json::from_str("{}").expect("empty json must parse");
        assert!(parsed.abstract_text.is_empty());
        assert!(parsed.abstract_source.is_empty());
        assert!(parsed.abstract_url.is_empty());
        assert!(parsed.heading.is_empty());
        assert!(parsed.related_topics.is_empty());
    }
}
