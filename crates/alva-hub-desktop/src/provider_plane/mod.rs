// INPUT:  serde, versioned ModelRegistry, pure RoutePlanner
// OUTPUT: ProviderModelOptions plus crate-private provider/model routing policy
// POS:    Hub provider plane boundary between connection persistence and engine-specific route projection.

mod model_registry;
mod route_planner;

use serde::{Deserialize, Serialize};

pub(crate) use model_registry::ModelRegistry;
pub(crate) use route_planner::{PlannedModelRoute, RoutePlanner, RoutePlanningInput};

/// Product-owned controls that one model route may safely expose.
///
/// This remains part of the public provider DTO even though the authority that
/// produces it lives in the backend provider plane.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModelOptions {
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
    #[serde(default)]
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub supports_fast_mode: bool,
}

impl ProviderModelOptions {
    pub(crate) fn is_empty(&self) -> bool {
        self.reasoning_efforts.is_empty()
            && self.default_reasoning_effort.is_none()
            && !self.supports_fast_mode
    }
}
