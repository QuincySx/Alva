// INPUT:  alva_types, async_trait, serde, serde_json, reqwest
// OUTPUT: InternetSearchTool
// POS:    Searches the internet using DuckDuckGo Instant Answer API with domain filtering
//         and progress tracking.
//! internet_search — search the internet using DuckDuckGo Instant Answer API

use alva_types::{AgentError, ProgressEvent, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    query: String,
    /// Max number of results to return (default 5)
    #[serde(default)]
    max_results: Option<usize>,
    /// Only return results from these domains.
    #[serde(default)]
    allowed_domains: Option<Vec<String>>,
    /// Exclude results from these domains.
    #[serde(default)]
    blocked_domains: Option<Vec<String>>,
}

/// DuckDuckGo API response (partial)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DdgResponse {
    #[serde(default)]
    abstract_text: String,
    #[serde(default)]
    abstract_source: String,
    #[serde(default)]
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
    #[serde(default)]
    first_url: String,
}

/// Extract domain from a URL for filtering purposes.
fn extract_domain_from_url(url: &str) -> Option<String> {
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
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

pub struct InternetSearchTool;

#[async_trait]
impl Tool for InternetSearchTool {
    fn name(&self) -> &str {
        "internet_search"
    }

    fn description(&self) -> &str {
        "Search the internet for information. Returns search results with titles, snippets, and URLs. \
         Supports domain filtering (allowed/blocked) for focused searches."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 5)"
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only return results from these domains (e.g., ['docs.rs', 'github.com'])"
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Exclude results from these domains"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "internet_search".into(), message: e.to_string() })?;

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
            .map_err(|e| AgentError::ToolError { tool_name: "internet_search".into(), message: format!("HTTP client error: {e}") })?;

        // Report progress: sending request
        ctx.report_progress(ProgressEvent::Status {
            message: "Sending search request...".into(),
        });

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "internet_search".into(), message: format!("HTTP request failed: {e}") })?;

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

        let ddg: DdgResponse = resp
            .json()
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "internet_search".into(), message: format!("Failed to parse response: {e}") })?;

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
            if !should_include_result(&topic.first_url, &params.allowed_domains, &params.blocked_domains) {
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

        Ok(ToolOutput::text(serde_json::to_string_pretty(&output)
            .unwrap_or_else(|_| "{}".to_string())))
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
