// INPUT:  reqwest, serde
// OUTPUT: fetch_remote_models / test_connection helpers exposing
//         /v1/models-style endpoints across supported providers.
// POS:    Used by the Settings UI to let the user (a) verify that an
//         api_key / base_url combo actually authenticates, and (b) discover
//         which models the configured endpoint advertises. Keeps API keys
//         in Rust — the webview never makes a direct HTTPS call.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct RemoteModelInfo {
    pub id: String,
    pub display_name: Option<String>,
    pub owned_by: Option<String>,
    pub capabilities: ModelCapabilities,
}

/// Best-effort capability metadata inferred from the model id, since neither
/// OpenAI nor Anthropic's /v1/models exposes these fields. `None` means
/// "unknown, don't assume" — the UI shows it as "?".
#[derive(Serialize, Clone, Default)]
pub struct ModelCapabilities {
    pub supports_tools: Option<bool>,
    pub is_reasoning: Option<bool>,
    pub context_window: Option<u32>,
}

/// Extract capabilities from an OpenRouter `/api/v1/models` entry. Every
/// other provider returns `Default` (all None) — we intentionally do not
/// maintain a regex-based fallback because it's a maintenance trap.
fn extract_openrouter_capabilities(entry: &RawModelEntry) -> ModelCapabilities {
    let context_window = entry.context_length;

    let supports_tools = entry.supported_parameters.as_ref().map(|params| {
        params
            .iter()
            .any(|p| p == "tools" || p == "tool_choice" || p == "function_call")
    });

    // OpenRouter exposes `reasoning` as a supported parameter on models that
    // accept their reasoning-mode request shape (OpenAI o-series, Claude
    // extended thinking, DeepSeek R1, etc.).
    let is_reasoning = entry
        .supported_parameters
        .as_ref()
        .map(|params| params.iter().any(|p| p == "reasoning"));

    ModelCapabilities {
        supports_tools,
        is_reasoning,
        context_window,
    }
}

#[derive(Serialize)]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub status: Option<u16>,
    pub message: Option<String>,
    pub model_count: usize,
}

/// Minimal shape that covers OpenAI / Anthropic / OpenRouter /v1/models
/// responses. OpenAI and Anthropic return nothing useful past the id.
/// OpenRouter returns `context_length` + `supported_parameters` on each
/// entry — the only provider that gives us real capability metadata.
#[derive(Deserialize)]
struct RawResponse {
    #[serde(default)]
    data: Vec<RawModelEntry>,
}

#[derive(Deserialize)]
struct RawModelEntry {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    owned_by: Option<String>,
    // OpenRouter-specific
    #[serde(default)]
    context_length: Option<u32>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}

fn is_openrouter(base_url: &str) -> bool {
    base_url.contains("openrouter.ai")
}

fn resolve_models_url(provider: &str, base_url: &str) -> String {
    let b = base_url.trim_end_matches('/');

    // OpenRouter exposes /api/v1/models. Their recommended base is
    // `https://openrouter.ai/api/v1` for OpenAI-compatibility, so check both
    // shapes.
    if is_openrouter(b) {
        if b.contains("/api/v1") {
            return format!("{b}/models");
        }
        return format!("{b}/api/v1/models");
    }

    match provider {
        "openai" => {
            if b.ends_with("/v1") || b.contains("/v1/") {
                format!("{b}/models")
            } else {
                format!("{b}/v1/models")
            }
        }
        _ => format!("{b}/v1/models"),
    }
}

fn build_request(
    provider: &str,
    api_key: &str,
    base_url: &str,
) -> reqwest::RequestBuilder {
    let url = resolve_models_url(provider, base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("reqwest client");

    let req = client.get(&url);
    match provider {
        "anthropic" => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => req.bearer_auth(api_key),
    }
}

pub async fn fetch_remote_models(
    provider: &str,
    api_key: &str,
    base_url: &str,
) -> Result<Vec<RemoteModelInfo>, String> {
    if api_key.is_empty() {
        return Err("api_key is empty".into());
    }
    let resp = build_request(provider, api_key, base_url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        return Err(format!("HTTP {status}: {snippet}"));
    }

    let parsed: RawResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {e}"))?;

    let openrouter = is_openrouter(base_url);
    let mut out: Vec<RemoteModelInfo> = parsed
        .data
        .into_iter()
        .map(|m| {
            // Real capability data is only available through OpenRouter's
            // enriched response. Everywhere else we leave the fields as
            // `None` and let the UI hide the chips.
            let capabilities = if openrouter {
                extract_openrouter_capabilities(&m)
            } else {
                ModelCapabilities::default()
            };
            RemoteModelInfo {
                id: m.id,
                display_name: m.display_name,
                owned_by: m.owned_by,
                capabilities,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub async fn test_connection(
    provider: &str,
    api_key: &str,
    base_url: &str,
) -> ConnectionTestResult {
    let start = Instant::now();
    if api_key.is_empty() {
        return ConnectionTestResult {
            ok: false,
            latency_ms: 0,
            status: None,
            message: Some("api_key is empty".into()),
            model_count: 0,
        };
    }

    match build_request(provider, api_key, base_url).send().await {
        Ok(resp) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let status = resp.status();
            let ok = status.is_success();
            let mut model_count = 0;
            let mut message = None;
            if ok {
                if let Ok(parsed) = resp.json::<RawResponse>().await {
                    model_count = parsed.data.len();
                }
            } else {
                let body = resp.text().await.unwrap_or_default();
                let snippet: String = body.chars().take(300).collect();
                message = Some(format!("HTTP {status}: {snippet}"));
            }
            ConnectionTestResult {
                ok,
                latency_ms,
                status: Some(status.as_u16()),
                message,
                model_count,
            }
        }
        Err(e) => ConnectionTestResult {
            ok: false,
            latency_ms: start.elapsed().as_millis() as u64,
            status: None,
            message: Some(e.to_string()),
            model_count: 0,
        },
    }
}
