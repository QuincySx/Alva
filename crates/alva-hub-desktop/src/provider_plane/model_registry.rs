// INPUT:  ProviderModelOptions
// OUTPUT: versioned bundled ModelRegistry lookup
// POS:    Static model declaration authority independent of provider connections and runtime availability.

use super::ProviderModelOptions;

const BUNDLED_MODEL_REGISTRY_VERSION: u32 = 1;

const GPT_REASONING: &[&str] = &["low", "medium", "high", "xhigh"];
const GPT_56_REASONING: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const GPT_56_CODEX_REASONING: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultra"];
const CLAUDE_5_REASONING: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const CLAUDE_46_REASONING: &[&str] = &["low", "medium", "high", "max"];
const CLAUDE_45_REASONING: &[&str] = &["low", "medium", "high"];

#[derive(Debug, Clone, Copy)]
struct ModelCapabilities {
    reasoning_efforts: &'static [&'static str],
    default_reasoning_effort: Option<&'static str>,
    supports_fast_mode: bool,
}

#[derive(Debug, Clone, Copy)]
struct EngineOverride {
    engine_id: &'static str,
    capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Copy)]
struct ModelDefinition {
    id: &'static str,
    capabilities: ModelCapabilities,
    engine_overrides: &'static [EngineOverride],
}

const fn capabilities(
    reasoning_efforts: &'static [&'static str],
    default_reasoning_effort: Option<&'static str>,
    supports_fast_mode: bool,
) -> ModelCapabilities {
    ModelCapabilities {
        reasoning_efforts,
        default_reasoning_effort,
        supports_fast_mode,
    }
}

const CODEX_SOL_OVERRIDE: &[EngineOverride] = &[EngineOverride {
    engine_id: "codex",
    capabilities: capabilities(GPT_56_CODEX_REASONING, Some("high"), true),
}];

const MODELS: &[ModelDefinition] = &[
    ModelDefinition {
        id: "gpt-5.6-sol",
        capabilities: capabilities(GPT_56_REASONING, Some("high"), true),
        engine_overrides: CODEX_SOL_OVERRIDE,
    },
    ModelDefinition {
        id: "gpt-5.6-terra",
        capabilities: capabilities(GPT_56_REASONING, Some("high"), true),
        engine_overrides: CODEX_SOL_OVERRIDE,
    },
    ModelDefinition {
        id: "gpt-5.6-luna",
        capabilities: capabilities(GPT_56_REASONING, None, false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "gpt-5.5",
        capabilities: capabilities(GPT_REASONING, Some("high"), true),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "gpt-5.4",
        capabilities: capabilities(GPT_REASONING, Some("high"), true),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "gpt-5.4-mini",
        capabilities: capabilities(GPT_REASONING, Some("high"), true),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "gpt-5.4-nano",
        capabilities: capabilities(GPT_REASONING, None, false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-opus-5",
        capabilities: capabilities(CLAUDE_5_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-opus-4-8",
        capabilities: capabilities(CLAUDE_5_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-opus-4-7",
        capabilities: capabilities(CLAUDE_5_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-sonnet-5",
        capabilities: capabilities(CLAUDE_5_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-fable-5",
        capabilities: capabilities(CLAUDE_5_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-opus-4-6",
        capabilities: capabilities(CLAUDE_46_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-sonnet-4-6",
        capabilities: capabilities(CLAUDE_46_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
    ModelDefinition {
        id: "claude-opus-4-5",
        capabilities: capabilities(CLAUDE_45_REASONING, Some("high"), false),
        engine_overrides: &[],
    },
];

/// Versioned, immutable metadata bundled with this product build.
///
/// Provider discovery may later add a separate remote snapshot, but it must
/// not mutate these declarations or silently invent controls for unknown IDs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModelRegistry {
    schema_version: u32,
    models: &'static [ModelDefinition],
}

impl ModelRegistry {
    pub(crate) const fn bundled() -> Self {
        Self {
            schema_version: BUNDLED_MODEL_REGISTRY_VERSION,
            models: MODELS,
        }
    }

    pub(crate) const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub(crate) fn canonical_id_for(&self, model_id: &str) -> Option<&'static str> {
        let candidate = model_id.rsplit('/').next().unwrap_or(model_id);
        self.models
            .iter()
            .find(|definition| definition.id == candidate)
            .map(|definition| definition.id)
    }

    pub(crate) fn options_for(
        &self,
        model_id: &str,
        engine_id: &str,
    ) -> Option<ProviderModelOptions> {
        debug_assert_eq!(self.schema_version(), BUNDLED_MODEL_REGISTRY_VERSION);
        let canonical_id = self.canonical_id_for(model_id)?;
        let definition = self
            .models
            .iter()
            .find(|definition| definition.id == canonical_id)?;
        let capabilities = definition
            .engine_overrides
            .iter()
            .find(|override_| override_.engine_id == engine_id)
            .map(|override_| override_.capabilities)
            .unwrap_or(definition.capabilities);
        Some(ProviderModelOptions {
            reasoning_efforts: capabilities
                .reasoning_efforts
                .iter()
                .map(|effort| (*effort).to_string())
                .collect(),
            default_reasoning_effort: capabilities.default_reasoning_effort.map(str::to_string),
            supports_fast_mode: capabilities.supports_fast_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_registry_is_versioned_and_unknown_models_stay_unknown() {
        let registry = ModelRegistry::bundled();
        assert_eq!(registry.schema_version(), 1);
        assert_eq!(
            registry.canonical_id_for("openai/gpt-5.6-sol"),
            Some("gpt-5.6-sol")
        );
        assert!(registry.options_for("engine-default", "codex").is_none());
        assert!(registry.options_for("future-model", "alva").is_none());
    }

    #[test]
    fn engine_override_is_data_not_a_runtime_guess() {
        let registry = ModelRegistry::bundled();
        let alva = registry.options_for("openai/gpt-5.6-sol", "alva").unwrap();
        let codex = registry.options_for("gpt-5.6-sol", "codex").unwrap();
        assert_eq!(
            alva.reasoning_efforts,
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            codex.reasoning_efforts,
            ["low", "medium", "high", "xhigh", "max", "ultra"]
        );
    }
}
