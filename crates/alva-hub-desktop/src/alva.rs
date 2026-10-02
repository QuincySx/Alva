// INPUT:  engine adapters, provider/profile route, claimed HubSession/HubStore and Inspector authority
// OUTPUT: route/profile-aware assembly with ALVA capability snapshot and root-bound product context/media Plugin recipe
// POS:    Hub Desktop engine assembly; adapters stay behind EngineRuntime and
//         are always configured from the session permission profile.

use std::path::Path;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory, SessionRepository};
use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_adapter_pi::{
    PiAdapter, PiAdapterConfig, PiApiProtocol, PiProviderConfig, PiToolPolicy,
};
use alva_engine_runtime::EngineRuntime;
use alva_llm_provider::ProviderConfig;

use crate::inspection::HubInspectorService;
use crate::permissions::{enforcement_for, EngineEnforcement, PermissionProfile};
use crate::store::EngineTarget;

/// Assemble an engine that enforces `profile` equivalently or stricter.
/// Fails closed when the engine can only weaken the session's permission
/// contract — continuity never overrides it.
pub fn build_engine(
    engine_id: &str,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let target = EngineTarget::for_engine(engine_id)?;
    // In-process engines (no required binary) run against the workspace
    // provider config; descriptor-driven, never name-driven.
    let provider_config = match crate::engine_registry::blueprint(engine_id) {
        Some(entry) => match &entry.descriptor {
            Some(descriptor) if descriptor.install_metadata.required_binary.is_none() => {
                Some(ProviderConfig::load(workspace)?)
            }
            Some(_) => None,
            // The builtin in-process engine carries no descriptor: its
            // blueprint build closure owns the provider-config requirement,
            // and loading the workspace config here keeps that contract.
            None => Some(ProviderConfig::load(workspace)?),
        },
        None => None,
    };
    build_engine_for_target(&target, provider_config, profile, workspace, history_dir)
}

/// Assemble the exact provider/model route selected in the composer.
pub fn build_engine_for_target(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    build_engine_for_target_with_repository(
        target,
        provider_config,
        profile,
        workspace,
        history_dir,
        None,
        None,
        None,
    )
}

/// Assemble an engine using the same Alva native repository as Hub Inspector.
/// Non-Alva adapters ignore the service because they truthfully expose no full
/// native inspection capability yet.
pub fn build_engine_for_target_with_inspector(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    inspector: &HubInspectorService,
) -> Result<Arc<dyn EngineRuntime>, String> {
    build_engine_for_target_with_repository(
        target,
        provider_config,
        profile,
        workspace,
        history_dir,
        Some(inspector.repository()),
        None,
        None,
    )
}

/// Hub product assembly with a single persisted ALVA capability snapshot.
pub fn build_engine_for_target_with_settings(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    inspector: &HubInspectorService,
    settings: &crate::HubToolSettings,
) -> Result<Arc<dyn EngineRuntime>, String> {
    build_engine_for_target_with_repository(
        target,
        provider_config,
        profile,
        workspace,
        history_dir,
        Some(inspector.repository()),
        Some(settings),
        None,
    )
}

/// Product-owned context for the already claimed HubSession. Other engines
/// ignore this ALVA Plugin recipe; no workspace/global history is scanned.
pub fn build_engine_for_target_with_context(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    inspector: &HubInspectorService,
    settings: &crate::HubToolSettings,
    store: &crate::HubStore,
    product_session_id: &str,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let image_capability = target
        .required_capabilities
        .iter()
        .any(|capability| capability == "images" || capability == "vision");
    let plugins = crate::hub_context::HubContextPluginFactory::new(
        store.clone(), product_session_id.into(),
    ).with_media_route(
        image_capability, format!("{}/{}", target.provider_id, target.model_id),
    );
    build_engine_for_target_with_repository(
        target, provider_config, profile, workspace, history_dir,
        Some(inspector.repository()), Some(settings), Some(Arc::new(plugins)),
    )
}

fn build_engine_for_target_with_repository(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    repository: Option<SessionRepository>,
    tool_settings: Option<&crate::HubToolSettings>,
    session_plugins: Option<Arc<dyn alva_engine_adapter_alva::SessionPluginFactory>>,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let engine_id = target.engine_id.as_str();
    let plan = enforcement_for(engine_id, profile)?;
    let args = crate::engine_registry::EngineBuildArgs {
        target,
        provider_config,
        plan: &plan,
        workspace,
        history_dir,
        repository,
        tool_settings,
        session_plugins,
    };
    match crate::engine_registry::blueprint(engine_id) {
        Some(entry) => (entry.build)(&args),
        None => Err(format!(
            "engine '{engine_id}' is not supported by Hub; it must be registered \
             with a blueprint to be buildable"
        )),
    }
}
