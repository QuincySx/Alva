//! internet_search — search the internet using DuckDuckGo Instant Answer API

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Debug, Deserialize)]
struct Input {
    query: String,
    /// Max number of results to return (default 5)
    max_results: Option<usize>,
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

pub struct InternetSearchTool;

#[async_trait]
impl Tool for InternetSearchTool {
    fn name(&self) -> &str {
        "internet_search"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "internet_search".to_string(),
            description: "Search the internet for information. Returns search results with titles, snippets, and URLs.".to_string(),
            parameters: json!({
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
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let max_results = params.max_results.unwrap_or(5);

        // Use DuckDuckGo Instant Answer API (JSON, no auth required)
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencoding(&params.query)
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("SrowAgent/0.1")
            .build()
            .map_err(|e| EngineError::ToolExecution(format!("HTTP client error: {e}")))?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| EngineError::ToolExecution(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(EngineError::ToolExecution(format!(
                "Search API returned status {}",
                status
            )));
        }

        let ddg: DdgResponse = resp
            .json()
            .await
            .map_err(|e| EngineError::ToolExecution(format!("Failed to parse response: {e}")))?;

        // Build results
        let mut results: Vec<Value> = Vec::new();

        // Add abstract (instant answer) if available
        if !ddg.abstract_text.is_empty() {
            results.push(json!({
                "title": ddg.heading,
                "snippet": ddg.abstract_text,
                "url": ddg.abstract_url,
                "source": ddg.abstract_source,
            }));
        }

        // Add related topics
        for topic in ddg.related_topics.iter().take(max_results.saturating_sub(results.len())) {
            if topic.text.is_empty() {
                continue;
            }
            results.push(json!({
                "snippet": topic.text,
                "url": topic.first_url,
            }));
        }

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

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "internet_search".to_string(),
            output: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
            duration_ms,
        })
    }
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
