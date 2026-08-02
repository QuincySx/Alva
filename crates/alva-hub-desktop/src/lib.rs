// INPUT:  alva, run, store (sub-modules)
// OUTPUT: engine assembly, persisted run/switch seams, branch/policy/handoff/timeline DTOs
// POS:    Hub Desktop multi-engine library facade shared by Tauri IPC and product tests.

mod alva;
mod failure;
mod readiness;
mod run;
mod store;

pub use alva::build_engine;
pub use failure::{classify_failure, triggers_fallback, FailureClass};
pub use readiness::{
    capability_labels, classify_engine, probe_readiness, stream_health_from_outcome, EngineProbe,
    EngineProbeFacts, EngineReadiness, ReadinessReason, ReadinessStatus, StreamHealth, SystemProbe,
    HUB_ENGINE_IDS,
};
pub use run::{run_once, run_persisted, RunOutcome, RunRequest, TimelineEvent, TimelineStatus};
pub use store::{
    EngineTarget, FallbackPolicy, HandoffKind, HandoffPackage, HubSessionDetail, HubSessionSummary,
    HubStore, PendingSwitch, SideEffectEvidence, StoredAttempt, StoredBranch, StoredTimelineEvent,
    StoredTurn, StoredUsage, SwitchDisposition, SwitchOutcome,
};
