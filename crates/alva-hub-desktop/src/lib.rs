// INPUT:  Hub Desktop engine, provider, workspace, run, store, and collaboration sub-modules
// OUTPUT: public engine/catalog/workspace plus persisted run, branch, policy, handoff, and timeline seams
// POS:    Hub Desktop multi-engine library facade shared by Tauri IPC and product tests.

mod alva;
mod claims;
mod failure;
mod permissions;
mod provider_diagnostics;
mod provider_plane;
mod providers;
mod readiness;
mod redact;
mod repair;
mod run;
mod session_workspace;
mod store;
mod worktree;

pub use alva::{build_engine, build_engine_for_target};
pub use claims::{claims_conflict, conflicting_claims, ClaimMode, ResourceClaim};
pub use failure::{classify_failure, triggers_fallback, FailureClass};
pub use permissions::{
    enforcement_for, permission_eligibility, permission_group_key, EnforcementPlan,
    EnforcementRelation, EngineEligibility, EngineEnforcement, PermissionGroup, PermissionMember,
    PermissionProfile, PermissionRouter,
};
pub use provider_diagnostics::{
    diagnose_provider_inference, discover_provider_models, DiscoveredProviderModel,
    ProviderDiagnosticResult,
};
pub use providers::{
    ModelRoute, ProviderCatalog, ProviderModel, ProviderModelOptions, ProviderProfile,
    SaveProviderProfile,
};
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
    run_once, run_persisted, run_persisted_with_target, RunOutcome, RunRequest, TimelineActivity,
    TimelineEvent, TimelineStatus,
};
pub use session_workspace::{
    create_managed_session_workspace, WorkspaceBinding, WorkspaceKind,
};
pub use store::{
    EngineTarget, FallbackPolicy, HandoffKind, HandoffPackage, HubSessionDetail, HubSessionSummary,
    HubStore, PendingSwitch, SideEffectEvidence, StoredAttempt, StoredBranch, StoredTimelineEvent,
    StoredTurn, StoredUsage, SwitchDisposition, SwitchOutcome, TaskClaim, TeamRunDetail,
    TeamWorker, TranscriptTurn, WorkerLifecycle, WorkerTask,
};
pub use worktree::{
    adopt_into_workspace, adoption_preview, create_worker_worktree, discard_worktree,
    supports_isolation, worktree_changes, worktree_root, AdoptionPreview, WorktreeChange,
    WorktreeHandle,
};
