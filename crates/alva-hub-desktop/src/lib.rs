// INPUT:  Hub Desktop engine, provider, workspace, run, store, and collaboration sub-modules
// OUTPUT: engine/provider/workspace/Skills/automation/MCP, reference-first input files and durable orchestration APIs
// POS:    Hub Desktop multi-engine library facade shared by Tauri IPC and product tests.

mod alva;
mod claims;
pub mod engine_registry;
mod engine_wiring;
mod failure;
mod inspection;
mod mcp_diagnostics;
pub mod permissions;
mod provider_diagnostics;
mod provider_plane;
mod providers;
pub mod readiness;
mod redact;
mod repair;
mod run;
mod session_workspace;
mod sdk_installation;
mod store;
mod worktree;
mod workspace_files;
mod workspace_git;
mod workspace_skills;
mod terminal;
mod user_browser;
pub mod worktree_discovery;

pub use alva::{build_engine, build_engine_for_target, build_engine_for_target_with_inspector, build_engine_for_target_with_settings};
pub use claims::{claims_conflict, conflicting_claims, ClaimMode, ResourceClaim};
pub use engine_registry::{
    blueprint, register as register_engine_blueprint, registered_engine_ids, EngineBlueprint,
    EngineBuildArgs,
};
pub use failure::{classify_failure, triggers_fallback, FailureClass};
pub use inspection::{HubInspectionRecord, HubInspectionSnapshot, HubInspectorService};
pub use mcp_diagnostics::{probe_managed_mcp, McpDiagnostic};
pub use permissions::{
    enforcement_for, is_stale_permission_error, permission_eligibility, permission_group_key,
    release_run_engines, EnforcementPlan, EnforcementRelation, EngineEligibility,
    EngineEnforcement, PermissionAnswerOutcome, PermissionGroup, PermissionMember,
    PermissionProfile, PermissionRouter, RunEngineLease,
};
pub use provider_diagnostics::{
    diagnose_provider_inference, discover_provider_models, DiscoveredProviderModel,
    ProviderDiagnosticResult,
};
pub use providers::{
    CreateProviderProfile, ModelRoute, ProviderCatalog, ProviderModel, ProviderModelOptions,
    ProviderProfile, SaveProviderProfile,
};
pub use readiness::{
    capability_labels, classify_engine, is_executable_file, probe_readiness, read_cli_version,
    stream_health_from_outcome, version_at_least, veto_missing_declared_binary, AuthKind,
    EngineProbe, EngineProbeFacts, EngineReadiness, ReadinessReason, ReadinessSpec,
    ReadinessStatus, StreamHealth, SystemProbe, ENGINE_CAPABILITIES, MODEL_CAPABILITIES,
};
pub use redact::{redact_handoff, redact_text};
pub use repair::{
    repair_argv, repair_plan, run_repair_command, RepairAction, RepairKind, RepairOutcome,
};
pub use run::{
    run_once, run_persisted, run_persisted_with_target, run_persisted_with_automation, ActiveHubs, MessagePosition, RunOutcome,
    RunRequest, TimelineActivity, TimelineEvent, TimelineStatus, run_persisted_with_input_attachments,
};
pub use session_workspace::{create_managed_session_workspace, WorkspaceBinding, WorkspaceKind};
pub use store::{
    Automation, AutomationAction,
    validate_input_attachments, InputAttachment, InputAttachmentRef, InputAttachmentContent,
    HubToolSettings, ToolComponent, ManagedMcpServer, ToolSettingsAction,
    EngineTarget, FallbackPolicy, HandoffKind, HandoffPackage, HubSessionDetail, HubSessionSummary,
    HubStore, MessageJournalPage, MessageJournalRecord, PendingSwitch, ProjectSummary, SideEffectEvidence, StoredAttempt, StoredBranch,
    StoredTimelineEvent, StoredTurn, StoredUsage, SwitchDisposition, SwitchOutcome, TaskClaim,
    TeamRunDetail, TeamWorker, ToolContentChunk, ToolContentRef, TranscriptTurn, WorkerLifecycle, WorkerTask,
};
pub use worktree_discovery::{discover_worktrees, WorktreeDiscoveryError, WorktreeDiscoveryReply, WorktreeInfo};
pub use workspace_files::{list_workspace_files, read_workspace_file, WorkspaceDirectory, WorkspaceEntry, WorkspacePreview};
pub use workspace_git::{workspace_git_status, workspace_git_diff, GitDiffArea, GitFileStatus, WorkspaceGitStatus, WorkspaceGitDiff};
pub use workspace_skills::{WorkspaceSkill, WorkspaceSkillAction, WorkspaceSkillCatalog, WorkspaceSkillDocument, WorkspaceSkillsManager};
pub use terminal::{TerminalManager, TerminalSnapshot};
pub use user_browser::{privileged_window, BrowserSnapshot, UserBrowserManager};
pub use worktree::{
    adopt_into_workspace, adoption_preview, create_worker_worktree, discard_worktree,
    supports_isolation, worktree_changes, worktree_root, AdoptionPreview, WorktreeChange,
    WorktreeHandle,
};
