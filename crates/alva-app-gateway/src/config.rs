use std::collections::HashMap;

use alva_llm_provider::{AliasRouter, ProviderConfig};
use serde::Deserialize;

/// Top-level gateway configuration loaded from a YAML routing file.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    /// Address to bind the HTTP server to, e.g. `"127.0.0.1:8787"`.
    pub listen: String,
    /// Map of alias → route configuration.
    pub routes: HashMap<String, RouteConfig>,
}

/// Per-route upstream configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    /// Provider kind, e.g. `"openai-chat"`, `"anthropic"`.
    pub kind: String,
    /// Upstream base URL, e.g. `"https://api.openai.com/v1"`.
    pub base_url: String,
    /// Name of the environment variable that holds the API key.
    pub api_key_env: String,
    /// Model name to forward to the upstream.
    pub model: String,
    /// Optional max tokens override; defaults to 8192 if absent.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl RouteConfig {
    /// Resolve `api_key_env` from the environment and construct a [`ProviderConfig`].
    ///
    /// Returns an error if the environment variable is not set.
    pub fn to_provider_config(&self) -> Result<ProviderConfig, String> {
        let api_key = std::env::var(&self.api_key_env)
            .map_err(|_| format!("env var {} not set for route", self.api_key_env))?;
        Ok(ProviderConfig {
            api_key,
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            max_tokens: self.max_tokens.unwrap_or(8192),
            custom_headers: Default::default(),
            kind: Some(self.kind.clone()),
        })
    }
}

impl GatewayConfig {
    /// Parse a YAML string into a [`GatewayConfig`].
    pub fn from_yaml(s: &str) -> Result<Self, String> {
        serde_yaml::from_str(s).map_err(|e| format!("parse gateway config: {e}"))
    }

    /// Build an [`AliasRouter`] by resolving every route's `api_key_env`.
    ///
    /// Returns an error if any environment variable is missing.
    pub fn build_router(&self) -> Result<AliasRouter, String> {
        let mut router = AliasRouter::new();
        for (alias, rc) in &self.routes {
            router.insert(alias.clone(), rc.to_provider_config()?);
        }
        Ok(router)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_config_resolves_env_to_provider_config() {
        std::env::set_var("TEST_GW_KEY", "secret");
        let rc = RouteConfig {
            kind: "openai-chat".into(),
            base_url: "u".into(),
            api_key_env: "TEST_GW_KEY".into(),
            model: "m".into(),
            max_tokens: None,
        };
        let pc = rc.to_provider_config().unwrap();
        assert_eq!(pc.api_key, "secret");
        assert_eq!(pc.kind.as_deref(), Some("openai-chat"));
        assert_eq!(pc.model, "m");
    }

    #[test]
    fn gateway_config_from_yaml_parses_routes() {
        std::env::set_var("TEST_GW_KEY2", "sek");
        let yaml = r#"
listen: "127.0.0.1:8787"
routes:
  gpt-x:
    kind: openai-chat
    base_url: "https://api.example.com/v1"
    api_key_env: TEST_GW_KEY2
    model: real-model
"#;
        let cfg = GatewayConfig::from_yaml(yaml).unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:8787");
        assert!(cfg.routes.contains_key("gpt-x"));
    }

    #[test]
    fn route_config_missing_env_errors() {
        let rc = RouteConfig {
            kind: "openai-chat".into(),
            base_url: "u".into(),
            api_key_env: "DEFINITELY_UNSET_VAR_XYZ".into(),
            model: "m".into(),
            max_tokens: None,
        };
        assert!(rc.to_provider_config().is_err());
    }
}
