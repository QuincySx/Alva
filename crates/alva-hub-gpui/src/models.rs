// INPUT:  ProviderConfig plus persisted custom provider metadata
// OUTPUT: Runtime/provider/model catalog, capability-aware routes, route guard
// POS:    Hub product model plane shared by composer, settings, and engine assembly

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntime {
    Alva,
    ClaudeCode,
    Codex,
    Pi,
}

impl AgentRuntime {
    pub const ALL: [Self; 4] = [Self::Alva, Self::ClaudeCode, Self::Codex, Self::Pi];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Alva => "alva",
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Alva => "ALVA",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Pi => "Pi",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|runtime| runtime.id() == value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
    Gemini,
    NativeSubscription,
}

impl ApiProtocol {
    pub const fn id(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic",
            Self::OpenAiChat => "openai_chat",
            Self::OpenAiResponses => "openai_responses",
            Self::Gemini => "gemini",
            Self::NativeSubscription => "native_subscription",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "Anthropic Messages",
            Self::OpenAiChat => "OpenAI Chat Completions",
            Self::OpenAiResponses => "OpenAI Responses",
            Self::Gemini => "Gemini GenerateContent",
            Self::NativeSubscription => "Native subscription",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::AnthropicMessages,
            Self::OpenAiChat,
            Self::OpenAiResponses,
            Self::Gemini,
            Self::NativeSubscription,
        ]
        .into_iter()
        .find(|protocol| protocol.id() == value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    Subscription,
    ApiKey,
    Local,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    ExtraHigh,
}

impl ReasoningEffort {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::ExtraHigh => "extra_high",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "No reasoning",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::ExtraHigh => "Extra high",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Off,
            Self::Low,
            Self::Medium,
            Self::High,
            Self::ExtraHigh,
        ]
        .into_iter()
        .find(|effort| effort.id() == value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub text: bool,
    pub images: bool,
    pub tools: bool,
    pub streaming: bool,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub reasoning_efforts: Vec<ReasoningEffort>,
    pub default_reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProfile {
    pub provider_id: String,
    pub display_name: String,
    pub auth: ProviderAuth,
    pub protocol: ApiProtocol,
    pub base_url: Option<String>,
    pub runtimes: Vec<AgentRuntime>,
    pub connected: bool,
    pub built_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProfile {
    pub model_id: String,
    pub display_name: String,
    pub provider_id: String,
    pub protocol: ApiProtocol,
    pub runtimes: Vec<AgentRuntime>,
    pub capabilities: ModelCapabilities,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRoute {
    pub runtime: AgentRuntime,
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomProviderProfile {
    pub provider_id: String,
    pub display_name: String,
    pub protocol: ApiProtocol,
    pub base_url: String,
    pub runtimes: Vec<AgentRuntime>,
    pub models: Vec<CustomModelProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomModelProfile {
    pub model_id: String,
    pub display_name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub images: bool,
    pub tools: bool,
    pub reasoning_efforts: Vec<ReasoningEffort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredCustomProvider {
    pub profile: CustomProviderProfile,
    pub api_key: String,
}

#[derive(Clone)]
pub struct ProviderRuntimeConfig {
    pub protocol: ApiProtocol,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Clone, Default)]
pub struct ModelCatalog {
    providers: Vec<ProviderProfile>,
    models: Vec<ModelProfile>,
    runtime_configs: HashMap<String, ProviderRuntimeConfig>,
}

impl ModelCatalog {
    pub fn load(workspace: &Path, custom: &[StoredCustomProvider]) -> Self {
        let mut catalog = Self::built_in();
        if let Ok(config) = alva_llm_provider::ProviderConfig::load(workspace) {
            catalog.add_workspace_provider(config);
        }
        for provider in custom {
            catalog.add_custom_provider(provider.clone());
        }
        catalog
    }

    pub fn built_in() -> Self {
        let mut catalog = Self::default();
        catalog.providers.extend([
            ProviderProfile {
                provider_id: "claude-subscription".into(),
                display_name: "Claude subscription".into(),
                auth: ProviderAuth::Subscription,
                protocol: ApiProtocol::NativeSubscription,
                base_url: None,
                runtimes: vec![AgentRuntime::ClaudeCode],
                connected: true,
                built_in: true,
            },
            ProviderProfile {
                provider_id: "codex-subscription".into(),
                display_name: "ChatGPT subscription".into(),
                auth: ProviderAuth::Subscription,
                protocol: ApiProtocol::NativeSubscription,
                base_url: None,
                runtimes: vec![AgentRuntime::Codex],
                connected: true,
                built_in: true,
            },
        ]);
        catalog.models.extend([
            subscription_model(
                "claude-subscription",
                "engine-default",
                "Claude Code default",
                AgentRuntime::ClaudeCode,
                200_000,
                vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                ],
            ),
            subscription_model(
                "codex-subscription",
                "engine-default",
                "Codex default",
                AgentRuntime::Codex,
                200_000,
                vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                    ReasoningEffort::ExtraHigh,
                ],
            ),
        ]);
        catalog
    }

    pub fn providers(&self) -> &[ProviderProfile] {
        &self.providers
    }

    pub fn models(&self) -> &[ModelProfile] {
        &self.models
    }

    pub fn provider(&self, provider_id: &str) -> Option<&ProviderProfile> {
        self.providers
            .iter()
            .find(|provider| provider.provider_id == provider_id)
    }

    pub fn model(&self, provider_id: &str, model_id: &str) -> Option<&ModelProfile> {
        self.models
            .iter()
            .find(|model| model.provider_id == provider_id && model.model_id == model_id)
    }

    pub fn runtime_config(&self, provider_id: &str) -> Option<&ProviderRuntimeConfig> {
        self.runtime_configs.get(provider_id)
    }

    pub fn models_for_runtime(&self, runtime: AgentRuntime) -> Vec<&ModelProfile> {
        self.models
            .iter()
            .filter(|model| {
                model.enabled
                    && model.runtimes.contains(&runtime)
                    && self.provider(&model.provider_id).is_some_and(|provider| {
                        provider.runtimes.contains(&runtime)
                            && protocol_supports_runtime(provider.protocol, runtime)
                    })
            })
            .collect()
    }

    pub fn default_route(&self, runtime: AgentRuntime) -> Option<ModelRoute> {
        let model = self.models_for_runtime(runtime).into_iter().find(|model| {
            self.provider(&model.provider_id)
                .is_some_and(|provider| provider.connected)
        })?;
        Some(ModelRoute {
            runtime,
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            reasoning_effort: model.capabilities.default_reasoning_effort,
        })
    }

    pub fn resolve(&self, mut route: ModelRoute) -> Result<ModelRoute, String> {
        let provider = self
            .provider(&route.provider_id)
            .ok_or_else(|| format!("provider '{}' was not found", route.provider_id))?;
        if !provider.connected {
            return Err(format!(
                "provider '{}' is not connected",
                provider.display_name
            ));
        }
        if !provider.runtimes.contains(&route.runtime) {
            return Err(format!(
                "{} cannot run through {}",
                route.runtime.label(),
                provider.display_name
            ));
        }
        if !protocol_supports_runtime(provider.protocol, route.runtime) {
            return Err(format!(
                "{} does not support the {} API used by {}",
                route.runtime.label(),
                provider.protocol.label(),
                provider.display_name
            ));
        }
        let model = self
            .model(&route.provider_id, &route.model_id)
            .ok_or_else(|| format!("model '{}' was not found for this provider", route.model_id))?;
        if !model.enabled || !model.runtimes.contains(&route.runtime) {
            return Err(format!(
                "model '{}' does not support {}",
                model.display_name,
                route.runtime.label()
            ));
        }
        if let Some(effort) = route.reasoning_effort {
            if !model.capabilities.reasoning_efforts.contains(&effort) {
                route.reasoning_effort = model.capabilities.default_reasoning_effort;
            }
        } else {
            route.reasoning_effort = model.capabilities.default_reasoning_effort;
        }
        Ok(route)
    }

    pub fn engine_target(&self, route: ModelRoute) -> Result<crate::store::EngineTarget, String> {
        let route = self.resolve(route)?;
        let provider = self
            .provider(&route.provider_id)
            .expect("resolved route provider must exist");
        let model = self
            .model(&route.provider_id, &route.model_id)
            .expect("resolved route model must exist");
        let mut required_capabilities = vec!["streaming".to_string()];
        if model.capabilities.images {
            required_capabilities.push("images".to_string());
        }
        if model.capabilities.tools {
            required_capabilities.push("tools".to_string());
        }
        Ok(crate::store::EngineTarget {
            engine_id: route.runtime.id().to_string(),
            model_id: route.model_id,
            provider_id: route.provider_id,
            reasoning_effort: route.reasoning_effort.map(|effort| effort.id().to_string()),
            account_source: provider.display_name.clone(),
            billing_route: match provider.auth {
                ProviderAuth::Subscription => "Subscription".to_string(),
                ProviderAuth::ApiKey => "Provider API account".to_string(),
                ProviderAuth::Local => "Local model".to_string(),
            },
            automatic_use_authorized: false,
            required_capabilities,
        })
    }

    fn add_workspace_provider(&mut self, config: alva_llm_provider::ProviderConfig) {
        let protocol = protocol_from_kind(config.kind.as_deref());
        let runtime_config = ProviderRuntimeConfig {
            protocol,
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
        };
        self.providers.push(ProviderProfile {
            provider_id: "alva-workspace".into(),
            display_name: "ALVA workspace provider".into(),
            auth: ProviderAuth::ApiKey,
            protocol,
            base_url: Some(config.base_url),
            runtimes: vec![AgentRuntime::Alva, AgentRuntime::Pi],
            connected: !config.api_key.is_empty(),
            built_in: true,
        });
        self.models.push(ModelProfile {
            model_id: config.model.clone(),
            display_name: config.model,
            provider_id: "alva-workspace".into(),
            protocol,
            runtimes: vec![AgentRuntime::Alva, AgentRuntime::Pi],
            capabilities: capabilities_for_protocol(protocol, config.max_tokens),
            enabled: true,
        });
        self.runtime_configs
            .insert("alva-workspace".into(), runtime_config);
    }

    fn add_custom_provider(&mut self, stored: StoredCustomProvider) {
        let custom = stored.profile;
        let provider_id = custom.provider_id.clone();
        let runtime_config = ProviderRuntimeConfig {
            protocol: custom.protocol,
            base_url: custom.base_url.clone(),
            api_key: stored.api_key.clone(),
        };
        self.providers.push(ProviderProfile {
            provider_id: custom.provider_id.clone(),
            display_name: custom.display_name,
            auth: ProviderAuth::ApiKey,
            protocol: custom.protocol,
            base_url: Some(custom.base_url),
            runtimes: custom.runtimes.clone(),
            connected: !stored.api_key.trim().is_empty(),
            built_in: false,
        });
        self.models
            .extend(custom.models.into_iter().map(|model| ModelProfile {
                model_id: model.model_id,
                display_name: model.display_name,
                provider_id: custom.provider_id.clone(),
                protocol: custom.protocol,
                runtimes: custom.runtimes.clone(),
                capabilities: ModelCapabilities {
                    text: true,
                    images: model.images,
                    tools: model.tools,
                    streaming: true,
                    context_window: model.context_window,
                    max_output_tokens: model.max_output_tokens,
                    default_reasoning_effort: model.reasoning_efforts.last().copied(),
                    reasoning_efforts: model.reasoning_efforts,
                },
                enabled: true,
            }));
        self.runtime_configs.insert(provider_id, runtime_config);
    }
}

pub fn protocol_supports_runtime(protocol: ApiProtocol, runtime: AgentRuntime) -> bool {
    match runtime {
        AgentRuntime::Alva | AgentRuntime::Pi => protocol != ApiProtocol::NativeSubscription,
        AgentRuntime::ClaudeCode => matches!(
            protocol,
            ApiProtocol::NativeSubscription | ApiProtocol::AnthropicMessages
        ),
        AgentRuntime::Codex => matches!(
            protocol,
            ApiProtocol::NativeSubscription | ApiProtocol::OpenAiResponses
        ),
    }
}

fn subscription_model(
    provider_id: &str,
    model_id: &str,
    display_name: &str,
    runtime: AgentRuntime,
    context_window: u32,
    reasoning_efforts: Vec<ReasoningEffort>,
) -> ModelProfile {
    ModelProfile {
        model_id: model_id.into(),
        display_name: display_name.into(),
        provider_id: provider_id.into(),
        protocol: ApiProtocol::NativeSubscription,
        runtimes: vec![runtime],
        capabilities: ModelCapabilities {
            text: true,
            images: true,
            tools: true,
            streaming: true,
            context_window,
            max_output_tokens: 32_000,
            default_reasoning_effort: reasoning_efforts.last().copied(),
            reasoning_efforts,
        },
        enabled: true,
    }
}

fn protocol_from_kind(kind: Option<&str>) -> ApiProtocol {
    match kind {
        Some("anthropic") => ApiProtocol::AnthropicMessages,
        Some("openai-responses") => ApiProtocol::OpenAiResponses,
        Some("gemini") => ApiProtocol::Gemini,
        _ => ApiProtocol::OpenAiChat,
    }
}

fn capabilities_for_protocol(protocol: ApiProtocol, max_output_tokens: u32) -> ModelCapabilities {
    let reasoning_efforts = match protocol {
        ApiProtocol::OpenAiResponses | ApiProtocol::AnthropicMessages => vec![
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
        ],
        _ => Vec::new(),
    };
    ModelCapabilities {
        text: true,
        images: matches!(
            protocol,
            ApiProtocol::AnthropicMessages | ApiProtocol::OpenAiResponses | ApiProtocol::Gemini
        ),
        tools: true,
        streaming: true,
        context_window: 128_000,
        max_output_tokens,
        default_reasoning_effort: reasoning_efforts.last().copied(),
        reasoning_efforts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_models_are_runtime_scoped() {
        let catalog = ModelCatalog::built_in();
        let claude = catalog.models_for_runtime(AgentRuntime::ClaudeCode);
        let codex = catalog.models_for_runtime(AgentRuntime::Codex);

        assert_eq!(claude.len(), 1);
        assert_eq!(claude[0].provider_id, "claude-subscription");
        assert_eq!(codex.len(), 1);
        assert_eq!(codex[0].provider_id, "codex-subscription");
    }

    #[test]
    fn route_guard_rejects_cross_runtime_provider() {
        let catalog = ModelCatalog::built_in();
        let error = catalog
            .resolve(ModelRoute {
                runtime: AgentRuntime::Codex,
                provider_id: "claude-subscription".into(),
                model_id: "engine-default".into(),
                reasoning_effort: None,
            })
            .unwrap_err();

        assert!(error.contains("cannot run"));
    }

    #[test]
    fn unsupported_effort_falls_back_to_model_default() {
        let catalog = ModelCatalog::built_in();
        let route = catalog
            .resolve(ModelRoute {
                runtime: AgentRuntime::ClaudeCode,
                provider_id: "claude-subscription".into(),
                model_id: "engine-default".into(),
                reasoning_effort: Some(ReasoningEffort::ExtraHigh),
            })
            .unwrap();

        assert_eq!(route.reasoning_effort, Some(ReasoningEffort::High));
    }

    #[test]
    fn custom_provider_declares_compatible_runtimes() {
        let mut catalog = ModelCatalog::built_in();
        catalog.add_custom_provider(StoredCustomProvider {
            profile: CustomProviderProfile {
                provider_id: "acme".into(),
                display_name: "Acme".into(),
                protocol: ApiProtocol::OpenAiResponses,
                base_url: "https://example.invalid".into(),
                runtimes: vec![AgentRuntime::Alva, AgentRuntime::Codex],
                models: vec![CustomModelProfile {
                    model_id: "reasoner".into(),
                    display_name: "Reasoner".into(),
                    context_window: 256_000,
                    max_output_tokens: 32_000,
                    images: true,
                    tools: true,
                    reasoning_efforts: vec![ReasoningEffort::Medium, ReasoningEffort::High],
                }],
            },
            api_key: "secret".into(),
        });

        assert!(catalog
            .resolve(ModelRoute {
                runtime: AgentRuntime::Codex,
                provider_id: "acme".into(),
                model_id: "reasoner".into(),
                reasoning_effort: Some(ReasoningEffort::High),
            })
            .is_ok());
        assert!(catalog
            .models_for_runtime(AgentRuntime::ClaudeCode)
            .iter()
            .all(|model| model.provider_id != "acme"));
    }

    #[test]
    fn codex_rejects_chat_completions_only_providers() {
        assert!(!protocol_supports_runtime(
            ApiProtocol::OpenAiChat,
            AgentRuntime::Codex
        ));
        assert!(protocol_supports_runtime(
            ApiProtocol::OpenAiResponses,
            AgentRuntime::Codex
        ));
    }

    #[test]
    fn default_route_skips_disconnected_providers() {
        let mut catalog = ModelCatalog::built_in();
        catalog.add_custom_provider(StoredCustomProvider {
            profile: CustomProviderProfile {
                provider_id: "offline".into(),
                display_name: "Offline provider".into(),
                protocol: ApiProtocol::OpenAiResponses,
                base_url: "https://example.invalid".into(),
                runtimes: vec![AgentRuntime::Alva],
                models: vec![CustomModelProfile {
                    model_id: "offline-model".into(),
                    display_name: "Offline model".into(),
                    context_window: 128_000,
                    max_output_tokens: 32_000,
                    images: true,
                    tools: true,
                    reasoning_efforts: vec![ReasoningEffort::High],
                }],
            },
            api_key: String::new(),
        });

        assert!(catalog.default_route(AgentRuntime::Alva).is_none());
        assert!(catalog.default_route(AgentRuntime::ClaudeCode).is_some());
    }
}
