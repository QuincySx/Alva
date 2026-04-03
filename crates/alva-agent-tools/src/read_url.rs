// INPUT:  alva_types, async_trait, serde, serde_json, reqwest, std::sync, std::time
// OUTPUT: ReadUrlTool
// POS:    Fetches a web page and returns content with HTML-to-markdown conversion,
//         LRU cache with TTL, rate limiting per domain, and content size limiting.
//! read_url — fetch a web page and return its content (HTML converted to markdown-like text)

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Maximum cache entries.
const CACHE_MAX_ENTRIES: usize = 50;
/// Cache TTL in seconds (15 minutes).
const CACHE_TTL_SECS: u64 = 900;
/// Max requests per domain per minute.
const RATE_LIMIT_PER_DOMAIN: usize = 10;
/// Rate limit window in seconds.
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

#[derive(Debug, Deserialize)]
struct Input {
    url: String,
    /// Maximum content length in characters (default: 50000)
    #[serde(default)]
    max_length: Option<usize>,
    /// Optional prompt for filtering/processing fetched content
    #[serde(default)]
    prompt: Option<String>,
}

/// Cached response entry.
struct CacheEntry {
    content: String,
    content_type: String,
    fetched_at: Instant,
}

/// Rate limit tracker for a domain.
struct RateLimitEntry {
    requests: Vec<Instant>,
}

/// Global cache (process-lifetime).
struct UrlCache {
    entries: HashMap<String, CacheEntry>,
    rate_limits: HashMap<String, RateLimitEntry>,
    /// Insertion order for LRU eviction.
    order: Vec<String>,
}

impl UrlCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            rate_limits: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Get cached content if still valid.
    fn get(&mut self, url: &str) -> Option<(&str, &str)> {
        // Check expiry first and remove if expired
        let expired = self
            .entries
            .get(url)
            .is_some_and(|entry| entry.fetched_at.elapsed() >= Duration::from_secs(CACHE_TTL_SECS));

        if expired {
            self.entries.remove(url);
            self.order.retain(|u| u != url);
            return None;
        }

        if self.entries.contains_key(url) {
            // Move to end of LRU order
            self.order.retain(|u| u != url);
            self.order.push(url.to_string());
            let entry = self.entries.get(url).unwrap();
            return Some((&entry.content, &entry.content_type));
        }
        None
    }

    /// Insert a new cache entry, evicting oldest if at capacity.
    fn insert(&mut self, url: String, content: String, content_type: String) {
        // Evict if at capacity
        while self.entries.len() >= CACHE_MAX_ENTRIES {
            if let Some(oldest) = self.order.first().cloned() {
                self.entries.remove(&oldest);
                self.order.remove(0);
            } else {
                break;
            }
        }

        self.order.retain(|u| u != &url);
        self.order.push(url.clone());
        self.entries.insert(url, CacheEntry {
            content,
            content_type,
            fetched_at: Instant::now(),
        });
    }

    /// Check rate limit for a domain. Returns true if allowed.
    fn check_rate_limit(&mut self, domain: &str) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);

        let entry = self.rate_limits.entry(domain.to_string()).or_insert_with(|| {
            RateLimitEntry { requests: Vec::new() }
        });

        // Remove requests outside the window
        entry.requests.retain(|t| now.duration_since(*t) < window);

        if entry.requests.len() >= RATE_LIMIT_PER_DOMAIN {
            return false;
        }

        entry.requests.push(now);
        true
    }
}

// Use a simple mutex-wrapped global.
// The cache is created on first access via std::sync::OnceLock (stable since 1.70).
fn global_cache() -> &'static Mutex<UrlCache> {
    static CACHE: std::sync::OnceLock<Mutex<UrlCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(UrlCache::new()))
}

pub struct ReadUrlTool;

#[async_trait]
impl Tool for ReadUrlTool {
    fn name(&self) -> &str {
        "read_url"
    }

    fn description(&self) -> &str {
        "Fetch a web page URL and return its content with HTML converted to readable text. \
         Includes an LRU cache (15-minute TTL) and per-domain rate limiting. \
         Useful for reading articles, documentation, or any web content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum content length in characters (default: 50000)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional prompt for filtering or processing the fetched content"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "read_url".into(), message: e.to_string() })?;

        let max_length = params.max_length.unwrap_or(50_000);

        // Extract domain for rate limiting
        let domain = extract_domain(&params.url).unwrap_or_default();

        // Check cache first
        {
            let mut cache = global_cache().lock().unwrap();
            if let Some((cached_content, cached_ct)) = cache.get(&params.url) {
                let content = cached_content.to_string();
                let ct = cached_ct.to_string();

                let truncated = content.len() > max_length;
                let final_content = if truncated {
                    let end = content
                        .char_indices()
                        .nth(max_length)
                        .map(|(i, _)| i)
                        .unwrap_or(content.len());
                    format!("{}...\n\n[Truncated: content exceeded {} characters]", &content[..end], max_length)
                } else {
                    content
                };

                let mut output = json!({
                    "url": params.url,
                    "content": final_content,
                    "content_type": ct,
                    "truncated": truncated,
                    "cached": true,
                });

                if let Some(ref prompt) = params.prompt {
                    output["prompt"] = json!(prompt);
                }

                return Ok(ToolOutput::text(serde_json::to_string_pretty(&output)
                    .unwrap_or_else(|_| "{}".to_string())));
            }

            // Check rate limit
            if !cache.check_rate_limit(&domain) {
                return Err(AgentError::ToolError {
                    tool_name: "read_url".into(),
                    message: format!(
                        "Rate limit exceeded for domain '{}': max {} requests per minute",
                        domain, RATE_LIMIT_PER_DOMAIN
                    ),
                });
            }
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("SrowAgent/0.1 (compatible; bot)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| AgentError::ToolError { tool_name: "read_url".into(), message: format!("HTTP client error: {e}") })?;

        let resp = client
            .get(&params.url)
            .send()
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "read_url".into(), message: format!("HTTP request failed: {e}") })?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            return Err(AgentError::ToolError {
                tool_name: "read_url".into(),
                message: format!("HTTP {} for URL: {}", status, params.url),
            });
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "read_url".into(), message: format!("Failed to read response body: {e}") })?;

        // Convert HTML to markdown-like text
        let plain_text = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
            html_to_markdown(&body)
        } else {
            // Already plain text (JSON, text/plain, etc.)
            body
        };

        // Store in cache
        {
            let mut cache = global_cache().lock().unwrap();
            cache.insert(params.url.clone(), plain_text.clone(), content_type.clone());
        }

        // Truncate if needed
        let truncated = plain_text.len() > max_length;
        let content = if truncated {
            // Truncate at a char boundary
            let end = plain_text
                .char_indices()
                .nth(max_length)
                .map(|(i, _)| i)
                .unwrap_or(plain_text.len());
            format!("{}...\n\n[Truncated: content exceeded {} characters]", &plain_text[..end], max_length)
        } else {
            plain_text
        };

        let mut output = json!({
            "url": params.url,
            "content": content,
            "content_type": content_type,
            "truncated": truncated,
        });

        if let Some(ref prompt) = params.prompt {
            output["prompt"] = json!(prompt);
        }

        Ok(ToolOutput::text(serde_json::to_string_pretty(&output)
            .unwrap_or_else(|_| "{}".to_string())))
    }
}

/// Extract domain from URL for rate limiting.
fn extract_domain(url: &str) -> Option<String> {
    // Simple extraction: find the host portion
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = url.split('/').next()?;
    let domain = host.split(':').next()?; // Remove port
    Some(domain.to_lowercase())
}

/// Convert HTML to markdown-like text.
///
/// Preserves:
/// - Headings (h1-h6) as markdown headings
/// - Links as [text](url)
/// - Paragraphs and line breaks
/// - Lists (basic support)
/// - Bold/italic (basic)
///
/// Removes: scripts, styles, and other non-content tags.
fn html_to_markdown(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_buf = String::new();
    let mut link_href: Option<String> = None;
    let mut link_text = String::new();
    let mut in_link = false;

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            i += 1;
            continue;
        }

        if ch == '>' && in_tag {
            in_tag = false;
            let tag_lower = tag_buf.to_lowercase();
            let tag_name = tag_lower.split_whitespace().next().unwrap_or("");

            match tag_name {
                "script" => in_script = true,
                "/script" => in_script = false,
                "style" => in_style = true,
                "/style" => in_style = false,
                "h1" => result.push_str("\n# "),
                "h2" => result.push_str("\n## "),
                "h3" => result.push_str("\n### "),
                "h4" => result.push_str("\n#### "),
                "h5" => result.push_str("\n##### "),
                "h6" => result.push_str("\n###### "),
                "/h1" | "/h2" | "/h3" | "/h4" | "/h5" | "/h6" => result.push('\n'),
                "br" | "br/" => result.push('\n'),
                "p" | "/p" | "div" | "/div" => result.push_str("\n\n"),
                "li" => result.push_str("\n- "),
                "/li" => {}
                "ul" | "ol" | "/ul" | "/ol" => result.push('\n'),
                "strong" | "b" => result.push_str("**"),
                "/strong" | "/b" => result.push_str("**"),
                "em" | "i" => result.push('*'),
                "/em" | "/i" => result.push('*'),
                "code" => result.push('`'),
                "/code" => result.push('`'),
                "pre" => result.push_str("\n```\n"),
                "/pre" => result.push_str("\n```\n"),
                "tr" | "/tr" => result.push('\n'),
                "td" | "th" => result.push_str(" | "),
                _ if tag_name.starts_with('a') => {
                    // Extract href from tag
                    if let Some(href) = extract_href(&tag_buf) {
                        link_href = Some(href);
                        link_text.clear();
                        in_link = true;
                    }
                }
                "/a" => {
                    if in_link {
                        if let Some(ref href) = link_href {
                            result.push_str(&format!("[{}]({})", link_text.trim(), href));
                        } else {
                            result.push_str(link_text.trim());
                        }
                        in_link = false;
                        link_href = None;
                        link_text.clear();
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }

        if in_tag {
            tag_buf.push(ch);
            i += 1;
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        if in_link {
            link_text.push(ch);
        } else {
            result.push(ch);
        }
        i += 1;
    }

    // Decode common HTML entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Collapse multiple blank lines into at most two newlines
    let mut collapsed = String::with_capacity(result.len());
    let mut consecutive_newlines = 0u32;
    for ch in result.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                collapsed.push('\n');
            }
        } else if ch == '\r' {
            // skip CR
        } else {
            consecutive_newlines = 0;
            collapsed.push(ch);
        }
    }

    // Trim leading/trailing whitespace on each line
    collapsed
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Extract href attribute from an anchor tag buffer.
fn extract_href(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let href_pos = lower.find("href=")?;
    let after_href = &tag[href_pos + 5..];
    let trimmed = after_href.trim_start();

    if trimmed.starts_with('"') {
        let end = trimmed[1..].find('"')?;
        Some(trimmed[1..1 + end].to_string())
    } else if trimmed.starts_with('\'') {
        let end = trimmed[1..].find('\'')?;
        Some(trimmed[1..1 + end].to_string())
    } else {
        let end = trimmed.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(trimmed.len());
        Some(trimmed[..end].to_string())
    }
}
