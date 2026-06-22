// INPUT:  alva_kernel_abi, async_trait, reqwest, schemars, serde, serde_json, std::sync, std::time
// OUTPUT: ReadUrlTool
// POS:    Fetches a web page and returns content with HTML-to-markdown conversion,
//         LRU cache with TTL, rate limiting per domain, and content size limiting.
//! read_url — fetch a web page and return its content (HTML converted to markdown-like text)

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
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

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The URL to fetch.
    url: String,
    /// Maximum content length in characters (default: 50000).
    #[serde(default)]
    max_length: Option<usize>,
    /// Optional prompt for filtering or processing the fetched content.
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
        self.entries.insert(
            url,
            CacheEntry {
                content,
                content_type,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Check rate limit for a domain. Returns true if allowed.
    fn check_rate_limit(&mut self, domain: &str) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);

        let entry = self
            .rate_limits
            .entry(domain.to_string())
            .or_insert_with(|| RateLimitEntry {
                requests: Vec::new(),
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

#[derive(Tool)]
#[tool(
    name = "read_url",
    description = "Fetch a web page URL and return its content with HTML converted to readable text. \
        Includes an LRU cache (15-minute TTL) and per-domain rate limiting. \
        Useful for reading articles, documentation, or any web content.",
    input = Input,
    read_only,
)]
pub struct ReadUrlTool;

impl ReadUrlTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // SSRF defense (T6 / 3C path) is enforced by SecurityMiddleware
        // via the `url_aware_tools` map on SecurityGuard — when this tool
        // runs, the middleware has already inspected the URL and routed
        // it through HITL approval if needed. The tool itself stays
        // simple and assumes the request has been allowed.
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
                    format!(
                        "{}...\n\n[Truncated: content exceeded {} characters]",
                        &content[..end],
                        max_length
                    )
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

                return Ok(ToolOutput::text(
                    serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
                ));
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
            .map_err(|e| AgentError::ToolError {
                tool_name: "read_url".into(),
                message: format!("HTTP client error: {e}"),
            })?;

        let resp = client
            .get(&params.url)
            .send()
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "read_url".into(),
                message: format!("HTTP request failed: {e}"),
            })?;

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

        let body = resp.text().await.map_err(|e| AgentError::ToolError {
            tool_name: "read_url".into(),
            message: format!("Failed to read response body: {e}"),
        })?;

        // Convert HTML to markdown-like text
        let plain_text =
            if content_type.contains("text/html") || content_type.contains("application/xhtml") {
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
            format!(
                "{}...\n\n[Truncated: content exceeded {} characters]",
                &plain_text[..end],
                max_length
            )
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

        Ok(ToolOutput::text(
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        ))
    }
}

/// Extract domain from URL for rate limiting.
fn extract_domain(url: &str) -> Option<String> {
    // Simple extraction: find the host portion
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
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
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(trimmed.len());
        Some(trimmed[..end].to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Pure-logic tests for read_url helpers. The reqwest fetch path
    //! takes a user-supplied URL so wiremock can drive it end-to-end,
    //! but that integration belongs in tests/read_url_*.rs (deferred);
    //! this module covers the local-only logic that runs before/after
    //! the network call.
    use super::*;

    // ─── extract_domain ───────────────────────────────────────────────

    #[test]
    fn extract_domain_strips_scheme_and_path() {
        assert_eq!(
            extract_domain("https://example.com/foo/bar?q=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            extract_domain("http://docs.rs/").as_deref(),
            Some("docs.rs")
        );
    }

    #[test]
    fn extract_domain_drops_port() {
        assert_eq!(
            extract_domain("http://localhost:8080/api").as_deref(),
            Some("localhost")
        );
    }

    #[test]
    fn extract_domain_lowercases() {
        assert_eq!(
            extract_domain("HTTPS://Example.COM/").as_deref(),
            None,
            "scheme prefix match is case-sensitive — pinned current behaviour"
        );
        assert_eq!(
            extract_domain("https://Example.COM/").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn extract_domain_rejects_unknown_scheme() {
        // ftp:// / file:// / no-scheme → None
        assert!(extract_domain("ftp://example.com/").is_none());
        assert!(extract_domain("example.com/path").is_none());
    }

    // ─── extract_href ─────────────────────────────────────────────────

    #[test]
    fn extract_href_handles_three_quoting_styles() {
        // double-quoted (most common)
        assert_eq!(
            extract_href("a href=\"https://x.com\" class=\"link\"").as_deref(),
            Some("https://x.com")
        );
        // single-quoted
        assert_eq!(
            extract_href("a href='https://y.com'").as_deref(),
            Some("https://y.com")
        );
        // unquoted (HTML5 permits — terminates at whitespace or `>`)
        assert_eq!(
            extract_href("a href=https://z.com class=link").as_deref(),
            Some("https://z.com")
        );
    }

    #[test]
    fn extract_href_returns_none_when_attribute_missing() {
        assert!(extract_href("a class=\"foo\"").is_none());
        assert!(extract_href("p").is_none());
    }

    #[test]
    fn extract_href_is_case_insensitive_on_attribute_name() {
        // <a HREF="..."> in legacy HTML should still resolve
        assert_eq!(
            extract_href("a HREF=\"https://upper.com\"").as_deref(),
            Some("https://upper.com")
        );
    }

    // ─── html_to_markdown ─────────────────────────────────────────────

    #[test]
    fn html_to_markdown_renders_headings() {
        let md = html_to_markdown("<h1>Title</h1><h2>Sub</h2><h3>Sub2</h3>");
        assert!(md.contains("# Title"), "h1 → '# Title': {md}");
        assert!(md.contains("## Sub"), "h2 → '## Sub': {md}");
        assert!(md.contains("### Sub2"), "h3 → '### Sub2': {md}");
    }

    #[test]
    fn html_to_markdown_renders_links_with_href() {
        let md = html_to_markdown("Click <a href=\"https://example.com\">here</a> now.");
        // Pinned format: [text](url) markdown style
        assert!(
            md.contains("[here](https://example.com)"),
            "link not rendered as markdown: {md}"
        );
    }

    #[test]
    fn html_to_markdown_strips_script_and_style_content() {
        // Script/style content must be DROPPED, not rendered
        let md = html_to_markdown(
            "<p>visible</p><script>alert('XSS')</script><style>body{color:red}</style><p>also visible</p>",
        );
        assert!(md.contains("visible"), "real content missing: {md}");
        assert!(!md.contains("alert"), "script content leaked: {md}");
        assert!(!md.contains("color:red"), "style content leaked: {md}");
        assert!(!md.contains("XSS"), "script content leaked: {md}");
    }

    #[test]
    fn html_to_markdown_decodes_common_entities() {
        let md = html_to_markdown("AT&amp;T &lt;3 &quot;quoted&quot; &#39;apos&#39;");
        assert!(md.contains("AT&T"), "&amp; not decoded: {md}");
        assert!(md.contains("<3"), "&lt; not decoded: {md}");
        assert!(md.contains("\"quoted\""), "&quot; not decoded: {md}");
        assert!(md.contains("'apos'"), "&#39; not decoded: {md}");
    }

    #[test]
    fn html_to_markdown_renders_lists() {
        let md = html_to_markdown("<ul><li>one</li><li>two</li></ul>");
        // Each li → newline + "- " prefix
        assert!(md.contains("- one"), "missing - one: {md}");
        assert!(md.contains("- two"), "missing - two: {md}");
    }

    #[test]
    fn html_to_markdown_collapses_excessive_blank_lines() {
        // <p><p><p><p> would produce many \n\n; pin the cap at 2
        let md = html_to_markdown("<p>a</p><p>b</p><p>c</p>");
        // No run of 3+ consecutive \n
        assert!(
            !md.contains("\n\n\n"),
            "3+ consecutive newlines escaped collapse: {md:?}"
        );
        assert!(md.contains('a') && md.contains('b') && md.contains('c'));
    }

    #[test]
    fn html_to_markdown_emphasis_tags() {
        let md = html_to_markdown("<strong>bold</strong> and <em>italic</em>");
        assert!(md.contains("**bold**"), "strong → **bold**: {md}");
        assert!(md.contains("*italic*"), "em → *italic*: {md}");
    }

    // ─── UrlCache ─────────────────────────────────────────────────────

    #[test]
    fn url_cache_insert_then_get_roundtrips() {
        let mut cache = UrlCache::new();
        cache.insert("https://x.com".into(), "body".into(), "text/html".into());
        let got = cache.get("https://x.com");
        let (content, ct) = got.expect("must hit");
        assert_eq!(content, "body");
        assert_eq!(ct, "text/html");
    }

    #[test]
    fn url_cache_get_miss_returns_none() {
        let mut cache = UrlCache::new();
        assert!(cache.get("never-inserted").is_none());
    }

    #[test]
    fn url_cache_lru_evicts_oldest_at_capacity() {
        let mut cache = UrlCache::new();
        // Insert MAX + 1 entries → first one must be evicted
        for i in 0..=CACHE_MAX_ENTRIES {
            cache.insert(format!("u{i}"), format!("body{i}"), "text/html".into());
        }
        // u0 was the very first → evicted
        assert!(cache.get("u0").is_none(), "u0 should have been LRU-evicted");
        // u1..uN should still be present
        assert!(cache.get("u1").is_some());
        assert!(cache.get(&format!("u{CACHE_MAX_ENTRIES}")).is_some());
    }

    #[test]
    fn url_cache_get_promotes_to_mru_so_it_survives_next_eviction() {
        // Touch entry 0 after inserting MAX entries → it becomes MRU →
        // subsequent insert evicts entry 1 (now the LRU), not entry 0.
        let mut cache = UrlCache::new();
        for i in 0..CACHE_MAX_ENTRIES {
            cache.insert(format!("u{i}"), format!("body{i}"), "text/html".into());
        }
        let _ = cache.get("u0"); // promotes u0 to MRU
                                 // Adding one more triggers eviction
        cache.insert("new".into(), "x".into(), "text/html".into());
        // u0 should survive; u1 (next-oldest after u0 was promoted) should be gone
        assert!(cache.get("u0").is_some(), "u0 was promoted, must survive");
        assert!(cache.get("u1").is_none(), "u1 became LRU, must be evicted");
    }

    #[test]
    fn url_cache_rate_limit_blocks_after_threshold() {
        let mut cache = UrlCache::new();
        // First RATE_LIMIT_PER_DOMAIN requests must pass; the (N+1)th
        // must return false until the window slides.
        for i in 0..RATE_LIMIT_PER_DOMAIN {
            assert!(
                cache.check_rate_limit("example.com"),
                "request {i} should pass under threshold"
            );
        }
        assert!(
            !cache.check_rate_limit("example.com"),
            "request {} must be rate-limited",
            RATE_LIMIT_PER_DOMAIN + 1
        );
        // Different domain is independent
        assert!(cache.check_rate_limit("other.com"));
    }
}
