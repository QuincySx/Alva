// INPUT:  crate::{EngineRuntime, EngineInspector, RuntimeCapabilities}
// OUTPUT: EngineDescriptor + descriptor sub-types (permission plan / config surface / install metadata)
// POS:    Registration-time self-description for an engine (seam ticket 15 batch 1).
//
// The 4-method EngineRuntime trait answers "how do I run this engine". The
// descriptor answers "what can this engine do and what does it need" — the
// questions products currently answer by switch-ing on the engine NAME
// (permissions.rs `enforcement_for`, readiness `quick_veto_for_target`, etc.).
// Engines declare; products consult. No product may need to know an engine's
// name to decide whether it satisfies a permission profile.

use std::sync::Arc;

use crate::{EngineInspector, EngineRuntime};

/// How a declared permission plan relates to the product's requested profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRelation {
    /// The engine executes the exact profile requested.
    Equivalent,
    /// The engine enforces something at least as strict as the requested
    /// profile (never weaker).
    Stricter,
}

/// One engine-declared capability to satisfy a product permission profile.
///
/// The enforcement knob stays engine-private: products only see the relation
/// and a human summary, never vendor types like `CodexSandboxMode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPlanDecl {
    /// Product permission profile id this plan satisfies (e.g. "workspace-read",
    /// "workspace-write", "plan", "autonomous").
    pub profile_id: String,
    /// How this plan relates to a request for `profile_id`.
    pub relation: PermissionRelation,
    /// Human-readable enforcement summary for UI/readiness display.
    pub summary: String,
}

/// Provider protocol the engine routes through, so products stop hard-coding
/// per-engine protocol tables (route_planner.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProtocol {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
    CodexSubscription,
    ClaudeSubscription,
    /// Engine-local auth or no provider dependency.
    Native,
}

/// Request-side config the engine actually supports (ticket 15 D facts).
#[derive(Debug, Clone)]
pub struct ConfigSurface {
    /// `RuntimeRequest.max_turns` is honored by this engine.
    pub supports_max_turns: bool,
    /// Provider protocol used for model routing.
    pub provider_protocol: ProviderProtocol,
    /// The engine understands Hub's fast-mode flag.
    pub supports_fast_mode: bool,
    /// Custom HTTP headers are accepted on the provider connection.
    pub supports_custom_http_headers: bool,
}

/// Install / readiness facts, so `quick_veto_for_target` stops matching names.
#[derive(Debug, Clone, Default)]
pub struct InstallMetadata {
    /// Binary name the readiness probe should look for (e.g. "codex").
    pub required_binary: Option<String>,
    /// Short install guidance for the engine manager UI.
    pub install_hint: Option<String>,
}

/// One registered engine, with everything a product needs to enumerate,
/// vet, route, and permission it — without a single engine-name literal.
#[derive(Clone)]
pub struct EngineDescriptor {
    pub runtime: Arc<dyn EngineRuntime>,
    pub inspector: Option<Arc<dyn EngineInspector>>,
    /// Which permission profiles this engine can honor and how.
    pub permission_plans: Vec<PermissionPlanDecl>,
    /// Request-side config the engine supports.
    pub config_surface: ConfigSurface,
    /// Readiness / install facts.
    pub install_metadata: InstallMetadata,
}

/// Conservative defaults for engines that only declare their runtime: no
/// permission plans, no inspector, no install metadata, native protocol.
impl EngineDescriptor {
    pub fn from_runtime(runtime: Arc<dyn EngineRuntime>) -> Self {
        Self {
            runtime,
            inspector: None,
            permission_plans: Vec::new(),
            config_surface: ConfigSurface {
                supports_max_turns: false,
                provider_protocol: ProviderProtocol::Native,
                supports_fast_mode: false,
                supports_custom_http_headers: false,
            },
            install_metadata: InstallMetadata::default(),
        }
    }

    /// Whether this engine declares a plan that satisfies `profile_id` without
    /// weakening it (used by products instead of engine-name matches).
    pub fn can_satisfy(&self, profile_id: &str) -> bool {
        self.permission_plans.iter().any(|plan| {
            plan.profile_id == profile_id
                && matches!(plan.relation, PermissionRelation::Equivalent | PermissionRelation::Stricter)
        })
    }
}