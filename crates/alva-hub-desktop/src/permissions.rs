// INPUT:  alva_engine_runtime, claude/codex adapter permission knobs, serde
// OUTPUT: PermissionProfile, adapter enforcement contract, eligibility DTOs, PermissionRouter
// POS:    Session permission profile owner; decides which engines may keep or tighten it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alva_engine_adapter_claude::ClaudePrintPermissionMode;
use alva_engine_adapter_codex::CodexSandboxMode;
use alva_engine_runtime::{EngineRuntime, PermissionDecision};
use serde::{Deserialize, Serialize};

/// Stable per-session permission contract. It is chosen when the Hub session
/// is created and never changes across branch switches or fallback.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    /// Permission requests are routed to the user for an explicit decision.
    /// The engine's own risk classifier decides which tool calls raise a
    /// request — this profile guarantees the *routing* (a raised request is
    /// answered by the user, never auto-resolved), not that every side
    /// effect raises one.
    Interactive,
    /// Workspace-bounded side effects run without prompting.
    #[default]
    WorkspaceWrite,
    /// Everything is auto-allowed. Explicit user opt-in only.
    Unrestricted,
}

impl PermissionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::WorkspaceWrite => "workspace_write",
            Self::Unrestricted => "unrestricted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "interactive" => Ok(Self::Interactive),
            "workspace_write" => Ok(Self::WorkspaceWrite),
            "unrestricted" => Ok(Self::Unrestricted),
            other => Err(format!(
                "unknown Hub permission profile '{other}'; expected interactive, workspace_write, or unrestricted"
            )),
        }
    }
}

/// How the adapter mapping relates to the session profile. Anything weaker is
/// never constructed — such targets are ineligible instead.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementRelation {
    Equivalent,
    Stricter,
}

/// Concrete adapter knob the Hub build uses to enforce a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineEnforcement {
    /// Alva routes PermissionRequest/PermissionDecision to the Hub user.
    InteractiveCallbacks,
    ClaudePrintMode(ClaudePrintPermissionMode),
    CodexSandbox(CodexSandboxMode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforcementPlan {
    pub relation: EnforcementRelation,
    pub enforcement: EngineEnforcement,
    /// Human-readable mapping shown in eligibility UI and skip reasons.
    pub summary: String,
}

/// The adapter permission contract for the engines Hub actually builds.
///
/// A target is eligible only when its adapter can enforce the session profile
/// equivalently or stricter. Engines without a permission callback cannot
/// preserve the `interactive` contract: their closest mappings silently
/// auto-deny or auto-allow instead of asking the user, so they are ineligible
/// rather than quietly rewriting the profile.
pub fn enforcement_for(
    engine_id: &str,
    profile: PermissionProfile,
) -> Result<EnforcementPlan, String> {
    match (engine_id, profile) {
        ("alva", PermissionProfile::Interactive) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Equivalent,
            enforcement: EngineEnforcement::InteractiveCallbacks,
            summary: "risk-gated permission requests are routed to you for approval".into(),
        }),
        ("alva", PermissionProfile::WorkspaceWrite | PermissionProfile::Unrestricted) => {
            Ok(EnforcementPlan {
                relation: EnforcementRelation::Stricter,
                enforcement: EngineEnforcement::InteractiveCallbacks,
                summary: "alva still routes risk-gated requests to you instead of auto-allowing"
                    .into(),
            })
        }
        ("claude", PermissionProfile::Interactive) => Err(ineligible_reason(
            "claude",
            profile,
            "the claude CLI runs non-interactively and cannot route permission \
             approvals to you; its closest mappings auto-deny (dontAsk) or \
             auto-allow (acceptEdits), which weakens or silently rewrites the profile",
        )),
        ("claude", PermissionProfile::WorkspaceWrite) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Stricter,
            enforcement: EngineEnforcement::ClaudePrintMode(ClaudePrintPermissionMode::AcceptEdits),
            summary: "acceptEdits auto-accepts only file edits; other side effects are denied"
                .into(),
        }),
        ("claude", PermissionProfile::Unrestricted) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Equivalent,
            enforcement: EngineEnforcement::ClaudePrintMode(
                ClaudePrintPermissionMode::BypassPermissions,
            ),
            summary: "bypassPermissions auto-allows everything".into(),
        }),
        ("codex", PermissionProfile::Interactive) => Err(ineligible_reason(
            "codex",
            profile,
            "the codex CLI runs with approval_policy=never and cannot route \
             permission approvals to you; its sandbox either auto-allows \
             workspace writes or silently denies them, which weakens or \
             silently rewrites the profile",
        )),
        ("codex", PermissionProfile::WorkspaceWrite) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Equivalent,
            enforcement: EngineEnforcement::CodexSandbox(CodexSandboxMode::WorkspaceWrite),
            summary: "the workspace-write sandbox bounds side effects to the workspace".into(),
        }),
        ("codex", PermissionProfile::Unrestricted) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Equivalent,
            enforcement: EngineEnforcement::CodexSandbox(CodexSandboxMode::DangerFullAccess),
            summary: "danger-full-access auto-allows everything".into(),
        }),
        (other, _) => Err(format!(
            "unsupported Hub engine '{other}'; expected alva, claude, or codex"
        )),
    }
}

fn ineligible_reason(engine_id: &str, profile: PermissionProfile, detail: &str) -> String {
    format!(
        "{engine_id} cannot preserve or tighten the session permission profile \
         '{}': {detail}",
        profile.as_str()
    )
}

/// Eligibility DTO for the engine selector and policy UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EngineEligibility {
    pub engine_id: String,
    pub eligible: bool,
    pub relation: Option<EnforcementRelation>,
    /// Mapping summary when eligible; the ineligibility reason otherwise.
    pub detail: String,
}

pub fn permission_eligibility(profile: PermissionProfile) -> Vec<EngineEligibility> {
    crate::readiness::HUB_ENGINE_IDS
        .iter()
        .map(|engine_id| match enforcement_for(engine_id, profile) {
            Ok(plan) => EngineEligibility {
                engine_id: (*engine_id).to_string(),
                eligible: true,
                relation: Some(plan.relation),
                detail: plan.summary,
            },
            Err(reason) => EngineEligibility {
                engine_id: (*engine_id).to_string(),
                eligible: false,
                relation: None,
                detail: reason,
            },
        })
        .collect()
}

struct PendingPermission {
    engine: Arc<dyn EngineRuntime>,
    engine_session_id: String,
}

/// Routes user permission decisions back to the exact engine session that
/// emitted the request, even after fallback replaced the active engine.
/// Entries are single-use: a delivered decision cannot be replayed.
#[derive(Default)]
pub struct PermissionRouter {
    pending: Mutex<HashMap<(String, String), PendingPermission>>,
}

impl PermissionRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &self,
        product_session_id: &str,
        request_id: &str,
        engine: Arc<dyn EngineRuntime>,
        engine_session_id: &str,
    ) {
        self.pending
            .lock()
            .expect("permission router poisoned")
            .insert(
                (product_session_id.to_string(), request_id.to_string()),
                PendingPermission {
                    engine,
                    engine_session_id: engine_session_id.to_string(),
                },
            );
    }

    pub async fn respond(
        &self,
        product_session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), String> {
        let pending = self
            .pending
            .lock()
            .expect("permission router poisoned")
            .remove(&(product_session_id.to_string(), request_id.to_string()))
            .ok_or_else(|| {
                format!(
                    "permission request '{request_id}' is not pending for Hub session \
                     '{product_session_id}'; it may already be answered or the attempt ended"
                )
            })?;
        pending
            .engine
            .respond_permission(&pending.engine_session_id, request_id, decision)
            .await
            .map_err(|error| error.to_string())
    }

    /// Drop every pending request of a session — the attempt reached a
    /// terminal state or fell back, so its approvals are dead.
    pub fn clear_session(&self, product_session_id: &str) {
        self.pending
            .lock()
            .expect("permission router poisoned")
            .retain(|(session, _), _| session != product_session_id);
    }
}

#[cfg(test)]
mod tests {
    //! Contract tests for the three Hub adapter permission mappings and the
    //! decision router.
    //!
    //! The 3×3 table below is the authority for target eligibility: run.rs
    //! fallback gating and alva.rs engine assembly both consume it, so any
    //! mapping change must be made deliberately here first.

    use std::pin::Pin;
    use std::sync::Mutex as StdMutex;

    use alva_engine_runtime::{RuntimeCapabilities, RuntimeError, RuntimeEvent, RuntimeRequest};
    use async_trait::async_trait;
    use futures_core::Stream;

    use super::*;

    type ContractRow = (
        &'static str,
        PermissionProfile,
        Option<(EnforcementRelation, EngineEnforcement)>,
    );

    #[test]
    fn adapter_mapping_contract_table_is_stable() {
        use EnforcementRelation::{Equivalent, Stricter};
        let table: Vec<ContractRow> = vec![
            (
                "alva",
                PermissionProfile::Interactive,
                Some((Equivalent, EngineEnforcement::InteractiveCallbacks)),
            ),
            (
                "alva",
                PermissionProfile::WorkspaceWrite,
                Some((Stricter, EngineEnforcement::InteractiveCallbacks)),
            ),
            (
                "alva",
                PermissionProfile::Unrestricted,
                Some((Stricter, EngineEnforcement::InteractiveCallbacks)),
            ),
            ("claude", PermissionProfile::Interactive, None),
            (
                "claude",
                PermissionProfile::WorkspaceWrite,
                Some((
                    Stricter,
                    EngineEnforcement::ClaudePrintMode(ClaudePrintPermissionMode::AcceptEdits),
                )),
            ),
            (
                "claude",
                PermissionProfile::Unrestricted,
                Some((
                    Equivalent,
                    EngineEnforcement::ClaudePrintMode(
                        ClaudePrintPermissionMode::BypassPermissions,
                    ),
                )),
            ),
            ("codex", PermissionProfile::Interactive, None),
            (
                "codex",
                PermissionProfile::WorkspaceWrite,
                Some((
                    Equivalent,
                    EngineEnforcement::CodexSandbox(CodexSandboxMode::WorkspaceWrite),
                )),
            ),
            (
                "codex",
                PermissionProfile::Unrestricted,
                Some((
                    Equivalent,
                    EngineEnforcement::CodexSandbox(CodexSandboxMode::DangerFullAccess),
                )),
            ),
        ];
        for (engine_id, profile, expected) in table {
            let plan = enforcement_for(engine_id, profile);
            match expected {
                Some((relation, enforcement)) => {
                    let plan = plan.unwrap_or_else(|reason| {
                        panic!(
                            "{engine_id}/{} must be eligible: {reason}",
                            profile.as_str()
                        )
                    });
                    assert_eq!(plan.relation, relation, "{engine_id}/{}", profile.as_str());
                    assert_eq!(
                        plan.enforcement,
                        enforcement,
                        "{engine_id}/{}",
                        profile.as_str()
                    );
                }
                None => {
                    let reason = plan.expect_err("weaken-only mapping must be ineligible");
                    assert!(
                        reason.contains(engine_id) && reason.contains(profile.as_str()),
                        "ineligibility reason must name engine and profile: {reason}"
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_engine_is_rejected_loudly() {
        let error = enforcement_for("gemini", PermissionProfile::WorkspaceWrite).unwrap_err();
        assert!(error.contains("unsupported Hub engine 'gemini'"));
    }

    #[test]
    fn profile_round_trips_through_storage_text() {
        for profile in [
            PermissionProfile::Interactive,
            PermissionProfile::WorkspaceWrite,
            PermissionProfile::Unrestricted,
        ] {
            assert_eq!(PermissionProfile::parse(profile.as_str()), Ok(profile));
        }
        assert!(PermissionProfile::parse("yolo").is_err());
    }

    #[test]
    fn eligibility_report_covers_all_engines_with_reasons() {
        let report = permission_eligibility(PermissionProfile::Interactive);
        assert_eq!(
            report
                .iter()
                .map(|e| e.engine_id.as_str())
                .collect::<Vec<_>>(),
            vec!["alva", "claude", "codex"]
        );
        assert!(report[0].eligible);
        assert_eq!(report[0].relation, Some(EnforcementRelation::Equivalent));
        for ineligible in &report[1..] {
            assert!(!ineligible.eligible);
            assert!(ineligible.relation.is_none());
            assert!(ineligible.detail.contains("cannot preserve or tighten"));
        }
    }

    struct RecordingEngine {
        responses: StdMutex<Vec<(String, String, bool)>>,
    }

    impl RecordingEngine {
        fn new() -> Self {
            Self {
                responses: StdMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl EngineRuntime for RecordingEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Err(RuntimeError::ProcessError("not used".into()))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            session_id: &str,
            request_id: &str,
            decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            self.responses.lock().unwrap().push((
                session_id.to_string(),
                request_id.to_string(),
                matches!(decision, PermissionDecision::Allow { .. }),
            ));
            Ok(())
        }

        fn capabilities(&self) -> RuntimeCapabilities {
            RuntimeCapabilities {
                streaming: true,
                tool_control: true,
                permission_callback: true,
                resume: true,
                cancel: true,
            }
        }
    }

    #[tokio::test]
    async fn decision_routes_to_the_engine_session_that_asked() {
        let router = PermissionRouter::new();
        let source = Arc::new(RecordingEngine::new());
        let fallback_engine = Arc::new(RecordingEngine::new());
        router.register(
            "product-1",
            "req-1",
            Arc::clone(&source) as Arc<dyn EngineRuntime>,
            "engine-session-a",
        );
        // A later attempt on another engine must not steal the decision.
        router.register(
            "product-1",
            "req-2",
            Arc::clone(&fallback_engine) as Arc<dyn EngineRuntime>,
            "engine-session-b",
        );

        router
            .respond(
                "product-1",
                "req-1",
                PermissionDecision::Allow {
                    updated_input: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            source.responses.lock().unwrap().as_slice(),
            &[("engine-session-a".to_string(), "req-1".to_string(), true)]
        );
        assert!(fallback_engine.responses.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn decisions_are_single_use_and_unknown_requests_fail_loudly() {
        let router = PermissionRouter::new();
        let engine = Arc::new(RecordingEngine::new());
        router.register(
            "product-1",
            "req-1",
            Arc::clone(&engine) as Arc<dyn EngineRuntime>,
            "engine-session-a",
        );
        router
            .respond(
                "product-1",
                "req-1",
                PermissionDecision::Deny {
                    message: "no".into(),
                },
            )
            .await
            .unwrap();

        let replay = router
            .respond(
                "product-1",
                "req-1",
                PermissionDecision::Allow {
                    updated_input: None,
                },
            )
            .await
            .unwrap_err();
        assert!(replay.contains("not pending"));
        assert_eq!(engine.responses.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn clear_session_drops_only_that_sessions_pending_requests() {
        let router = PermissionRouter::new();
        let engine = Arc::new(RecordingEngine::new());
        router.register(
            "product-1",
            "req-1",
            Arc::clone(&engine) as Arc<dyn EngineRuntime>,
            "engine-session-a",
        );
        router.register(
            "product-2",
            "req-1",
            Arc::clone(&engine) as Arc<dyn EngineRuntime>,
            "engine-session-b",
        );

        router.clear_session("product-1");

        let dead = router
            .respond(
                "product-1",
                "req-1",
                PermissionDecision::Allow {
                    updated_input: None,
                },
            )
            .await
            .unwrap_err();
        assert!(dead.contains("not pending"));
        router
            .respond(
                "product-2",
                "req-1",
                PermissionDecision::Allow {
                    updated_input: None,
                },
            )
            .await
            .unwrap();
    }
}
