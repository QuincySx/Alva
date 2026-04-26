// INPUT:  std::sync::Arc, alva_kernel_abi::{Provider, ProviderRegistry, LanguageModel, ProviderError}, crate::{ProviderConfig, *}
// OUTPUT: build_provider_registry, ConfigProviderAdapter
// POS:    Wraps a ProviderConfig into a `dyn Provider` so any host (CLI / Tauri / tests)
//         can register the active provider on the bus uniformly. Sub-agents resolve
//         `kind/<model_id>` against the same auth config (different model from same provider).

use std::sync::Arc;

use alva_kernel_abi::{LanguageModel, Provider, ProviderError, ProviderRegistry};

use crate::{
    AnthropicProvider, GeminiProvider, OpenAIChatProvider, OpenAIResponsesProvider, ProviderConfig,
};

/// Resolves the canonical provider id (`anthropic` / `openai-chat` /
/// `openai-responses` / `gemini`) from a config's `kind` field, with
/// `openai-chat` as the broad default.
pub fn provider_id_from_config(config: &ProviderConfig) -> &'static str {
    match config.kind.as_deref() {
        Some("anthropic") => "anthropic",
        Some("openai-responses") => "openai-responses",
        Some("gemini") => "gemini",
        _ => "openai-chat",
    }
}

/// Single-provider adapter — produces a fresh `LanguageModel` for any
/// `model_id` reusing the wrapped config's auth (api_key, base_url,
/// max_tokens, headers).
///
/// Multi-provider concurrent registration (e.g. anthropic + openai both
/// configured) requires a settings-schema change and is out of scope of
/// this adapter. With a single active config this still enables
/// sub-agents to choose a different *model* from the same provider.
pub struct ConfigProviderAdapter {
    id: &'static str,
    base: ProviderConfig,
}

impl ConfigProviderAdapter {
    pub fn new(base: ProviderConfig) -> Self {
        Self {
            id: provider_id_from_config(&base),
            base,
        }
    }

    fn config_for(&self, model_id: &str) -> ProviderConfig {
        ProviderConfig {
            model: model_id.to_string(),
            ..self.base.clone()
        }
    }
}

impl Provider for ConfigProviderAdapter {
    fn id(&self) -> &str {
        self.id
    }

    fn language_model(
        &self,
        model_id: &str,
    ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
        let cfg = self.config_for(model_id);
        let lm: Arc<dyn LanguageModel> = match self.id {
            "anthropic" => Arc::new(AnthropicProvider::new(cfg)),
            "openai-responses" => Arc::new(OpenAIResponsesProvider::new(cfg)),
            "gemini" => Arc::new(GeminiProvider::new(cfg)),
            _ => Arc::new(OpenAIChatProvider::new(cfg)),
        };
        Ok(lm)
    }
}

/// Convenience: build a `ProviderRegistry` containing a single active
/// provider derived from `config`. Suitable for hosts that have one
/// active provider per session (CLI, Tauri).
pub fn build_provider_registry(config: &ProviderConfig) -> Arc<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(ConfigProviderAdapter::new(config.clone())));
    Arc::new(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            api_key: "k".into(),
            model: "default-model".into(),
            base_url: "https://example/v1".into(),
            max_tokens: 1024,
            custom_headers: Default::default(),
            kind: kind.map(String::from),
        }
    }

    #[test]
    fn adapter_id_maps_known_kinds() {
        assert_eq!(ConfigProviderAdapter::new(cfg(Some("anthropic"))).id(), "anthropic");
        assert_eq!(
            ConfigProviderAdapter::new(cfg(Some("openai-responses"))).id(),
            "openai-responses"
        );
        assert_eq!(ConfigProviderAdapter::new(cfg(Some("gemini"))).id(), "gemini");
        assert_eq!(ConfigProviderAdapter::new(cfg(None)).id(), "openai-chat");
        assert_eq!(
            ConfigProviderAdapter::new(cfg(Some("unknown-kind"))).id(),
            "openai-chat"
        );
    }

    #[test]
    fn adapter_overrides_model_id() {
        let a = ConfigProviderAdapter::new(cfg(Some("openai-chat")));
        let cfg = a.config_for("gpt-4o-mini");
        assert_eq!(cfg.model, "gpt-4o-mini");
        assert_eq!(cfg.api_key, "k");
        assert_eq!(cfg.base_url, "https://example/v1");
    }

    #[test]
    fn registry_resolves_active_kind() {
        let reg = build_provider_registry(&cfg(Some("anthropic")));
        // dyn LanguageModel doesn't impl Debug — match instead of expect.
        match reg.language_model("anthropic", "claude-opus-4-7") {
            Ok(_) => {}
            Err(e) => panic!("registry should resolve anthropic kind, got {e:?}"),
        }
    }

    #[test]
    fn registry_misses_unregistered_kind() {
        let reg = build_provider_registry(&cfg(Some("anthropic")));
        match reg.language_model("openai-chat", "gpt-4o") {
            Ok(_) => panic!("expected NoSuchModel error"),
            Err(ProviderError::NoSuchModel { .. }) => {}
            Err(other) => panic!("expected NoSuchModel, got {other:?}"),
        }
    }

    #[test]
    fn registry_listed_id() {
        let reg = build_provider_registry(&cfg(Some("gemini")));
        let ids: Vec<&str> = reg.provider_ids();
        assert_eq!(ids, vec!["gemini"]);
    }
}
