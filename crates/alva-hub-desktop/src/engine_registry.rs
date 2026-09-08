// INPUT:  alva_engine_runtime::EngineDescriptor, crate::permissions, crate::readiness
// OUTPUT: EngineBlueprint, register, blueprint, registered_engine_ids, enforcement_for
// POS:    The dynamic engine registry (ticket 17). Every engine — builtin or
//         registered later — declares ONE blueprint: its self-description, its
//         readiness spec, its probe/veto/enforcement/build closures. The
//         permission gate, readiness classifier, and assembly consult the
//         registry instead of matching on engine names; registering a new
//         engine requires touching nothing in those three modules.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use alva_engine_runtime::EngineDescriptor;
use alva_llm_provider::ProviderConfig;

use crate::permissions::{EnforcementPlan, PermissionProfile};
use crate::readiness::{EngineProbeFacts, SystemProbe};
use crate::store::EngineTarget;

/// Everything the build step needs. Assembled by the run seam from the
/// resolved target, the provider route, the permission plan, and the
/// optional shared repository.
pub struct EngineBuildArgs<'a> {
    pub target: &'a EngineTarget,
    pub provider_config: Option<ProviderConfig>,
    pub plan: &'a EnforcementPlan,
    pub workspace: &'a Path,
    pub history_dir: &'a Path,
    pub repository: Option<alva_engine_adapter_alva::SessionRepository>,
}

type ProbeFn = dyn Fn(&SystemProbe) -> EngineProbeFacts + Send + Sync;
type VetoFn = dyn Fn(&SystemProbe, bool) -> Option<String> + Send + Sync;
type EnforcementFn = dyn Fn(PermissionProfile) -> Result<EnforcementPlan, String> + Send + Sync;
type BuildFn = dyn Fn(&EngineBuildArgs) -> Result<Arc<dyn alva_engine_runtime::EngineRuntime>, String>
    + Send
    + Sync;

/// One engine's complete self-registration. Nothing outside this struct (and
/// the wiring that creates it) may name the engine.
pub struct EngineBlueprint {
    pub engine_id: String,
    /// Self-description. `None` for the in-process builtin whose descriptor
    /// would need a fully-configured runtime to construct; such engines carry
    /// their own enforcement and probe closures instead.
    pub descriptor: Option<EngineDescriptor>,
    pub readiness: crate::readiness::ReadinessSpec,
    /// Gather raw probe facts for this engine from a system probe pass.
    pub probe: Arc<ProbeFn>,
    /// Fast fallback veto (binary presence / credential presence only).
    pub quick_veto: Arc<VetoFn>,
    /// Map a session permission profile to this engine's enforcement plan.
    /// `None` falls back to the descriptor-driven plan: undeclared profiles
    /// fail by name, capable profiles are enforced by the engine itself.
    pub enforcement: Option<Arc<EnforcementFn>>,
    /// Construct the runtime.
    pub build: Arc<BuildFn>,
}

struct RegistryState {
    order: Vec<String>,
    map: HashMap<String, Arc<EngineBlueprint>>,
}

fn registry() -> &'static RwLock<RegistryState> {
    static REGISTRY: OnceLock<RwLock<RegistryState>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        RwLock::new(RegistryState {
            order: Vec::new(),
            map: HashMap::new(),
        })
    })
}

/// Register a blueprint. The first registration fixes the iteration order;
/// re-registering the same id replaces its blueprint in place (useful for
/// tests and for product-level adapter upgrades).
pub fn register(blueprint: EngineBlueprint) {
    let mut state = registry().write().expect("engine registry lock");
    if !state.order.contains(&blueprint.engine_id) {
        state.order.push(blueprint.engine_id.clone());
    }
    state
        .map
        .insert(blueprint.engine_id.clone(), Arc::new(blueprint));
}

/// The builtin engines, registered once on first consult. The names live in
/// `crate::engine_wiring` — the single wiring point — never in the gate,
/// readiness, or assembly modules.
pub fn ensure_builtins() {
    static SEEDED: OnceLock<()> = OnceLock::new();
    SEEDED.get_or_init(|| {
        for blueprint in crate::engine_wiring::builtin_blueprints() {
            register(blueprint);
        }
    });
}

/// Look up a blueprint (builtin seeding included).
pub fn blueprint(engine_id: &str) -> Option<Arc<EngineBlueprint>> {
    ensure_builtins();
    registry()
        .read()
        .expect("engine registry lock")
        .map
        .get(engine_id)
        .cloned()
}

/// Registration-order engine ids — the dashboard's engine list.
pub fn registered_engine_ids() -> Vec<String> {
    ensure_builtins();
    registry()
        .read()
        .expect("engine registry lock")
        .order
        .clone()
}

/// Permission-gate entry: delegate to the engine's own enforcement mapping.
pub fn enforcement_for(
    engine_id: &str,
    profile: PermissionProfile,
) -> Result<EnforcementPlan, String> {
    let Some(entry) = blueprint(engine_id) else {
        return Err(format!(
            "unsupported Hub engine '{engine_id}'; expected one of: {}",
            registered_engine_ids().join(", ")
        ));
    };
    match &entry.enforcement {
        Some(custom) => custom(profile),
        None => match &entry.descriptor {
            Some(descriptor) => default_descriptor_enforcement(descriptor, engine_id, profile),
            None => Err(format!(
                "engine '{engine_id}' declares neither a product enforcement mapping \
                 nor a descriptor permission plan"
            )),
        },
    }
}

/// The descriptor-driven plan for engines that do not carry a product-side
/// mapping: the engine enforces its own declared plan, or it is judged
/// ineligible by name when it cannot satisfy the profile at all.
fn default_descriptor_enforcement(
    descriptor: &EngineDescriptor,
    engine_id: &str,
    profile: PermissionProfile,
) -> Result<EnforcementPlan, String> {
    use alva_engine_runtime::PermissionRelation;

    let profile_key = profile.as_str();
    if !descriptor.can_satisfy(profile_key) {
        return Err(format!(
            "engine '{engine_id}' is declared in the catalog but cannot satisfy permission \
             profile '{profile_key}'"
        ));
    }
    let plan = descriptor
        .permission_plans
        .iter()
        .find(|plan| plan.profile_id == profile_key)
        .expect("can_satisfy checked the same plan set");
    Ok(EnforcementPlan {
        relation: match plan.relation {
            PermissionRelation::Equivalent => crate::permissions::EnforcementRelation::Equivalent,
            PermissionRelation::Stricter => crate::permissions::EnforcementRelation::Stricter,
        },
        enforcement: crate::permissions::EngineEnforcement::DeclaredByEngine,
        summary: plan.summary.clone(),
    })
}

/// Test/inspection convenience: run a blueprint's build entry with empty
/// routing inputs. Production assembly goes through the run seam's build
/// closure (see `alva::build_engine_for_target_with_repository`).
pub fn engine_registry_for_test_build(
    engine_id: &str,
) -> Result<Arc<dyn alva_engine_runtime::EngineRuntime>, String> {
    let entry = blueprint(engine_id).ok_or("engine not registered")?;
    let target = crate::store::EngineTarget::for_engine(engine_id)?;
    let plan = enforcement_for(engine_id, PermissionProfile::WorkspaceWrite)?;
    let args = EngineBuildArgs {
        target: &target,
        provider_config: None,
        plan: &plan,
        workspace: &std::path::PathBuf::from(std::env::temp_dir()),
        history_dir: &std::path::PathBuf::from(std::env::temp_dir()),
        repository: None,
    };
    (entry.build)(&args)
}
