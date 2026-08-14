// INPUT:  reqwest, provider_plane, alva_kernel_abi contracts, resolved alva_llm_provider::ProviderConfig
// OUTPUT: model discovery with binding previews, inference diagnostics, redacted DTOs
// POS:    Hub diagnostics adapter keeping provider HTTP details below Settings and model identity policy backend-owned.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use alva_kernel_abi::{AgentError, Message, ModelConfig};
use alva_llm_provider::auth::{resolve_auth_headers, AuthScheme};
use alva_llm_provider::ProviderConfig;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::provider_plane::RoutePlanner;

const DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_DISCOVERY_BYTES: usize = 4 * 1024 * 1024;
const MAX_DISCOVERED_MODELS: usize = 10_000;
const MAX_MODEL_FIELD_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiscoveredProviderModel {
    pub id: String,
    pub display_name: Option<String>,
    pub suggested_registry_id: Option<String>,
    pub compatible_engines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderDiagnosticResult {
    pub models: Vec<DiscoveredProviderModel>,
    pub ok: bool,
    pub latency_ms: u64,
    pub message: String,
}

#[derive(Debug)]
struct DiscoveryRequestSpec {
    url: String,
    headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelCatalog {
    data: Option<Vec<OpenAiModelEntry>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiModelCatalog {
    models: Option<Vec<GeminiModelEntry>>,
}

#[derive(Debug, Deserialize)]
struct GeminiModelEntry {
    name: String,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
}

/// Discover models advertised by the already-resolved provider route.
///
/// The response deliberately contains no request URL, response body, API key,
/// or custom header value. Settings may use the returned model IDs as a draft;
/// discovery never mutates [`crate::ProviderCatalog`].
pub async fn discover_provider_models(
    kind: &str,
    config: &ProviderConfig,
) -> ProviderDiagnosticResult {
    let started = Instant::now();
    let spec = match discovery_request_spec(kind, config) {
        Ok(spec) => spec,
        Err(message) => return failed(started, message),
    };
    let headers = match request_headers(&spec.headers) {
        Ok(headers) => headers,
        Err(message) => return failed(started, message),
    };
    let client = match reqwest::Client::builder()
        .timeout(DIAGNOSTIC_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return failed(started, "Could not initialize the diagnostic HTTP client."),
    };
    let mut response = match client.get(&spec.url).headers(headers).send().await {
        Ok(response) => response,
        Err(_) => {
            return failed(
                started,
                "Model discovery could not reach the configured provider endpoint.",
            )
        }
    };
    let status = response.status();
    if !status.is_success() {
        return failed(
            started,
            format!("Model discovery failed with HTTP {}.", status.as_u16()),
        );
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DISCOVERY_BYTES as u64)
    {
        return failed(started, "The provider model catalog is too large.");
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if body.len().saturating_add(chunk.len()) <= MAX_DISCOVERY_BYTES => {
                body.extend_from_slice(&chunk);
            }
            Ok(Some(_)) => return failed(started, "The provider model catalog is too large."),
            Ok(None) => break,
            Err(_) => return failed(started, "Could not read the provider model catalog."),
        }
    }
    let models = match parse_models(kind, &body) {
        Ok(models) => models,
        Err(message) => return failed(started, message),
    };
    let models = annotate_discovered_models(kind, models);
    ProviderDiagnosticResult {
        message: format!("Discovered {} model(s).", models.len()),
        models,
        ok: true,
        latency_ms: elapsed_ms(started),
    }
}

/// Send a real minimal inference through the canonical provider factory.
///
/// Success proves the exact model, authentication, endpoint, and provider wire
/// shape are executable. The assistant response and provider error body are not
/// returned because either may echo credentials or custom prompt-injected data.
pub async fn diagnose_provider_inference(
    kind: &str,
    config: &ProviderConfig,
) -> ProviderDiagnosticResult {
    let started = Instant::now();
    if let Err(message) = validate_input(kind, config, true) {
        return failed(started, message);
    }
    let mut diagnostic_config = config.clone();
    diagnostic_config.kind = Some(kind.to_string());
    diagnostic_config.max_tokens = 64;
    let model = alva_llm_provider::build_language_model(Some(kind), diagnostic_config);
    let request_config = ModelConfig {
        max_tokens: Some(32),
        disable_tools: true,
        ..Default::default()
    };
    let messages = [Message::user("Reply with the single word OK.")];
    match tokio::time::timeout(
        DIAGNOSTIC_TIMEOUT,
        model.complete(&messages, &[], &request_config),
    )
    .await
    {
        Ok(Ok(response))
            if response
                .message
                .content
                .iter()
                .any(|block| block.as_text().is_some_and(|text| !text.trim().is_empty())) =>
        {
            ProviderDiagnosticResult {
                models: Vec::new(),
                ok: true,
                latency_ms: elapsed_ms(started),
                message: "Inference succeeded for the selected model.".into(),
            }
        }
        Ok(Ok(_)) => failed(
            started,
            "The provider accepted inference but returned no text.",
        ),
        Ok(Err(error)) => failed(started, redacted_inference_error(&error)),
        Err(_) => failed(started, "Inference diagnostics timed out."),
    }
}

fn discovery_request_spec(
    kind: &str,
    config: &ProviderConfig,
) -> Result<DiscoveryRequestSpec, String> {
    validate_input(kind, config, false)?;
    let scheme = match kind {
        "anthropic" => AuthScheme::XApiKey,
        "gemini" => AuthScheme::GoogApiKey,
        "openai-chat" | "openai-responses" => AuthScheme::Bearer,
        _ => unreachable!("validate_input accepted an unsupported provider kind"),
    };
    let mut headers = resolve_auth_headers(&config.api_key, &config.custom_headers, scheme);
    if kind == "anthropic" {
        headers
            .entry("anthropic-version".into())
            .or_insert_with(|| "2023-06-01".into());
    }
    Ok(DiscoveryRequestSpec {
        url: models_url(kind, &config.base_url),
        headers,
    })
}

fn validate_input(kind: &str, config: &ProviderConfig, require_model: bool) -> Result<(), String> {
    if !matches!(
        kind,
        "anthropic" | "openai-chat" | "openai-responses" | "gemini"
    ) {
        return Err(format!(
            "Provider kind '{kind}' does not support diagnostics."
        ));
    }
    if config.base_url.trim().is_empty() {
        return Err("The provider endpoint is empty.".into());
    }
    validate_base_url(&config.base_url)?;
    if config.api_key.trim().is_empty() && config.custom_headers.is_empty() {
        return Err("Provider authentication is not configured.".into());
    }
    if require_model && config.model.trim().is_empty() {
        return Err("Choose a model before running inference diagnostics.".into());
    }
    Ok(())
}

fn validate_base_url(base_url: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| "The provider endpoint is not a valid URL.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The provider endpoint must be an HTTP or HTTPS URL.".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "The provider endpoint must not contain credentials, a query, or a fragment.".into(),
        );
    }
    Ok(())
}

fn models_url(kind: &str, base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    match kind {
        "gemini" if base.contains("/publishers/google") => {
            if base.ends_with("/models") {
                base.to_string()
            } else {
                format!("{base}/models")
            }
        }
        "gemini" if base.ends_with("/v1beta/models") => base.to_string(),
        "gemini" if base.ends_with("/v1beta") => format!("{base}/models"),
        "gemini" => format!("{base}/v1beta/models"),
        _ if base.ends_with("/v1") => format!("{base}/models"),
        _ => format!("{base}/v1/models"),
    }
}

fn request_headers(headers: &HashMap<String, String>) -> Result<HeaderMap, String> {
    let mut output = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "Provider authentication contains an invalid header name.".to_string())?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| "Provider authentication contains an invalid header value.".to_string())?;
        output.insert(name, value);
    }
    Ok(output)
}

fn parse_models(kind: &str, body: &[u8]) -> Result<Vec<DiscoveredProviderModel>, String> {
    let mut models = if kind == "gemini" {
        let catalog: GeminiModelCatalog = serde_json::from_slice(body)
            .map_err(|_| "The provider returned an invalid model catalog.".to_string())?;
        let entries = catalog
            .models
            .ok_or_else(|| "The provider returned an invalid model catalog.".to_string())?;
        if entries.len() > MAX_DISCOVERED_MODELS {
            return Err("The provider model catalog contains too many models.".into());
        }
        entries
            .into_iter()
            .filter_map(|model| {
                let id = model.name.strip_prefix("models/").unwrap_or(&model.name);
                (!id.trim().is_empty() && valid_model_field(id)).then(|| DiscoveredProviderModel {
                    id: id.to_string(),
                    display_name: model.display_name.filter(|name| valid_model_field(name)),
                    suggested_registry_id: None,
                    compatible_engines: Vec::new(),
                })
            })
            .collect::<Vec<_>>()
    } else {
        let catalog: OpenAiModelCatalog = serde_json::from_slice(body)
            .map_err(|_| "The provider returned an invalid model catalog.".to_string())?;
        let entries = catalog
            .data
            .ok_or_else(|| "The provider returned an invalid model catalog.".to_string())?;
        if entries.len() > MAX_DISCOVERED_MODELS {
            return Err("The provider model catalog contains too many models.".into());
        }
        entries
            .into_iter()
            .filter(|model| !model.id.trim().is_empty() && valid_model_field(&model.id))
            .map(|model| DiscoveredProviderModel {
                id: model.id,
                display_name: model.display_name.filter(|name| valid_model_field(name)),
                suggested_registry_id: None,
                compatible_engines: Vec::new(),
            })
            .collect::<Vec<_>>()
    };
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

fn annotate_discovered_models(
    provider_kind: &str,
    mut models: Vec<DiscoveredProviderModel>,
) -> Vec<DiscoveredProviderModel> {
    let planner = RoutePlanner::bundled();
    for model in &mut models {
        let plan = planner.preview_discovered_model(provider_kind, &model.id);
        model.suggested_registry_id = plan.suggested_registry_id;
        model.compatible_engines = plan.compatible_engines;
    }
    models
}

fn valid_model_field(value: &str) -> bool {
    value.len() <= MAX_MODEL_FIELD_BYTES && !value.chars().any(char::is_control)
}

fn redacted_inference_error(error: &AgentError) -> String {
    let rendered = error.to_string();
    if let Some(status) = http_status(&rendered) {
        return format!("Inference failed with HTTP {status}.");
    }
    match error {
        AgentError::ConfigError(_) => "The provider configuration was rejected.".into(),
        AgentError::Cancelled => "Inference diagnostics were cancelled.".into(),
        _ => "Inference failed before a usable response was returned.".into(),
    }
}

fn http_status(message: &str) -> Option<u16> {
    message
        .split(|character: char| !character.is_ascii_digit())
        .find_map(|token| {
            (token.len() == 3)
                .then(|| token.parse::<u16>().ok())
                .flatten()
                .filter(|status| (400..=599).contains(status))
        })
}

fn failed(started: Instant, message: impl Into<String>) -> ProviderDiagnosticResult {
    ProviderDiagnosticResult {
        models: Vec::new(),
        ok: false,
        latency_ms: elapsed_ms(started),
        message: message.into(),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(kind: &str) -> ProviderConfig {
        ProviderConfig {
            api_key: "synthetic-api-secret".into(),
            model: "synthetic-model".into(),
            base_url: match kind {
                "openai-chat" => "https://api.example.test/v1".into(),
                _ => "https://api.example.test".into(),
            },
            max_tokens: 128,
            custom_headers: HashMap::new(),
            kind: Some(kind.into()),
        }
    }

    #[test]
    fn request_shapes_match_provider_specific_model_endpoints_and_auth() {
        let anthropic = discovery_request_spec("anthropic", &config("anthropic")).unwrap();
        assert_eq!(anthropic.url, "https://api.example.test/v1/models");
        assert_eq!(
            anthropic.headers.get("x-api-key").map(String::as_str),
            Some("synthetic-api-secret")
        );
        assert_eq!(
            anthropic
                .headers
                .get("anthropic-version")
                .map(String::as_str),
            Some("2023-06-01")
        );

        let chat = discovery_request_spec("openai-chat", &config("openai-chat")).unwrap();
        assert_eq!(chat.url, "https://api.example.test/v1/models");
        assert_eq!(
            chat.headers.get("Authorization").map(String::as_str),
            Some("Bearer synthetic-api-secret")
        );

        let responses =
            discovery_request_spec("openai-responses", &config("openai-responses")).unwrap();
        assert_eq!(responses.url, "https://api.example.test/v1/models");

        let gemini = discovery_request_spec("gemini", &config("gemini")).unwrap();
        assert_eq!(gemini.url, "https://api.example.test/v1beta/models");
        assert_eq!(
            gemini.headers.get("x-goog-api-key").map(String::as_str),
            Some("synthetic-api-secret")
        );
    }

    #[test]
    fn custom_headers_replace_api_key_for_discovery() {
        let mut input = config("openai-chat");
        input.custom_headers.insert(
            "X-Synthetic-Authorization".into(),
            "synthetic-custom-secret".into(),
        );
        let spec = discovery_request_spec("openai-chat", &input).unwrap();
        assert!(!spec.headers.contains_key("Authorization"));
        assert_eq!(
            spec.headers
                .get("X-Synthetic-Authorization")
                .map(String::as_str),
            Some("synthetic-custom-secret")
        );
    }

    #[test]
    fn parsers_normalize_sort_and_deduplicate_provider_catalogs() {
        let openai = parse_models(
            "openai-responses",
            br#"{"data":[{"id":"z-model"},{"id":"a-model","display_name":"A"},{"id":"a-model"}]}"#,
        )
        .unwrap();
        assert_eq!(
            openai,
            vec![
                DiscoveredProviderModel {
                    id: "a-model".into(),
                    display_name: Some("A".into()),
                    suggested_registry_id: None,
                    compatible_engines: Vec::new(),
                },
                DiscoveredProviderModel {
                    id: "z-model".into(),
                    display_name: None,
                    suggested_registry_id: None,
                    compatible_engines: Vec::new(),
                },
            ]
        );

        let gemini = parse_models(
            "gemini",
            br#"{"models":[{"name":"models/gemini-z","displayName":"Gemini Z"},{"name":"models/gemini-a"}]}"#,
        )
        .unwrap();
        assert_eq!(gemini[0].id, "gemini-a");
        assert_eq!(gemini[1].id, "gemini-z");
        assert_eq!(gemini[1].display_name.as_deref(), Some("Gemini Z"));
    }

    #[test]
    fn discovered_models_carry_a_non_authoritative_registry_binding_preview() {
        let models = annotate_discovered_models(
            "openai-responses",
            vec![
                DiscoveredProviderModel {
                    id: "openai/gpt-5.6-sol".into(),
                    display_name: None,
                    suggested_registry_id: None,
                    compatible_engines: Vec::new(),
                },
                DiscoveredProviderModel {
                    id: "vendor-future-model".into(),
                    display_name: None,
                    suggested_registry_id: None,
                    compatible_engines: Vec::new(),
                },
            ],
        );
        assert_eq!(
            models[0].suggested_registry_id.as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(models[0].compatible_engines, ["alva", "codex", "pi"]);
        assert_eq!(models[1].suggested_registry_id, None);
        assert_eq!(models[1].compatible_engines, ["alva", "codex", "pi"]);
    }

    #[test]
    fn malformed_catalog_error_never_echoes_response_body() {
        let secret_body = br#"{"secret":"body-synthetic-secret"}"#;
        let error = parse_models("anthropic", secret_body).unwrap_err();
        assert_eq!(error, "The provider returned an invalid model catalog.");
        assert!(!error.contains("body-synthetic-secret"));
    }

    #[test]
    fn catalog_limits_model_count_and_field_lengths() {
        let too_many = serde_json::to_vec(&serde_json::json!({
            "data": (0..=MAX_DISCOVERED_MODELS)
                .map(|index| serde_json::json!({ "id": format!("model-{index}") }))
                .collect::<Vec<_>>()
        }))
        .unwrap();
        assert_eq!(
            parse_models("openai-chat", &too_many).unwrap_err(),
            "The provider model catalog contains too many models."
        );

        let long_id = "x".repeat(MAX_MODEL_FIELD_BYTES + 1);
        let body = serde_json::to_vec(&serde_json::json!({
            "data": [{ "id": long_id }, { "id": "valid-model", "display_name": "ok" }]
        }))
        .unwrap();
        let models = parse_models("openai-chat", &body).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "valid-model");
    }

    #[test]
    fn inference_errors_keep_only_safe_http_status() {
        let error = AgentError::LlmError(
            "HTTP 401 body={synthetic-api-secret: body-synthetic-secret}".into(),
        );
        let message = redacted_inference_error(&error);
        assert_eq!(message, "Inference failed with HTTP 401.");
        assert!(!message.contains("synthetic-api-secret"));
        assert!(!message.contains("body-synthetic-secret"));

        let generic = redacted_inference_error(&AgentError::Other(
            "custom header synthetic-custom-secret failed".into(),
        ));
        assert_eq!(
            generic,
            "Inference failed before a usable response was returned."
        );
        assert!(!generic.contains("synthetic-custom-secret"));
    }

    #[test]
    fn validation_fails_locally_without_credentials_or_supported_kind() {
        let mut input = config("anthropic");
        input.api_key.clear();
        assert_eq!(
            discovery_request_spec("anthropic", &input).unwrap_err(),
            "Provider authentication is not configured."
        );
        assert!(discovery_request_spec("unknown", &config("anthropic"))
            .unwrap_err()
            .contains("does not support diagnostics"));

        let mut unsafe_endpoint = config("anthropic");
        unsafe_endpoint.base_url =
            "https://synthetic-user:synthetic-secret@example.test/v1?key=secret".into();
        let error = discovery_request_spec("anthropic", &unsafe_endpoint).unwrap_err();
        assert_eq!(
            error,
            "The provider endpoint must not contain credentials, a query, or a fragment."
        );
        assert!(!error.contains("synthetic-secret"));
    }
}
