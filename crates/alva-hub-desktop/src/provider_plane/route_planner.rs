// INPUT:  BTreeMap, ModelRegistry, ProviderModelOptions
// OUTPUT: pure RoutePlanner and per-engine PlannedModelRoute values
// POS:    Deterministic Provider x Model x Engine policy projection without persistence or network access.

use std::collections::BTreeMap;

use super::{ModelRegistry, ProviderModelOptions};

const CLAUDE_SUBSCRIPTION_ENGINES: &[&str] = &["claude"];
const CODEX_SUBSCRIPTION_ENGINES: &[&str] = &["codex"];
const ANTHROPIC_ENGINES: &[&str] = &["alva", "claude", "pi"];
const OPENAI_RESPONSES_ENGINES: &[&str] = &["alva", "codex", "pi"];
const OPENAI_CHAT_ENGINES: &[&str] = &["alva", "pi"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedModelRoute {
    pub(crate) engine_id: String,
    pub(crate) options: ProviderModelOptions,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RoutePlanningInput<'a> {
    pub(crate) provider_id: &'a str,
    pub(crate) provider_kind: &'a str,
    pub(crate) provider_supported_engines: &'a [String],
    pub(crate) model_id: &'a str,
    pub(crate) model_registry_id: Option<&'a str>,
    pub(crate) model_supported_engines: &'a [String],
    pub(crate) runtime_options: Option<&'a BTreeMap<String, ProviderModelOptions>>,
}

/// Pure projection of provider protocol, model declaration, and engine rules.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoutePlanner {
    registry: ModelRegistry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredModelPlan {
    pub(crate) suggested_registry_id: Option<String>,
    pub(crate) compatible_engines: Vec<String>,
}

impl RoutePlanner {
    pub(crate) const fn bundled() -> Self {
        Self {
            registry: ModelRegistry::bundled(),
        }
    }

    pub(crate) fn compatible_engines(
        &self,
        provider_kind: &str,
    ) -> Option<&'static [&'static str]> {
        match provider_kind {
            "claude-subscription" => Some(CLAUDE_SUBSCRIPTION_ENGINES),
            "codex-subscription" => Some(CODEX_SUBSCRIPTION_ENGINES),
            "anthropic" => Some(ANTHROPIC_ENGINES),
            "openai-responses" => Some(OPENAI_RESPONSES_ENGINES),
            "openai-chat" | "gemini" => Some(OPENAI_CHAT_ENGINES),
            _ => None,
        }
    }

    pub(crate) fn provider_engines(
        &self,
        provider_kind: &str,
        configured_engines: &[String],
    ) -> Vec<String> {
        self.compatible_engines(provider_kind)
            .map(|engines| engines.iter().map(|engine| (*engine).to_string()).collect())
            .unwrap_or_else(|| configured_engines.to_vec())
    }

    pub(crate) fn supports_engine(&self, provider_kind: &str, engine_id: &str) -> bool {
        self.compatible_engines(provider_kind)
            .is_some_and(|engines| engines.contains(&engine_id))
    }

    pub(crate) fn normalize_registry_binding(
        &self,
        provider_id: &str,
        model_id: &str,
        requested_registry_id: Option<&str>,
    ) -> Option<String> {
        let canonical_id = self.registry.canonical_id_for(model_id)?;
        match requested_registry_id {
            Some(requested) if requested == canonical_id => Some(canonical_id.to_string()),
            None if is_bundled_catalog(provider_id) => Some(canonical_id.to_string()),
            _ => None,
        }
    }

    pub(crate) fn preview_discovered_model(
        &self,
        provider_kind: &str,
        model_id: &str,
    ) -> DiscoveredModelPlan {
        DiscoveredModelPlan {
            suggested_registry_id: self.registry.canonical_id_for(model_id).map(str::to_string),
            compatible_engines: self.provider_engines(provider_kind, &[]),
        }
    }

    pub(crate) fn plan_model(&self, input: RoutePlanningInput<'_>) -> Vec<PlannedModelRoute> {
        self.engine_ids_for_model(&input)
            .into_iter()
            .map(|engine_id| PlannedModelRoute {
                options: self.options_for(&input, &engine_id),
                engine_id,
            })
            .collect()
    }

    pub(crate) fn options_for(
        &self,
        input: &RoutePlanningInput<'_>,
        engine_id: &str,
    ) -> ProviderModelOptions {
        let registry_id = self.normalize_registry_binding(
            input.provider_id,
            input.model_id,
            input.model_registry_id,
        );
        let Some(registry_id) = registry_id else {
            return ProviderModelOptions::default();
        };
        if let Some(options) = input
            .runtime_options
            .and_then(|options| options.get(engine_id))
        {
            return options.clone();
        }
        let Some(mut options) = self.registry.options_for(&registry_id, engine_id) else {
            return ProviderModelOptions::default();
        };
        options.supports_fast_mode &= engine_id == "codex"
            && matches!(
                input.provider_kind,
                "openai-responses" | "codex-subscription"
            );
        options
    }

    fn engine_ids_for_model(&self, input: &RoutePlanningInput<'_>) -> Vec<String> {
        if let Some(engines) = self.compatible_engines(input.provider_kind) {
            return engines.iter().map(|engine| (*engine).to_string()).collect();
        }
        if input.model_supported_engines.is_empty() {
            input.provider_supported_engines.to_vec()
        } else {
            input.model_supported_engines.to_vec()
        }
    }
}

fn is_bundled_catalog(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "anthropic" | "openai-responses" | "claude-cli" | "codex-cli"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        provider_id: &'a str,
        provider_kind: &'a str,
        model_id: &'a str,
    ) -> RoutePlanningInput<'a> {
        RoutePlanningInput {
            provider_id,
            provider_kind,
            provider_supported_engines: &[],
            model_id,
            model_registry_id: None,
            model_supported_engines: &[],
            runtime_options: None,
        }
    }

    #[test]
    fn protocol_matrix_matches_product_contract() {
        let planner = RoutePlanner::bundled();
        assert_eq!(
            planner.compatible_engines("claude-subscription"),
            Some(&["claude"][..])
        );
        assert_eq!(
            planner.compatible_engines("codex-subscription"),
            Some(&["codex"][..])
        );
        assert_eq!(
            planner.compatible_engines("anthropic"),
            Some(&["alva", "claude", "pi"][..])
        );
        assert_eq!(
            planner.compatible_engines("openai-responses"),
            Some(&["alva", "codex", "pi"][..])
        );
        assert_eq!(
            planner.compatible_engines("openai-chat"),
            Some(&["alva", "pi"][..])
        );
        assert_eq!(
            planner.compatible_engines("gemini"),
            Some(&["alva", "pi"][..])
        );
    }

    #[test]
    fn one_provider_model_plans_runtime_specific_routes() {
        let planner = RoutePlanner::bundled();
        let routes =
            planner.plan_model(input("openai-responses", "openai-responses", "gpt-5.6-sol"));
        assert_eq!(
            routes
                .iter()
                .map(|route| route.engine_id.as_str())
                .collect::<Vec<_>>(),
            ["alva", "codex", "pi"]
        );
        let codex = routes
            .iter()
            .find(|route| route.engine_id == "codex")
            .unwrap();
        assert!(codex.options.supports_fast_mode);
        assert!(codex
            .options
            .reasoning_efforts
            .contains(&"ultra".to_string()));
        assert!(routes
            .iter()
            .filter(|route| route.engine_id != "codex")
            .all(|route| !route.options.supports_fast_mode
                && !route
                    .options
                    .reasoning_efforts
                    .contains(&"ultra".to_string())));
    }

    #[test]
    fn unknown_models_and_untrusted_catalogs_never_invent_controls() {
        let planner = RoutePlanner::bundled();
        assert!(planner
            .options_for(
                &input("codex-cli", "codex-subscription", "engine-default"),
                "codex"
            )
            .is_empty());
        assert!(planner
            .options_for(&input("custom", "openai-responses", "gpt-5.6-sol"), "codex")
            .is_empty());
    }

    #[test]
    fn explicit_registry_binding_is_required_for_untrusted_provider_ids() {
        let planner = RoutePlanner::bundled();
        let mut custom = input("custom", "openai-responses", "openai/gpt-5.6-sol");
        assert_eq!(
            planner.preview_discovered_model(custom.provider_kind, custom.model_id),
            DiscoveredModelPlan {
                suggested_registry_id: Some("gpt-5.6-sol".into()),
                compatible_engines: vec!["alva".into(), "codex".into(), "pi".into()],
            }
        );
        assert!(planner.options_for(&custom, "codex").is_empty());

        custom.model_registry_id = Some("gpt-5.6-sol");
        let bound = planner.options_for(&custom, "codex");
        assert!(bound.supports_fast_mode);
        assert!(bound.reasoning_efforts.contains(&"ultra".to_string()));

        custom.model_registry_id = Some("claude-opus-4-7");
        assert!(planner.options_for(&custom, "codex").is_empty());
    }
}
