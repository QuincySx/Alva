// INPUT:  alva, run, store (sub-modules)
// OUTPUT: engine assembly, persisted run/switch seams, branch/policy/handoff/timeline DTOs
// POS:    Hub Desktop multi-engine library facade shared by Tauri IPC and product tests.

mod alva;
mod automation_store;
mod claims;
mod failure;
mod models;
mod permissions;
mod plugin_store;
mod provider_store;
mod readiness;
mod redact;
mod repair;
mod run;
mod skill_store;
mod store;
mod ui_projection;
mod worktree;

pub use alva::{
    build_engine, build_engine_bundle, build_engine_bundle_for_target,
    build_engine_bundle_for_target_with_provider, EngineBuild,
};
pub use alva_protocol_skill::types::Skill as InstalledSkill;
pub use automation_store::{
    AutomationDefinition, AutomationRunStatus, AutomationSchedule, AutomationStore,
};
pub use claims::{claims_conflict, conflicting_claims, parse_claims, ClaimMode, ResourceClaim};
pub use failure::{classify_failure, triggers_fallback, FailureClass};
pub use models::{
    protocol_supports_runtime, AgentRuntime, ApiProtocol, CustomModelProfile,
    CustomProviderProfile, ModelCapabilities, ModelCatalog, ModelProfile, ModelRoute, ProviderAuth,
    ProviderProfile, ProviderRuntimeConfig, ReasoningEffort, StoredCustomProvider,
};
pub use permissions::{
    enforcement_for, permission_eligibility, permission_group_key, EnforcementPlan,
    EnforcementRelation, EngineEligibility, EngineEnforcement, PermissionGroup, PermissionMember,
    PermissionProfile, PermissionRouter,
};
pub use plugin_store::{InstalledPlugin, PluginScope, PluginStore};
pub use provider_store::ProviderStore;
pub use readiness::{
    capability_labels, classify_engine, probe_readiness, stream_health_from_outcome, EngineProbe,
    EngineProbeFacts, EngineReadiness, ReadinessReason, ReadinessStatus, StreamHealth, SystemProbe,
    HUB_ENGINE_IDS,
};
pub use redact::{redact_handoff, redact_text};
pub use repair::{
    repair_argv, repair_plan, run_repair_command, RepairAction, RepairKind, RepairOutcome,
};
pub use run::{
    run_once, run_persisted, run_persisted_with_target, PermissionRequestInfo, RunOutcome,
    RunRequest, TimelineEvent, TimelineStatus,
};
pub use skill_store::SkillCatalog;
pub use store::{
    EngineTarget, FallbackPolicy, HandoffKind, HandoffPackage, HubSessionDetail, HubSessionSummary,
    HubStore, NativeCleanupTask, PendingSwitch, SideEffectEvidence, StoredAttempt, StoredBranch,
    StoredTimelineEvent, StoredTurn, StoredUsage, SwitchDisposition, SwitchOutcome, TaskClaim,
    TeamRunDetail, TeamWorker, TranscriptTurn, WorkerLifecycle, WorkerTask,
};
pub use ui_projection::{
    project_chat, project_timeline, ChatItemKind, FallbackControls, ProjectedChatItem,
    ProjectedEvent,
};
pub use worktree::{
    adopt_into_workspace, adoption_preview, create_worker_worktree, discard_worktree,
    supports_isolation, worktree_changes, worktree_root, AdoptionPreview, WorktreeChange,
    WorktreeHandle,
};
