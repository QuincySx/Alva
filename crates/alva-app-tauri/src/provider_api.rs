// INPUT:  reqwest, serde, std::time, alva_kernel_abi::{LanguageModel, Message, ModelConfig}, alva_llm_provider::*
// OUTPUT: RemoteModelInfo + fetch_remote_models for the models listing UI;
//         ConnectionTestResult + test_connection for the "Test connection" button — real inference ping, not /v1/models count.
// POS:    Used by the Settings UI to let the user (a) browse what models the configured endpoint advertises and (b) verify
//         that their api_key + model combo actually works end-to-end (auth + routing + model availability) with a minimal prompt.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::model::{LanguageModel, ModelConfig};
use alva_kernel_abi::ContentBlock;
use alva_llm_provider::ProviderConfig;

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
    /// Per-model output token cap (separate from `context_window` — input
    /// space is much larger than what the model will emit in one
    /// response). Pulled from OpenRouter's `top_provider.max_completion_tokens`
    /// when available; other providers don't expose it on their list
    /// endpoints, so it stays `None` and the user can override in
    /// Settings. The actual `max_tokens` we send falls back to a
    /// pi-mono-style `32_000` default.
    pub max_output_tokens: Option<u32>,
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

    let max_output_tokens = entry
        .top_provider
        .as_ref()
        .and_then(|tp| tp.max_completion_tokens);

    ModelCapabilities {
        supports_tools,
        is_reasoning,
        context_window,
        max_output_tokens,
    }
}

/// Result of a real inference-ping connection test (not a /v1/models count).
///
/// A connection test isn't useful if it only proves the /models endpoint is
/// reachable — users need to know whether their **specific model** actually
/// works end-to-end (auth right? model available on this key? request body
/// well-formed for this backend?). So the test sends a tiny user message
/// through the configured provider + model and reports what comes back.
#[derive(Serialize, Clone)]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    /// Error / status message. On success: a short confirmation. On failure:
    /// the provider error text (HTTP status + body, parse error, etc.).
    pub message: Option<String>,
    /// The model id that was tested — echoed back so the UI can confirm
    /// "we really pinged this one".
    pub model: Option<String>,
    /// First ~200 chars of the assistant's text reply. Lets the user see a
    /// live response rather than just "OK".
    pub sample_response: Option<String>,
    /// Input token count from the response usage, if the provider populated it.
    pub input_tokens: Option<u32>,
    /// Output token count.
    pub output_tokens: Option<u32>,
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
    #[serde(default)]
    top_provider: Option<RawTopProvider>,
}

/// OpenRouter's nested `top_provider` block that carries the real
/// per-model output-token cap. Fields default to None — providers that
/// don't expose it leave the block out entirely.
#[derive(Deserialize)]
struct RawTopProvider {
    #[serde(default)]
    max_completion_tokens: Option<u32>,
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

    // Helper: return "{b}/models" if `b` already ends with /v1, else "{b}/v1/models".
    // Users frequently configure `base_url` either way (`api.anthropic.com` or
    // `api.anthropic.com/v1`) and we shouldn't double up the /v1 segment.
    let v1_models = |b: &str| -> String {
        if b.ends_with("/v1") {
            format!("{b}/models")
        } else {
            format!("{b}/v1/models")
        }
    };

    match provider {
        "openai" | "openai-responses" | "anthropic" => v1_models(b),
        // Gemini API: /v1beta/models, response shape is {models: [{name, displayName, ...}]}
        // rather than OpenAI's {data: [...]}. We hit the endpoint but `fetch_remote_models`
        // will return empty because the RawResponse shape doesn't match — the UI still
        // works (user can type model name manually).
        "gemini" => {
            if b.ends_with("/v1beta") {
                format!("{b}/models")
            } else {
                format!("{b}/v1beta/models")
            }
        }
        _ => v1_models(b),
    }
}

fn build_request(
    provider: &str,
    api_key: &str,
    base_url: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let url = resolve_models_url(provider, base_url);
    // Client::builder().build() can fail when the system's TLS backend
    // can't initialize (missing CA store, broken certs, etc.). Surface
    // that as a user-visible error rather than panicking the Tauri IPC
    // handler.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("failed to build reqwest client (TLS/cert init): {e}"))?;

    let req = client.get(&url);
    let req = match provider {
        "anthropic" => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        "gemini" => req.header("x-goog-api-key", api_key),
        _ => req.bearer_auth(api_key),
    };
    Ok(req)
}

pub async fn fetch_remote_models(
    provider: &str,
    api_key: &str,
    base_url: &str,
) -> Result<Vec<RemoteModelInfo>, String> {
    if api_key.is_empty() {
        return Err("api_key is empty".into());
    }
    let url = resolve_models_url(provider, base_url);
    let resp = build_request(provider, api_key, base_url)?
        .send()
        .await
        .map_err(|e| format!("request failed: {e} (url: {url})"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        // 404 on the models endpoint is usually a proxy that only
        // implements the inference path — not an auth or config problem.
        // Tell the user how to proceed rather than just surfacing the
        // raw HTTP response.
        if status.as_u16() == 404 {
            return Err(format!(
                "HTTP 404 @ {url} — this endpoint isn't implemented by your \
                 provider/proxy. Inference probably still works; type the \
                 model name manually in the Model field."
            ));
        }
        return Err(format!("HTTP {status} @ {url} — {snippet}"));
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

// ---------------------------------------------------------------------------
// test_connection — real inference ping
// ---------------------------------------------------------------------------

/// Send a minimal prompt ("Reply with OK.") to the configured provider +
/// model and report what came back. This proves:
/// - API key is valid for this endpoint
/// - The model id is actually served here
/// - The request body shape is accepted (so agent runs won't fail at turn 1)
/// - Rough end-to-end latency
///
/// Much more useful than pinging /v1/models because that endpoint can list
/// stale model names that actually 404 on inference, or succeed with an API
/// key that lacks access to the specific model the user configured.
pub async fn test_connection(
    provider: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> ConnectionTestResult {
    let start = Instant::now();
    if api_key.is_empty() {
        return ConnectionTestResult {
            ok: false,
            latency_ms: 0,
            message: Some("api_key is empty".into()),
            model: None,
            sample_response: None,
            input_tokens: None,
            output_tokens: None,
        };
    }
    if model.is_empty() {
        return ConnectionTestResult {
            ok: false,
            latency_ms: 0,
            message: Some("model is empty — pick a model before testing".into()),
            model: None,
            sample_response: None,
            input_tokens: None,
            output_tokens: None,
        };
    }

    let config = ProviderConfig {
        api_key: api_key.to_string(),
        model: model.to_string(),
        base_url: base_url.to_string(),
        max_tokens: 64,
        custom_headers: Default::default(),
        kind: Some(provider.to_string()),
    };

    // Single kind→provider switch lives in alva-llm-provider (PR-10).
    let lm: Arc<dyn LanguageModel> =
        alva_llm_provider::build_language_model(Some(provider), config);

    let messages = vec![Message::user(
        "Respond with the single word OK and nothing else.",
    )];
    let model_config = ModelConfig {
        temperature: Some(0.0),
        max_tokens: Some(32),
        ..Default::default()
    };

    match lm.complete(&messages, &[], &model_config).await {
        Ok(resp) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let text = resp
                .message
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let sample: String = text.chars().take(200).collect();
            let ok = !sample.is_empty();
            ConnectionTestResult {
                ok,
                latency_ms,
                message: if ok {
                    Some(format!("OK ({latency_ms} ms)"))
                } else {
                    Some("provider returned empty response".into())
                },
                model: Some(model.to_string()),
                sample_response: if sample.is_empty() {
                    None
                } else {
                    Some(sample)
                },
                input_tokens: resp.message.usage.as_ref().map(|u| u.input_tokens),
                output_tokens: resp.message.usage.as_ref().map(|u| u.output_tokens),
            }
        }
        Err(e) => ConnectionTestResult {
            ok: false,
            latency_ms: start.elapsed().as_millis() as u64,
            message: Some(e.to_string()),
            model: Some(model.to_string()),
            sample_response: None,
            input_tokens: None,
            output_tokens: None,
        },
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the URL-routing helpers used by `fetch_remote_models`.
    //! Wrong routing → wrong endpoint → 404 / auth errors that confuse
    //! users; these tests pin the per-provider behavior + /v1 suffix
    //! handling that has been hand-tuned over time.
    use super::*;

    // -- is_openrouter -----------------------------------------------------

    #[test]
    fn is_openrouter_detects_canonical_host() {
        assert!(is_openrouter("https://openrouter.ai"));
        assert!(is_openrouter("https://openrouter.ai/api/v1"));
        assert!(is_openrouter("https://openrouter.ai/"));
    }

    #[test]
    fn is_openrouter_negative_cases() {
        assert!(!is_openrouter("https://api.openai.com"));
        assert!(!is_openrouter("https://api.anthropic.com"));
        assert!(!is_openrouter("https://router.example.com"));
        assert!(!is_openrouter(""));
    }

    // -- resolve_models_url: OpenRouter ------------------------------------

    #[test]
    fn resolve_models_url_openrouter_with_api_v1_appends_models() {
        let url = resolve_models_url("openai-chat", "https://openrouter.ai/api/v1");
        assert_eq!(url, "https://openrouter.ai/api/v1/models");
    }

    #[test]
    fn resolve_models_url_openrouter_without_api_v1_adds_full_path() {
        let url = resolve_models_url("openai-chat", "https://openrouter.ai");
        assert_eq!(url, "https://openrouter.ai/api/v1/models");
    }

    #[test]
    fn resolve_models_url_openrouter_trims_trailing_slash() {
        let url = resolve_models_url("openai-chat", "https://openrouter.ai/");
        assert_eq!(url, "https://openrouter.ai/api/v1/models");
    }

    // -- resolve_models_url: openai / openai-responses / anthropic --------

    #[test]
    fn resolve_models_url_openai_without_v1_adds_v1_models() {
        assert_eq!(
            resolve_models_url("openai", "https://api.openai.com"),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn resolve_models_url_openai_with_v1_avoids_double_v1() {
        // Regression guard against `https://...v1/v1/models` doubling.
        assert_eq!(
            resolve_models_url("openai", "https://api.openai.com/v1"),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn resolve_models_url_openai_responses_uses_same_v1_path() {
        assert_eq!(
            resolve_models_url("openai-responses", "https://api.openai.com"),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn resolve_models_url_anthropic_uses_v1_models() {
        assert_eq!(
            resolve_models_url("anthropic", "https://api.anthropic.com"),
            "https://api.anthropic.com/v1/models"
        );
        // Also with the /v1 suffix in base
        assert_eq!(
            resolve_models_url("anthropic", "https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/models"
        );
    }

    #[test]
    fn resolve_models_url_strips_trailing_slash_before_routing() {
        // The trim_end_matches('/') happens before the per-provider
        // switch, so every branch should yield the same URL with/without
        // a trailing slash on the base.
        let a = resolve_models_url("openai", "https://api.openai.com/v1/");
        let b = resolve_models_url("openai", "https://api.openai.com/v1");
        assert_eq!(a, b);
    }

    // -- resolve_models_url: gemini ---------------------------------------

    #[test]
    fn resolve_models_url_gemini_uses_v1beta_models() {
        assert_eq!(
            resolve_models_url("gemini", "https://generativelanguage.googleapis.com"),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
    }

    #[test]
    fn resolve_models_url_gemini_with_v1beta_avoids_double() {
        assert_eq!(
            resolve_models_url("gemini", "https://generativelanguage.googleapis.com/v1beta"),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
    }

    // -- resolve_models_url: unknown provider fallback --------------------

    #[test]
    fn resolve_models_url_unknown_provider_falls_back_to_v1_models() {
        // Match arm `_ => v1_models(b)` — preserves backward-compat for
        // proxies/wrappers that aren't explicitly named.
        assert_eq!(
            resolve_models_url("future-provider", "https://my-proxy.example.com"),
            "https://my-proxy.example.com/v1/models"
        );
    }

    // -- extract_openrouter_capabilities -----------------------------------
    //
    // Construct RawModelEntry via serde_json::from_str so the test reads
    // like a real OpenRouter API response slice. This doubles as a fixture
    // reference for future wiremock-based fetch_remote_models tests.

    fn parse_entry(json: &str) -> RawModelEntry {
        serde_json::from_str(json).expect("RawModelEntry JSON fixture is malformed")
    }

    #[test]
    fn extract_caps_bare_entry_yields_all_none() {
        // Minimal OpenAI/Anthropic-shaped entry — only `id`. All capability
        // fields stay None and the UI hides their chips.
        let entry = parse_entry(r#"{ "id": "gpt-4o" }"#);
        let caps = extract_openrouter_capabilities(&entry);
        assert_eq!(caps.context_window, None);
        assert_eq!(caps.supports_tools, None);
        assert_eq!(caps.is_reasoning, None);
        assert_eq!(caps.max_output_tokens, None);
    }

    #[test]
    fn extract_caps_context_length_passes_through() {
        let entry =
            parse_entry(r#"{ "id": "anthropic/claude-3.7-sonnet", "context_length": 200000 }"#);
        let caps = extract_openrouter_capabilities(&entry);
        assert_eq!(caps.context_window, Some(200_000));
    }

    #[test]
    fn extract_caps_supports_tools_via_tools_keyword() {
        let entry =
            parse_entry(r#"{ "id": "x", "supported_parameters": ["tools", "temperature"] }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).supports_tools,
            Some(true)
        );
    }

    #[test]
    fn extract_caps_supports_tools_via_tool_choice_keyword() {
        // OpenRouter sometimes exposes only `tool_choice` for OpenAI-style
        // function calling models — must still be treated as tool-capable.
        let entry = parse_entry(r#"{ "id": "x", "supported_parameters": ["tool_choice"] }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).supports_tools,
            Some(true)
        );
    }

    #[test]
    fn extract_caps_supports_tools_via_function_call_keyword() {
        // Legacy OpenAI naming — older models surface `function_call` instead
        // of `tools`. Still considered tool-capable.
        let entry = parse_entry(r#"{ "id": "x", "supported_parameters": ["function_call"] }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).supports_tools,
            Some(true)
        );
    }

    #[test]
    fn extract_caps_supports_tools_false_when_none_of_the_keywords() {
        // `supported_parameters` is present but none of the three tool keys —
        // result should be `Some(false)`, NOT `None`. UI uses the
        // distinction: Some(false) shows "no tools" chip, None shows "?".
        let entry =
            parse_entry(r#"{ "id": "x", "supported_parameters": ["temperature", "top_p"] }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).supports_tools,
            Some(false)
        );
    }

    #[test]
    fn extract_caps_is_reasoning_true_when_keyword_present() {
        let entry =
            parse_entry(r#"{ "id": "openai/o1", "supported_parameters": ["reasoning", "tools"] }"#);
        let caps = extract_openrouter_capabilities(&entry);
        assert_eq!(caps.is_reasoning, Some(true));
        assert_eq!(caps.supports_tools, Some(true));
    }

    #[test]
    fn extract_caps_is_reasoning_false_when_other_params_present() {
        let entry = parse_entry(r#"{ "id": "x", "supported_parameters": ["temperature"] }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).is_reasoning,
            Some(false)
        );
    }

    #[test]
    fn extract_caps_max_output_tokens_from_top_provider() {
        // Real OpenRouter shape: { "top_provider": { "max_completion_tokens": 8192 } }
        let entry =
            parse_entry(r#"{ "id": "x", "top_provider": { "max_completion_tokens": 8192 } }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).max_output_tokens,
            Some(8192)
        );
    }

    #[test]
    fn extract_caps_max_output_tokens_none_when_top_provider_missing_field() {
        // top_provider block exists but the nested field is absent — the
        // serde default for max_completion_tokens is None, so caps stays None.
        let entry = parse_entry(r#"{ "id": "x", "top_provider": {} }"#);
        assert_eq!(
            extract_openrouter_capabilities(&entry).max_output_tokens,
            None
        );
    }

    #[test]
    fn extract_caps_full_realistic_openrouter_entry() {
        // Approximates a real `/api/v1/models` row for a reasoning model
        // with tool support — exercises all four capability fields at
        // once. Doubles as a fixture reference for wiremock tests.
        let entry = parse_entry(
            r#"{
                "id": "anthropic/claude-3.7-sonnet:thinking",
                "display_name": "Claude 3.7 Sonnet (Thinking)",
                "owned_by": "anthropic",
                "context_length": 200000,
                "supported_parameters": ["tools", "tool_choice", "reasoning", "temperature"],
                "top_provider": { "max_completion_tokens": 64000 }
            }"#,
        );
        let caps = extract_openrouter_capabilities(&entry);
        assert_eq!(caps.context_window, Some(200_000));
        assert_eq!(caps.supports_tools, Some(true));
        assert_eq!(caps.is_reasoning, Some(true));
        assert_eq!(caps.max_output_tokens, Some(64_000));
    }
}
