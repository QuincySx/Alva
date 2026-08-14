// INPUT:  serde, provider_plane, explicit managed-config paths, private app-data path, alva_llm_provider, EngineTarget
// OUTPUT: ProviderCatalog, explicit model registry bindings, redacted snapshots, executable/default ModelRoute values
// POS:    Hub provider connection catalog owning backend-validated model identity, secret persistence, and route DTO projection.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use alva_llm_provider::ProviderConfig;
use serde::{Deserialize, Serialize};

pub use crate::provider_plane::ProviderModelOptions;
use crate::provider_plane::{PlannedModelRoute, RoutePlanner, RoutePlanningInput};
use crate::store::EngineTarget;

const CATALOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModel {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    /// Explicit binding to one bundled registry declaration. Missing means a
    /// custom model whose optional controls must not be guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supported_engines: Vec<String>,
    /// Product-owned, per-engine model controls. The Settings webview may
    /// round-trip this metadata but cannot grant capabilities: save() rebuilds
    /// it from the backend model catalog.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_options: BTreeMap<String, ProviderModelOptions>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderProfile {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: bool,
    pub api_key_configured: bool,
    pub requires_api_key: bool,
    pub endpoint_editable: bool,
    pub managed: bool,
    pub max_output_tokens: u32,
    pub supported_engines: Vec<String>,
    pub models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SaveProviderProfile {
    pub id: String,
    pub display_name: String,
    pub base_url: String,
    pub enabled: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
    pub max_output_tokens: u32,
    pub models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelRoute {
    pub target: EngineTarget,
    pub engine_label: String,
    pub model_label: String,
    pub provider_label: String,
    pub description: String,
    pub context_window: Option<u64>,
    pub capabilities: Vec<String>,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
    pub supports_fast_mode: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProviderProfile {
    id: String,
    display_name: String,
    kind: String,
    base_url: String,
    enabled: bool,
    api_key: String,
    #[serde(default)]
    custom_headers: HashMap<String, String>,
    requires_api_key: bool,
    endpoint_editable: bool,
    managed: bool,
    max_output_tokens: u32,
    supported_engines: Vec<String>,
    models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCatalog {
    version: u32,
    providers: Vec<StoredProviderProfile>,
    #[serde(default)]
    default_target: Option<EngineTarget>,
}

#[derive(Debug, Clone)]
struct CatalogState {
    providers: Vec<StoredProviderProfile>,
    default_target: Option<EngineTarget>,
}

#[derive(Debug, Default, Deserialize)]
struct SharedProviderConfig {
    #[serde(default)]
    providers: BTreeMap<String, ManagedProviderEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ManagedProviderEntry {
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    custom_headers: HashMap<String, String>,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Clone)]
pub struct ProviderCatalog {
    path: PathBuf,
    shared_config_path: Option<PathBuf>,
    include_legacy_config: bool,
    state: Arc<Mutex<CatalogState>>,
}

impl ProviderCatalog {
    /// Open a Hub-owned catalog without implicitly consulting the user's home directory.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        Self::open_inner(path.into(), None, false)
    }

    /// Open a catalog with one explicit, read-only shared Alva config source.
    pub fn open_with_shared_config(
        path: impl Into<PathBuf>,
        shared_config_path: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        Self::open_inner(path.into(), Some(shared_config_path.into()), false)
    }

    /// Desktop compatibility adapter: normalize legacy XDG/workspace/env
    /// precedence into one managed, read-only profile.
    pub fn open_with_shared_and_legacy_config(
        path: impl Into<PathBuf>,
        shared_config_path: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        Self::open_inner(path.into(), Some(shared_config_path.into()), true)
    }

    fn open_inner(
        path: PathBuf,
        shared_config_path: Option<PathBuf>,
        include_legacy_config: bool,
    ) -> Result<Self, String> {
        ensure_private_parent(&path)?;
        reject_symlink(&path)?;
        let (state, needs_persist) = if path.exists() {
            let contents = fs::read_to_string(&path)
                .map_err(|error| format!("read provider catalog {}: {error}", path.display()))?;
            let stored: StoredCatalog = serde_json::from_str(&contents)
                .map_err(|error| format!("decode provider catalog {}: {error}", path.display()))?;
            if stored.version > CATALOG_VERSION {
                return Err(format!(
                    "provider catalog version {} is newer than supported version {CATALOG_VERSION}",
                    stored.version
                ));
            }
            (
                CatalogState {
                    providers: merge_fixed_providers(stored.providers),
                    default_target: stored.default_target,
                },
                false,
            )
        } else {
            (
                CatalogState {
                    providers: default_providers(),
                    default_target: None,
                },
                true,
            )
        };
        let catalog = Self {
            path,
            shared_config_path,
            include_legacy_config,
            state: Arc::new(Mutex::new(state)),
        };
        if needs_persist {
            let state = catalog.lock()?.clone();
            catalog.persist_state(&state)?;
        }
        Ok(catalog)
    }

    pub fn list(&self, workspace: Option<&Path>) -> Result<Vec<ProviderProfile>, String> {
        Ok(self
            .effective_providers(workspace)?
            .iter()
            .map(public_profile)
            .collect())
    }

    pub fn save(
        &self,
        input: SaveProviderProfile,
        _workspace: Option<&Path>,
    ) -> Result<ProviderProfile, String> {
        validate_save_input(&input)?;
        let mut state = self.lock()?;
        let mut next = state.clone();
        let provider = next
            .providers
            .iter_mut()
            .find(|provider| provider.id == input.id)
            .ok_or_else(|| format!("provider '{}' was not found", input.id))?;
        if provider.managed {
            return Err(format!(
                "provider '{}' is managed by an external configuration",
                provider.id
            ));
        }
        provider.display_name = input.display_name.trim().to_string();
        provider.enabled = input.enabled;
        provider.max_output_tokens = input.max_output_tokens;
        let provider_id = provider.id.clone();
        let provider_kind = provider.kind.clone();
        provider.models = normalize_models(input.models, &provider_id, &provider_kind);
        if provider.endpoint_editable {
            provider.base_url = normalize_endpoint(&input.base_url);
        }
        if input.clear_api_key {
            provider.api_key.clear();
        } else if let Some(api_key) = input.api_key.filter(|key| !key.trim().is_empty()) {
            provider.api_key = api_key.trim().to_string();
        }
        let result = public_profile(provider);
        self.persist_state(&next)?;
        *state = next;
        Ok(result)
    }

    pub fn routes(&self, workspace: Option<&Path>) -> Result<Vec<ModelRoute>, String> {
        routes_for_providers(&self.effective_providers(workspace)?)
    }

    /// Return the normalized availability facts for one exact route identity.
    pub fn route_for_target(
        &self,
        target: &EngineTarget,
        workspace: Option<&Path>,
    ) -> Result<Option<ModelRoute>, String> {
        let route = self
            .routes(workspace)?
            .into_iter()
            .find(|route| same_route_identity(&route.target, target));
        route
            .map(|route| apply_requested_options(route, target))
            .transpose()
    }

    /// Return the explicit global default route without substituting another route.
    /// A configured-but-disabled route is returned with `available = false`.
    pub fn default_target(&self, workspace: Option<&Path>) -> Result<Option<ModelRoute>, String> {
        let default_target = self.lock()?.default_target.clone();
        let Some(default_target) = default_target else {
            return Ok(None);
        };
        let route = self.route_for_target(&default_target, workspace)?;
        Ok(Some(
            route.unwrap_or_else(|| missing_default_route(default_target)),
        ))
    }

    /// Persist a new global default only when that exact route is currently executable.
    pub fn set_default_target(
        &self,
        target: EngineTarget,
        workspace: Option<&Path>,
    ) -> Result<ModelRoute, String> {
        let route = self
            .route_for_target(&target, workspace)?
            .ok_or_else(|| route_not_configured_error(&target))?;
        if !route.available {
            return Err(route
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| "the selected model route is unavailable".into()));
        }
        let mut state = self.lock()?;
        let mut next = state.clone();
        next.default_target = Some(route.target.clone());
        self.persist_state(&next)?;
        *state = next;
        Ok(route)
    }

    /// Validate a UI-selected route and return the direct-provider config
    /// needed by its runtime adapter. Native CLI subscription routes return
    /// `None`; API-backed ALVA, Claude Code, Codex, and Pi routes return the
    /// same normalized provider configuration.
    pub fn resolve_target(
        &self,
        target: &EngineTarget,
        workspace: &Path,
    ) -> Result<Option<ProviderConfig>, String> {
        self.resolve_target_at(target, Some(workspace))
    }

    /// Whether one exact route consumes a configured third-party API rather
    /// than the runtime's own native subscription. This is the readiness seam
    /// used by the Composer and fallback policy.
    pub fn target_uses_api(
        &self,
        target: &EngineTarget,
        workspace: Option<&Path>,
    ) -> Result<bool, String> {
        Ok(self.resolve_target_at(target, workspace)?.is_some())
    }

    fn resolve_target_at(
        &self,
        target: &EngineTarget,
        workspace: Option<&Path>,
    ) -> Result<Option<ProviderConfig>, String> {
        let providers = self.effective_providers(workspace)?;
        let provider = providers
            .iter()
            .find(|provider| provider.id == target.provider_id)
            .ok_or_else(|| format!("provider '{}' is not configured", target.provider_id))?;
        let model = provider
            .models
            .iter()
            .find(|model| model.id == target.model_id)
            .ok_or_else(|| {
                format!(
                    "model '{}' is not configured for provider '{}'",
                    target.model_id, provider.display_name
                )
            })?;
        let engines = model_engines(provider, model);
        if !engines.iter().any(|engine| engine == &target.engine_id) {
            return Err(format!(
                "provider '{}' model '{}' does not support the {} engine",
                provider.display_name, model.display_name, target.engine_id
            ));
        }
        let (available, reason) = route_availability(provider, model, &target.engine_id);
        if !available {
            return Err(reason.unwrap_or_else(|| "the selected model route is unavailable".into()));
        }
        if is_native_subscription(&provider.kind) {
            return Ok(None);
        }
        Ok(Some(ProviderConfig {
            api_key: provider.api_key.clone(),
            model: model.id.clone(),
            base_url: provider.base_url.clone(),
            max_tokens: provider.max_output_tokens,
            custom_headers: provider.custom_headers.clone(),
            kind: Some(provider.kind.clone()),
        }))
    }

    /// Resolve one API provider for backend-only discovery and diagnostics.
    /// Credentials remain inside Rust and are never projected into the web DTOs.
    pub fn resolve_provider_for_diagnostics(
        &self,
        provider_id: &str,
        workspace: Option<&Path>,
    ) -> Result<ProviderConfig, String> {
        let providers = self.effective_providers(workspace)?;
        let provider = providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| format!("provider '{provider_id}' is not configured"))?;
        if !provider.enabled {
            return Err(format!(
                "provider '{}' is disabled in Settings",
                provider.display_name
            ));
        }
        if !is_supported_provider_kind(&provider.kind) {
            return Err(format!(
                "provider '{}' does not support API model discovery",
                provider.display_name
            ));
        }
        if provider.requires_api_key && !has_credentials(provider) {
            return Err(format!(
                "add credentials for '{}' in Settings before discovering models",
                provider.display_name
            ));
        }
        if provider.base_url.trim().is_empty() {
            return Err(format!(
                "add an API endpoint for '{}' before discovering models",
                provider.display_name
            ));
        }
        validate_endpoint(&provider.base_url)?;
        let model = provider
            .models
            .iter()
            .find(|model| model.enabled && !model.id.trim().is_empty())
            .map(|model| model.id.clone())
            .unwrap_or_else(|| default_model_id(&provider.kind).into());
        Ok(ProviderConfig {
            api_key: provider.api_key.clone(),
            model,
            base_url: provider.base_url.clone(),
            max_tokens: provider.max_output_tokens,
            custom_headers: provider.custom_headers.clone(),
            kind: Some(provider.kind.clone()),
        })
    }

    fn effective_providers(
        &self,
        workspace: Option<&Path>,
    ) -> Result<Vec<StoredProviderProfile>, String> {
        let mut providers = Vec::new();
        if self.include_legacy_config {
            if let Some(workspace) = workspace {
                providers.push(legacy_resolved_provider(workspace)?);
            } else {
                providers.push(workspace_provider(None)?);
            }
        } else {
            providers.push(workspace_provider(workspace)?);
        }
        providers.extend(self.lock()?.providers.clone());
        if let Some(path) = &self.shared_config_path {
            providers.extend(shared_providers(path)?);
        }
        Ok(providers)
    }

    fn persist_state(&self, state: &CatalogState) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&StoredCatalog {
            version: CATALOG_VERSION,
            providers: state.providers.clone(),
            default_target: state.default_target.clone(),
        })
        .map_err(|error| format!("encode provider catalog: {error}"))?;
        write_private_file(&self.path, json.as_bytes())
    }

    fn lock(&self) -> Result<MutexGuard<'_, CatalogState>, String> {
        self.state
            .lock()
            .map_err(|_| "provider catalog lock was poisoned".to_string())
    }
}

fn public_profile(provider: &StoredProviderProfile) -> ProviderProfile {
    let supported_engines = provider_engines(provider);
    let models = provider
        .models
        .iter()
        .cloned()
        .map(|mut model| {
            model.registry_id = RoutePlanner::bundled().normalize_registry_binding(
                &provider.id,
                &model.id,
                model.registry_id.as_deref(),
            );
            let routes = planned_model_routes(provider, &model);
            model.runtime_options = routes
                .iter()
                .filter_map(|route| {
                    (!route.options.is_empty())
                        .then(|| (route.engine_id.clone(), route.options.clone()))
                })
                .collect();
            model.supported_engines = routes.into_iter().map(|route| route.engine_id).collect();
            model
        })
        .collect();
    ProviderProfile {
        id: provider.id.clone(),
        display_name: provider.display_name.clone(),
        kind: provider.kind.clone(),
        base_url: public_endpoint(&provider.base_url),
        enabled: provider.enabled,
        api_key_configured: has_credentials(provider),
        requires_api_key: provider.requires_api_key,
        endpoint_editable: provider.endpoint_editable,
        managed: provider.managed,
        max_output_tokens: provider.max_output_tokens,
        supported_engines,
        models,
    }
}

fn routes_for_providers(providers: &[StoredProviderProfile]) -> Result<Vec<ModelRoute>, String> {
    let mut routes = Vec::new();
    for provider in providers {
        for model in &provider.models {
            for planned in planned_model_routes(provider, model) {
                let engine_id = planned.engine_id;
                let (available, unavailable_reason) =
                    route_availability(provider, model, &engine_id);
                let options = planned.options;
                routes.push(ModelRoute {
                    target: route_target(provider, model, &engine_id, &options)?,
                    engine_label: engine_label(&engine_id).to_string(),
                    model_label: model.display_name.clone(),
                    provider_label: provider.display_name.clone(),
                    description: route_description(provider, model),
                    context_window: model.context_window,
                    capabilities: model.capabilities.clone(),
                    reasoning_efforts: options.reasoning_efforts,
                    default_reasoning_effort: options.default_reasoning_effort,
                    supports_fast_mode: options.supports_fast_mode,
                    available,
                    unavailable_reason,
                });
            }
        }
    }
    Ok(routes)
}

fn provider_engines(provider: &StoredProviderProfile) -> Vec<String> {
    RoutePlanner::bundled().provider_engines(&provider.kind, &provider.supported_engines)
}

fn model_engines(provider: &StoredProviderProfile, model: &ProviderModel) -> Vec<String> {
    planned_model_routes(provider, model)
        .into_iter()
        .map(|route| route.engine_id)
        .collect()
}

fn planned_model_routes(
    provider: &StoredProviderProfile,
    model: &ProviderModel,
) -> Vec<PlannedModelRoute> {
    RoutePlanner::bundled().plan_model(RoutePlanningInput {
        provider_id: &provider.id,
        provider_kind: &provider.kind,
        provider_supported_engines: &provider.supported_engines,
        model_id: &model.id,
        model_registry_id: model.registry_id.as_deref(),
        model_supported_engines: &model.supported_engines,
        runtime_options: Some(&model.runtime_options),
    })
}

fn route_availability(
    provider: &StoredProviderProfile,
    model: &ProviderModel,
    engine_id: &str,
) -> (bool, Option<String>) {
    if provider.id == "workspace-config" && !provider.enabled {
        return (
            false,
            Some("Choose a workspace with an Alva provider configuration.".into()),
        );
    }
    if !provider.enabled {
        return (false, Some("This provider is disabled in Settings.".into()));
    }
    if !model.enabled {
        return (false, Some("This model is disabled in Settings.".into()));
    }
    if model.id.trim().is_empty() {
        return (
            false,
            Some("Add a model ID before using this route.".into()),
        );
    }
    if !RoutePlanner::bundled().supports_engine(&provider.kind, engine_id) {
        return (
            false,
            Some(format!(
                "Provider kind '{}' is not supported by the {} engine.",
                provider.kind,
                engine_label(engine_id)
            )),
        );
    }
    if provider.requires_api_key && !has_credentials(provider) {
        return (
            false,
            Some("Add an API key in Settings to use this provider.".into()),
        );
    }
    if is_supported_provider_kind(&provider.kind) {
        if provider.base_url.trim().is_empty() {
            return (false, Some("Add an API endpoint in Settings.".into()));
        }
        if let Err(reason) = validate_endpoint(&provider.base_url) {
            return (false, Some(reason));
        }
        if engine_id != "alva" && !provider.custom_headers.is_empty() {
            return (
                false,
                Some(format!(
                    "{} cannot safely forward this provider's custom authentication headers yet.",
                    engine_label(engine_id)
                )),
            );
        }
    }
    (true, None)
}

fn has_credentials(provider: &StoredProviderProfile) -> bool {
    !provider.api_key.trim().is_empty() || !provider.custom_headers.is_empty()
}

fn route_target(
    provider: &StoredProviderProfile,
    model: &ProviderModel,
    engine_id: &str,
    options: &ProviderModelOptions,
) -> Result<EngineTarget, String> {
    let mut target = EngineTarget::for_engine(engine_id)?;
    target.model_id = model.id.clone();
    target.provider_id = provider.id.clone();
    target.account_source = match provider.kind.as_str() {
        "claude-subscription" => "Claude Code subscription".into(),
        "codex-subscription" => "Codex subscription".into(),
        _ if provider.id == "workspace-config" => "Workspace provider configuration".into(),
        _ if provider.id.starts_with("shared:") => {
            format!("Shared Alva profile '{}'", provider.display_name)
        }
        _ => format!("{} API key", provider.display_name),
    };
    target.billing_route = match provider.kind.as_str() {
        "claude-subscription" => "Anthropic account via Claude Code".into(),
        "codex-subscription" => "OpenAI account via Codex".into(),
        _ if provider.id == "workspace-config" => "Workspace provider account".into(),
        _ if provider.id.starts_with("shared:") => {
            format!("Shared {} provider account", provider.display_name)
        }
        _ => format!("{} API usage", provider.display_name),
    };
    target.required_capabilities = model.capabilities.clone();
    target.reasoning_effort = options.default_reasoning_effort.clone();
    Ok(target)
}

fn apply_requested_options(
    mut route: ModelRoute,
    requested: &EngineTarget,
) -> Result<ModelRoute, String> {
    let reasoning_effort = requested
        .reasoning_effort
        .clone()
        .or_else(|| route.default_reasoning_effort.clone());
    if reasoning_effort.as_ref().is_some_and(|effort| {
        !route
            .reasoning_efforts
            .iter()
            .any(|candidate| candidate == effort)
    }) {
        return Err(format!(
            "reasoning effort '{}' is not supported by route '{}/{}/{}'",
            reasoning_effort.as_deref().unwrap_or_default(),
            requested.engine_id,
            requested.provider_id,
            requested.model_id
        ));
    }
    if requested.fast_mode && !route.supports_fast_mode {
        return Err(format!(
            "Fast mode is not supported by route '{}/{}/{}'",
            requested.engine_id, requested.provider_id, requested.model_id
        ));
    }
    route.target.reasoning_effort = reasoning_effort;
    route.target.fast_mode = requested.fast_mode;
    Ok(route)
}

fn route_description(provider: &StoredProviderProfile, model: &ProviderModel) -> String {
    match provider.kind.as_str() {
        "claude-subscription" | "codex-subscription" => {
            "Uses the selected model through your local CLI subscription.".into()
        }
        _ if provider.id == "workspace-config" => {
            "Uses the provider configuration resolved for this workspace.".into()
        }
        _ if provider.id.starts_with("shared:") => {
            "Uses a read-only profile from the shared Alva configuration.".into()
        }
        _ => format!(
            "{} through the {} API.",
            model.display_name, provider.display_name
        ),
    }
}

fn engine_label(engine_id: &str) -> &'static str {
    match engine_id {
        "alva" => "Alva",
        "claude" => "Claude Code",
        "codex" => "Codex",
        "pi" => "Pi",
        _ => "Unknown",
    }
}

fn validate_save_input(input: &SaveProviderProfile) -> Result<(), String> {
    if input.display_name.trim().is_empty() {
        return Err("provider name must not be empty".into());
    }
    if input.max_output_tokens == 0 {
        return Err("max output tokens must be greater than zero".into());
    }
    if !input.base_url.trim().is_empty() {
        validate_endpoint(input.base_url.trim())?;
    }
    let mut model_ids = HashSet::new();
    for model in &input.models {
        let id = model.id.trim();
        if id.is_empty() || model.display_name.trim().is_empty() {
            return Err("every provider model requires an id and display name".into());
        }
        if !model_ids.insert(id) {
            return Err(format!("provider model id '{id}' is duplicated"));
        }
        if model
            .supported_engines
            .iter()
            .any(|engine| !matches!(engine.as_str(), "alva" | "claude" | "codex" | "pi"))
        {
            return Err(format!("model '{}' names an unsupported engine", model.id));
        }
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    let endpoint = endpoint.trim();
    if endpoint.chars().any(char::is_whitespace) {
        return Err("provider endpoint must not contain whitespace".into());
    }
    if endpoint.contains(['?', '#']) {
        return Err("provider endpoint must not contain a query or fragment".into());
    }
    if let Some(authority) = endpoint.strip_prefix("https://") {
        if endpoint_host(authority).is_some() {
            return Ok(());
        }
        return Err("provider HTTPS endpoint requires a host".into());
    }
    if let Some(authority) = endpoint.strip_prefix("http://") {
        let host = endpoint_host(authority)
            .ok_or_else(|| "provider HTTP endpoint requires a host".to_string())?;
        if is_loopback_host(host) {
            return Ok(());
        }
        return Err("provider endpoint must use HTTPS unless it targets loopback".into());
    }
    Err("provider endpoint must use HTTPS or loopback HTTP".into())
}

fn public_endpoint(endpoint: &str) -> String {
    let authority = endpoint
        .split_once("://")
        .map(|(_, remainder)| remainder)
        .unwrap_or(endpoint)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        String::new()
    } else {
        endpoint
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

fn endpoint_host(authority_and_path: &str) -> Option<&str> {
    let authority = authority_and_path
        .split(['/', '?', '#'])
        .next()
        .filter(|authority| !authority.is_empty() && !authority.contains('@'))?;
    if authority.starts_with('[') {
        return authority
            .strip_prefix('[')?
            .split_once(']')
            .map(|(host, _)| host)
            .filter(|host| !host.is_empty());
    }
    authority
        .split_once(':')
        .map(|(host, _)| host)
        .or(Some(authority))
        .filter(|host| !host.is_empty())
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim().trim_end_matches('/').to_string()
}

fn normalize_models(
    models: Vec<ProviderModel>,
    provider_id: &str,
    provider_kind: &str,
) -> Vec<ProviderModel> {
    let planner = RoutePlanner::bundled();
    models
        .into_iter()
        .map(|mut model| {
            model.id = model.id.trim().to_string();
            model.display_name = model.display_name.trim().to_string();
            model.registry_id = planner.normalize_registry_binding(
                provider_id,
                &model.id,
                model.registry_id.as_deref(),
            );
            model.runtime_options = planner
                .plan_model(RoutePlanningInput {
                    provider_id,
                    provider_kind,
                    provider_supported_engines: &model.supported_engines,
                    model_id: &model.id,
                    model_registry_id: model.registry_id.as_deref(),
                    model_supported_engines: &model.supported_engines,
                    runtime_options: None,
                })
                .into_iter()
                .filter_map(|route| {
                    (!route.options.is_empty()).then(|| (route.engine_id, route.options))
                })
                .collect();
            model
        })
        .collect()
}

fn is_supported_provider_kind(kind: &str) -> bool {
    matches!(
        kind,
        "anthropic" | "openai-chat" | "openai-responses" | "gemini"
    )
}

fn is_native_subscription(kind: &str) -> bool {
    matches!(kind, "claude-subscription" | "codex-subscription")
}

fn same_route_identity(left: &EngineTarget, right: &EngineTarget) -> bool {
    left.engine_id == right.engine_id
        && left.model_id == right.model_id
        && left.provider_id == right.provider_id
}

fn route_not_configured_error(target: &EngineTarget) -> String {
    format!(
        "route '{}/{}/{}' is not configured",
        target.engine_id, target.provider_id, target.model_id
    )
}

fn missing_default_route(target: EngineTarget) -> ModelRoute {
    let engine_label = engine_label(&target.engine_id).to_string();
    ModelRoute {
        engine_label,
        model_label: target.model_id.clone(),
        provider_label: target.provider_id.clone(),
        description: "The configured default route no longer exists in the provider catalog."
            .into(),
        context_window: None,
        capabilities: target.required_capabilities.clone(),
        reasoning_efforts: Vec::new(),
        default_reasoning_effort: target.reasoning_effort.clone(),
        supports_fast_mode: false,
        available: false,
        unavailable_reason: Some(format!(
            "Default route '{}/{}/{}' is no longer configured. Choose a new default in Settings.",
            target.engine_id, target.provider_id, target.model_id
        )),
        target,
    }
}

fn workspace_provider(workspace: Option<&Path>) -> Result<StoredProviderProfile, String> {
    let entry = workspace
        .map(|workspace| read_managed_entry(&workspace.join(".alva").join("config.json")))
        .transpose()?
        .flatten();
    Ok(managed_provider(
        "workspace-config".into(),
        "Workspace provider".into(),
        "workspace",
        entry,
    ))
}

fn legacy_resolved_provider(workspace: &Path) -> Result<StoredProviderProfile, String> {
    let config = ProviderConfig::load(workspace)?;
    let kind = config.kind.clone().unwrap_or_else(|| "openai-chat".into());
    Ok(managed_provider(
        "workspace-config".into(),
        "Resolved Alva configuration".into(),
        "legacy",
        Some(ManagedProviderEntry {
            api_key: config.api_key,
            model: Some(config.model),
            base_url: Some(config.base_url),
            max_tokens: Some(config.max_tokens),
            custom_headers: config.custom_headers,
            kind: Some(kind),
        }),
    ))
}

fn shared_providers(path: &Path) -> Result<Vec<StoredProviderProfile>, String> {
    let Some(contents) = read_optional(path)? else {
        return Ok(Vec::new());
    };
    let config: SharedProviderConfig = serde_json::from_str(&contents)
        .map_err(|error| format!("decode shared provider config {}: {error}", path.display()))?;
    Ok(config
        .providers
        .into_iter()
        .map(|(name, entry)| {
            let kind = entry.kind.clone().unwrap_or_else(|| name.clone());
            managed_provider(
                format!("shared:{name}"),
                name,
                "shared",
                Some(ManagedProviderEntry {
                    kind: Some(kind),
                    ..entry
                }),
            )
        })
        .collect())
}

fn read_managed_entry(path: &Path) -> Result<Option<ManagedProviderEntry>, String> {
    let Some(contents) = read_optional(path)? else {
        return Ok(None);
    };
    serde_json::from_str(&contents).map(Some).map_err(|error| {
        format!(
            "decode workspace provider config {}: {error}",
            path.display()
        )
    })
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "read managed provider config {}: {error}",
            path.display()
        )),
    }
}

fn managed_provider(
    id: String,
    display_name: String,
    source_kind: &str,
    entry: Option<ManagedProviderEntry>,
) -> StoredProviderProfile {
    let configured = entry.is_some();
    let entry = entry.unwrap_or_default();
    let kind = entry.kind.unwrap_or_else(|| "openai-chat".into());
    let model_id = entry.model.unwrap_or_else(|| {
        if source_kind == "workspace" {
            "workspace-config".into()
        } else {
            default_model_id(&kind).into()
        }
    });
    let base_url = entry
        .base_url
        .unwrap_or_else(|| alva_llm_provider::default_base_url(Some(&kind)).into());
    StoredProviderProfile {
        id,
        display_name,
        kind,
        base_url,
        enabled: configured,
        api_key: entry.api_key,
        custom_headers: entry.custom_headers,
        requires_api_key: true,
        endpoint_editable: false,
        managed: true,
        max_output_tokens: entry.max_tokens.unwrap_or(8192),
        supported_engines: vec!["alva".into()],
        models: vec![default_model(&model_id, &model_id, "alva")],
    }
}

fn merge_fixed_providers(stored: Vec<StoredProviderProfile>) -> Vec<StoredProviderProfile> {
    let mut merged = default_providers();
    for provider in stored.into_iter().filter(|provider| !provider.managed) {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.id == provider.id)
        {
            *existing = provider;
        } else {
            // Keep named profiles created by newer schema versions even though V1 has no CRUD.
            merged.push(provider);
        }
    }
    merged
}

fn default_providers() -> Vec<StoredProviderProfile> {
    vec![
        provider(
            "claude-cli",
            "Claude Code",
            "claude-subscription",
            "",
            false,
            false,
            &["claude"],
            vec![
                default_model("engine-default", "Default model", "claude"),
                default_model("claude-opus-4-7", "Claude Opus 4.7", "claude"),
            ],
        ),
        provider(
            "codex-cli",
            "Codex",
            "codex-subscription",
            "",
            false,
            false,
            &["codex"],
            vec![
                default_model("engine-default", "Default model", "codex"),
                default_model("gpt-5.6-sol", "GPT-5.6-Sol", "codex"),
            ],
        ),
        provider(
            "anthropic",
            "Anthropic",
            "anthropic",
            alva_llm_provider::default_base_url(Some("anthropic")),
            true,
            false,
            &["alva"],
            vec![default_model("claude-opus-4-7", "Claude Opus 4.7", "alva")],
        ),
        provider(
            "openai-responses",
            "OpenAI Responses",
            "openai-responses",
            alva_llm_provider::default_base_url(Some("openai-responses")),
            true,
            false,
            &["alva"],
            vec![default_model("gpt-5.4", "GPT-5.4", "alva")],
        ),
        provider(
            "openai-compatible",
            "OpenAI compatible",
            "openai-chat",
            alva_llm_provider::default_base_url(Some("openai-chat")),
            true,
            true,
            &["alva"],
            vec![default_model("gpt-5.4", "GPT-5.4", "alva")],
        ),
        provider(
            "gemini",
            "Google Gemini",
            "gemini",
            alva_llm_provider::default_base_url(Some("gemini")),
            true,
            false,
            &["alva"],
            vec![default_model(
                "gemini-3.6-flash",
                "Gemini 3.6 Flash",
                "alva",
            )],
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn provider(
    id: &str,
    display_name: &str,
    kind: &str,
    base_url: &str,
    requires_api_key: bool,
    endpoint_editable: bool,
    supported_engines: &[&str],
    models: Vec<ProviderModel>,
) -> StoredProviderProfile {
    let managed = matches!(kind, "claude-subscription" | "codex-subscription");
    StoredProviderProfile {
        id: id.into(),
        display_name: display_name.into(),
        kind: kind.into(),
        base_url: base_url.into(),
        enabled: true,
        api_key: String::new(),
        custom_headers: HashMap::new(),
        requires_api_key,
        endpoint_editable,
        managed,
        max_output_tokens: 8192,
        supported_engines: supported_engines
            .iter()
            .map(|engine| (*engine).into())
            .collect(),
        models,
    }
}

fn default_model_id(kind: &str) -> &'static str {
    match kind {
        "anthropic" => "claude-opus-4-7",
        "gemini" => "gemini-3.6-flash",
        _ => "gpt-5.4",
    }
}

fn default_model(id: &str, display_name: &str, engine: &str) -> ProviderModel {
    ProviderModel {
        id: id.into(),
        display_name: display_name.into(),
        enabled: true,
        registry_id: RoutePlanner::bundled()
            .preview_discovered_model("", id)
            .suggested_registry_id,
        context_window: None,
        capabilities: Vec::new(),
        supported_engines: vec![engine.into()],
        runtime_options: BTreeMap::new(),
    }
}

fn ensure_private_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("provider catalog path {} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "create provider catalog directory {}: {error}",
            parent.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "secure provider catalog directory {}: {error}",
                parent.display()
            )
        })?;
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "provider catalog {} must not be a symbolic link",
            path.display()
        )),
        Ok(_) | Err(_) if !path.exists() => Ok(()),
        Ok(_) => Ok(()),
        Err(error) => Err(format!(
            "inspect provider catalog {}: {error}",
            path.display()
        )),
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("provider catalog path {} has no parent", path.display()))?;
    let temp = parent.join(format!(".providers.{}.tmp", std::process::id()));
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|error| format!("create provider catalog temp file: {error}"))?
    };
    #[cfg(not(unix))]
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|error| format!("create provider catalog temp file: {error}"))?;
    if let Err(error) = file.write_all(contents) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(format!("write provider catalog: {error}"));
    }
    if let Err(error) = file.sync_all() {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(format!("sync provider catalog: {error}"));
    }
    drop(file);
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("replace provider catalog {}: {error}", path.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved_anthropic(key: Option<&str>, model_id: &str) -> SaveProviderProfile {
        SaveProviderProfile {
            id: "anthropic".into(),
            display_name: "Anthropic direct".into(),
            base_url: "https://api.anthropic.com/".into(),
            enabled: true,
            api_key: key.map(str::to_string),
            clear_api_key: false,
            max_output_tokens: 4096,
            models: vec![ProviderModel {
                id: model_id.into(),
                display_name: "Test model".into(),
                enabled: true,
                registry_id: None,
                context_window: Some(100_000),
                capabilities: vec!["vision".into()],
                supported_engines: vec!["alva".into()],
                runtime_options: BTreeMap::new(),
            }],
        }
    }

    #[test]
    fn catalog_redacts_keys_reopens_and_resolves_saved_models() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        let catalog = ProviderCatalog::open(&path).unwrap();
        let saved = catalog
            .save(
                saved_anthropic(Some("synthetic-secret"), "test-model"),
                None,
            )
            .unwrap();
        assert!(saved.api_key_configured);
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("synthetic-secret"));

        let reopened = ProviderCatalog::open(&path).unwrap();
        let route = reopened
            .routes(None)
            .unwrap()
            .into_iter()
            .find(|route| route.target.model_id == "test-model")
            .unwrap();
        assert!(route.available);
        assert_eq!(route.target.provider_id, "anthropic");
        let config = reopened
            .resolve_target(&route.target, directory.path())
            .unwrap()
            .unwrap();
        assert_eq!(config.api_key, "synthetic-secret");
        assert_eq!(config.model, "test-model");
    }

    #[test]
    fn incomplete_draft_saves_but_route_explains_unavailability() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        catalog
            .save(saved_anthropic(None, "draft-model"), None)
            .unwrap();
        let route = catalog
            .routes(None)
            .unwrap()
            .into_iter()
            .find(|route| route.target.model_id == "draft-model")
            .unwrap();
        assert!(!route.available);
        assert!(route.unavailable_reason.unwrap().contains("API key"));
    }

    #[test]
    fn one_anthropic_model_fans_out_to_every_compatible_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        let saved = catalog
            .save(
                saved_anthropic(Some("synthetic-key"), "shared-claude-model"),
                None,
            )
            .unwrap();

        assert_eq!(saved.supported_engines, ["alva", "claude", "pi"]);
        assert_eq!(saved.models[0].supported_engines, ["alva", "claude", "pi"]);
        let routes: Vec<_> = catalog
            .routes(None)
            .unwrap()
            .into_iter()
            .filter(|route| route.target.model_id == "shared-claude-model")
            .collect();
        assert_eq!(
            routes
                .iter()
                .map(|route| route.target.engine_id.as_str())
                .collect::<Vec<_>>(),
            ["alva", "claude", "pi"]
        );
        assert!(routes.iter().all(|route| route.available));
        for route in routes {
            let config = catalog
                .resolve_target(&route.target, directory.path())
                .unwrap()
                .expect("direct API routes must resolve provider configuration");
            assert_eq!(config.kind.as_deref(), Some("anthropic"));
            assert_eq!(config.model, "shared-claude-model");
        }
    }

    #[test]
    fn subscription_routes_remain_private_to_their_cli_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        for provider_id in ["claude-cli", "codex-cli"] {
            let route = catalog
                .routes(None)
                .unwrap()
                .into_iter()
                .find(|route| route.target.provider_id == provider_id)
                .unwrap();
            assert!(!catalog.target_uses_api(&route.target, None).unwrap());
        }
    }

    #[test]
    fn route_options_are_runtime_specific_and_requested_values_are_validated() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        let routes = catalog.routes(None).unwrap();
        let codex_default = routes
            .iter()
            .find(|route| {
                route.target.provider_id == "codex-cli" && route.target.model_id == "engine-default"
            })
            .unwrap();
        assert!(codex_default.reasoning_efforts.is_empty());
        assert_eq!(codex_default.default_reasoning_effort, None);
        assert!(!codex_default.supports_fast_mode);

        let codex_sol = routes
            .iter()
            .find(|route| {
                route.target.provider_id == "codex-cli" && route.target.model_id == "gpt-5.6-sol"
            })
            .unwrap();
        assert_eq!(
            codex_sol.reasoning_efforts,
            ["low", "medium", "high", "xhigh", "max", "ultra"]
        );
        assert_eq!(codex_sol.default_reasoning_effort.as_deref(), Some("high"));
        assert!(codex_sol.supports_fast_mode);

        let mut requested = codex_sol.target.clone();
        requested.reasoning_effort = Some("ultra".into());
        requested.fast_mode = true;
        let resolved = catalog.route_for_target(&requested, None).unwrap().unwrap();
        assert_eq!(resolved.target.reasoning_effort.as_deref(), Some("ultra"));
        assert!(resolved.target.fast_mode);

        requested.reasoning_effort = Some("minimal".into());
        assert!(catalog
            .route_for_target(&requested, None)
            .unwrap_err()
            .contains("reasoning effort"));

        let mut input = saved_anthropic(Some("synthetic"), "gpt-5.6-sol");
        input.id = "openai-responses".into();
        catalog.save(input, None).unwrap();
        let direct_routes: Vec<_> = catalog
            .routes(None)
            .unwrap()
            .into_iter()
            .filter(|route| {
                route.target.provider_id == "openai-responses"
                    && route.target.model_id == "gpt-5.6-sol"
            })
            .collect();
        let direct_codex = direct_routes
            .iter()
            .find(|route| route.target.engine_id == "codex")
            .unwrap();
        assert!(direct_codex.supports_fast_mode);
        assert!(direct_codex.reasoning_efforts.contains(&"ultra".into()));
        for engine_id in ["alva", "pi"] {
            let route = direct_routes
                .iter()
                .find(|route| route.target.engine_id == engine_id)
                .unwrap();
            assert!(!route.supports_fast_mode);
            assert!(!route.reasoning_efforts.contains(&"ultra".into()));
        }

        let claude_default = routes
            .iter()
            .find(|route| {
                route.target.provider_id == "claude-cli"
                    && route.target.model_id == "engine-default"
            })
            .unwrap();
        assert!(claude_default.reasoning_efforts.is_empty());
        assert!(!claude_default.supports_fast_mode);
        let claude_opus = routes
            .iter()
            .find(|route| {
                route.target.provider_id == "claude-cli"
                    && route.target.model_id == "claude-opus-4-7"
            })
            .unwrap();
        assert_eq!(
            claude_opus.reasoning_efforts,
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert!(!claude_opus.supports_fast_mode);

        let mut unsupported_fast = claude_opus.target.clone();
        unsupported_fast.fast_mode = true;
        assert!(catalog
            .route_for_target(&unsupported_fast, None)
            .unwrap_err()
            .contains("Fast mode"));
    }

    #[test]
    fn model_registry_binding_is_explicit_and_backend_validated() {
        let known = ProviderModel {
            id: "openai/gpt-5.6-sol".into(),
            display_name: "Known".into(),
            enabled: true,
            registry_id: Some("gpt-5.6-sol".into()),
            context_window: None,
            capabilities: Vec::new(),
            supported_engines: Vec::new(),
            runtime_options: BTreeMap::new(),
        };
        let bound = normalize_models(vec![known.clone()], "custom", "openai-responses");
        assert_eq!(bound[0].registry_id.as_deref(), Some("gpt-5.6-sol"));
        assert!(bound[0]
            .runtime_options
            .get("codex")
            .is_some_and(|options| options.supports_fast_mode));

        let mut forged = known;
        forged.registry_id = Some("claude-opus-4-7".into());
        let unbound = normalize_models(vec![forged], "custom", "openai-responses");
        assert_eq!(unbound[0].registry_id, None);
        assert!(unbound[0].runtime_options.is_empty());

        let custom = default_model("vendor-future-model", "Future", "alva");
        assert_eq!(custom.registry_id, None);
    }

    #[test]
    fn diagnostics_resolver_keeps_credentials_backend_only() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        let missing = catalog
            .resolve_provider_for_diagnostics("anthropic", Some(directory.path()))
            .unwrap_err();
        assert!(missing.contains("credentials"));

        catalog
            .save(
                saved_anthropic(Some("synthetic-diagnostic-key"), "diagnostic-model"),
                None,
            )
            .unwrap();
        let config = catalog
            .resolve_provider_for_diagnostics("anthropic", Some(directory.path()))
            .unwrap();
        assert_eq!(config.api_key, "synthetic-diagnostic-key");
        assert_eq!(config.model, "diagnostic-model");
        assert!(catalog
            .resolve_provider_for_diagnostics("claude-cli", Some(directory.path()))
            .unwrap_err()
            .contains("does not support API model discovery"));
    }

    #[test]
    fn duplicate_model_ids_are_rejected_after_trimming() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        let mut input = saved_anthropic(Some("synthetic"), "same-model");
        input
            .models
            .push(default_model(" same-model ", "Duplicate", "alva"));
        let error = catalog.save(input, None).unwrap_err();
        assert!(error.contains("duplicated"));
    }

    #[test]
    fn endpoints_require_https_except_for_loopback_http() {
        assert!(validate_endpoint("https://models.example.test/v1").is_ok());
        assert!(validate_endpoint("http://localhost:11434/v1").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8080/v1").is_ok());
        assert!(validate_endpoint("http://[::1]:8080/v1").is_ok());
        assert!(validate_endpoint("http://models.example.test/v1").is_err());
        assert!(validate_endpoint("models.example.test/v1").is_err());
        assert!(validate_endpoint("https://models.example.test/v1?api_key=synthetic").is_err());
        assert!(validate_endpoint("https://models.example.test/v1#synthetic").is_err());
        assert_eq!(
            public_endpoint("https://synthetic:secret@example.test/v1"),
            ""
        );
        assert_eq!(
            public_endpoint("https://models.example.test/v1?api_key=synthetic"),
            "https://models.example.test/v1"
        );
    }

    #[test]
    fn failed_persist_does_not_advance_memory_or_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        let catalog = ProviderCatalog::open(&path).unwrap();
        let temp_collision = directory
            .path()
            .join(format!(".providers.{}.tmp", std::process::id()));
        fs::create_dir(&temp_collision).unwrap();

        let mut input = saved_anthropic(Some("synthetic"), "new-model");
        input.display_name = "Must not leak into memory".into();
        assert!(catalog.save(input, None).is_err());
        let profile = catalog
            .list(None)
            .unwrap()
            .into_iter()
            .find(|provider| provider.id == "anthropic")
            .unwrap();
        assert_eq!(profile.display_name, "Anthropic");

        let reopened = ProviderCatalog::open(&path).unwrap();
        let reopened_profile = reopened
            .list(None)
            .unwrap()
            .into_iter()
            .find(|provider| provider.id == "anthropic")
            .unwrap();
        assert_eq!(reopened_profile.display_name, "Anthropic");
    }

    #[test]
    fn managed_workspace_and_shared_profiles_are_normalized_and_redacted() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(workspace.join(".alva")).unwrap();
        fs::write(
            workspace.join(".alva/config.json"),
            r#"{"api_key":"workspace-synthetic","model":"workspace-model","kind":"anthropic","base_url":"https://workspace.example.test"}"#,
        )
        .unwrap();
        let shared_path = directory.path().join("shared-config.json");
        fs::write(
            &shared_path,
            r#"{"providers":{"team":{"api_key":"shared-synthetic","model":"team-model","base_url":"https://team.example.test","kind":"openai-responses"}}}"#,
        )
        .unwrap();
        let catalog = ProviderCatalog::open_with_shared_config(
            directory.path().join("providers.json"),
            &shared_path,
        )
        .unwrap();

        let profiles = catalog.list(Some(&workspace)).unwrap();
        let workspace_profile = profiles
            .iter()
            .find(|profile| profile.id == "workspace-config")
            .unwrap();
        let shared_profile = profiles
            .iter()
            .find(|profile| profile.id == "shared:team")
            .unwrap();
        assert!(workspace_profile.managed && workspace_profile.api_key_configured);
        assert!(shared_profile.managed && shared_profile.api_key_configured);
        let json = serde_json::to_string(&profiles).unwrap();
        assert!(!json.contains("workspace-synthetic"));
        assert!(!json.contains("shared-synthetic"));

        let route = catalog
            .routes(Some(&workspace))
            .unwrap()
            .into_iter()
            .find(|route| route.target.provider_id == "shared:team")
            .unwrap();
        let resolved = catalog
            .resolve_target(&route.target, &workspace)
            .unwrap()
            .unwrap();
        assert_eq!(resolved.kind.as_deref(), Some("openai-responses"));
        assert_eq!(resolved.api_key, "shared-synthetic");

        let persisted = fs::read_to_string(directory.path().join("providers.json")).unwrap();
        assert!(!persisted.contains("workspace-synthetic"));
        assert!(!persisted.contains("shared-synthetic"));
        assert!(!persisted.contains("shared:team"));
    }

    #[test]
    fn default_target_is_explicit_persistent_and_never_silently_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        let catalog = ProviderCatalog::open(&path).unwrap();
        catalog
            .save(saved_anthropic(Some("synthetic"), "default-model"), None)
            .unwrap();
        let route = catalog
            .routes(None)
            .unwrap()
            .into_iter()
            .find(|route| route.target.model_id == "default-model")
            .unwrap();
        catalog
            .set_default_target(route.target.clone(), None)
            .unwrap();

        let reopened = ProviderCatalog::open(&path).unwrap();
        let default = reopened.default_target(None).unwrap().unwrap();
        assert_eq!(default.target.model_id, "default-model");
        assert!(default.available);

        let mut disabled = saved_anthropic(None, "default-model");
        disabled.enabled = false;
        reopened.save(disabled, None).unwrap();
        let unavailable = reopened.default_target(None).unwrap().unwrap();
        assert_eq!(unavailable.target.model_id, "default-model");
        assert!(!unavailable.available);
        assert!(reopened
            .set_default_target(unavailable.target, None)
            .is_err());
    }

    #[test]
    fn removed_default_target_remains_explicitly_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        let catalog = ProviderCatalog::open(&path).unwrap();
        catalog
            .save(saved_anthropic(Some("synthetic"), "removed-default"), None)
            .unwrap();
        let target = catalog
            .routes(None)
            .unwrap()
            .into_iter()
            .find(|route| route.target.model_id == "removed-default")
            .unwrap()
            .target;
        catalog.set_default_target(target, None).unwrap();
        catalog
            .save(saved_anthropic(Some("synthetic"), "replacement"), None)
            .unwrap();

        let default = catalog.default_target(None).unwrap().unwrap();
        assert_eq!(default.target.model_id, "removed-default");
        assert!(!default.available);
        assert!(default
            .unavailable_reason
            .unwrap()
            .contains("no longer configured"));
    }

    #[test]
    fn schema_v1_without_default_target_still_opens() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        fs::write(&path, r#"{"version":1,"providers":[]}"#).unwrap();
        let catalog = ProviderCatalog::open(&path).unwrap();
        assert!(catalog.default_target(None).unwrap().is_none());
        assert!(catalog
            .list(None)
            .unwrap()
            .iter()
            .any(|provider| provider.id == "anthropic"));
    }

    #[test]
    fn fixed_catalog_has_core_starters_and_read_only_subscriptions() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = ProviderCatalog::open(directory.path().join("providers.json")).unwrap();
        let profiles = catalog.list(None).unwrap();
        let expected = [
            ("anthropic", "claude-opus-4-7"),
            ("openai-responses", "gpt-5.4"),
            ("openai-compatible", "gpt-5.4"),
            ("gemini", "gemini-3.6-flash"),
        ];
        for (provider_id, model_id) in expected {
            let provider = profiles
                .iter()
                .find(|provider| provider.id == provider_id)
                .unwrap();
            assert_eq!(provider.models.len(), 1);
            assert_eq!(provider.models[0].id, model_id);
        }
        assert!(profiles
            .iter()
            .filter(|provider| matches!(provider.id.as_str(), "claude-cli" | "codex-cli"))
            .all(|provider| provider.managed));
    }
}
