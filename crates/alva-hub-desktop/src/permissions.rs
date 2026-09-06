// INPUT:  alva_engine_runtime, claude/codex/pi adapter permission knobs, serde
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
    PiReadOnly,
    PiUnrestricted,
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
        ("pi", PermissionProfile::Interactive) => Err(ineligible_reason(
            "pi",
            profile,
            "Pi print mode does not expose interactive permission callbacks",
        )),
        ("pi", PermissionProfile::WorkspaceWrite) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Stricter,
            enforcement: EngineEnforcement::PiReadOnly,
            summary: "Pi is restricted to read, grep, find, and ls tools".into(),
        }),
        ("pi", PermissionProfile::Unrestricted) => Ok(EnforcementPlan {
            relation: EnforcementRelation::Equivalent,
            enforcement: EngineEnforcement::PiUnrestricted,
            summary: "Pi runs its complete configured tool set".into(),
        }),
        (other, _) => Err(format!(
            "unsupported Hub engine '{other}'; expected alva, claude, codex, or pi"
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
    /// What the request is asking for, so equivalent asks can be shown as
    /// one card without hiding which worker wants what.
    member: PermissionMember,
}

/// One request inside a (possibly grouped) approval card. Every field the
/// user needs to judge it stays per-request: aggregation must never hide the
/// worker, the action, or the resource scope.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PermissionMember {
    /// Collaboration scope this request belongs to (team run id, or the
    /// session id when solo). Requests from unrelated scopes must never
    /// merge onto one card.
    pub scope: String,
    pub product_session_id: String,
    pub request_id: String,
    /// Human label for the requesting session (worker engine, or "lead").
    pub requester: String,
    pub tool_name: String,
    pub resource: Option<String>,
}

/// What a grouped decision actually achieved.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GroupDecisionOutcome {
    pub answered: Vec<PermissionMember>,
    /// Members that could not be answered — surfaced, not logged away.
    pub failures: Vec<String>,
}

/// Equivalent concurrent requests, merged for one decision.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PermissionGroup {
    pub group_key: String,
    pub tool_name: String,
    pub members: Vec<PermissionMember>,
}

/// Two requests are equivalent when they come from the same collaboration
/// scope and ask for the same action on the same resource — deliberately NOT
/// "same tool", which would merge a write to one file with a write to
/// another, or `rm -rf` with `ls`.
///
/// A request whose resource is unknown is NEVER equivalent to anything: it
/// gets a key of its own, so an un-inspectable action can never ride along
/// on someone else's approval.
pub fn permission_group_key(
    scope: &str,
    tool_name: &str,
    resource: Option<&str>,
    request_id: &str,
) -> String {
    match resource {
        Some(resource) => format!("{scope}::{tool_name}::{resource}"),
        None => format!("{scope}::{tool_name}::unknown::{request_id}"),
    }
}

/// Routes user permission decisions back to the exact engine session that
/// emitted the request, even after fallback replaced the active engine.
/// Entries are single-use: a delivered decision cannot be replayed.
#[derive(Default)]
pub struct PermissionRouter {
    pending: Mutex<HashMap<(String, String), PendingPermission>>,
}

/// A permission response's backend outcome, surfaced to the UI so a stale
/// approval card can be told apart from a genuine delivery failure (ticket 03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAnswerOutcome {
    /// The decision was routed to the engine (or the engine confirmed it was
    /// already denied on its side).
    Delivered,
    /// The request is no longer answerable — already answered, the attempt
    /// ended, or the engine auto-denied it before the user replied.
    Expired,
}

/// Whether a `respond` error string means "this approval is no longer
/// actionable" rather than a genuine failure. The router miss text and the
/// engine's `PermissionNotFound` / `SessionNotFound` displays are the stale
/// class; everything else (process errors, timeouts) is a real failure.
pub fn is_stale_permission_error(error: &str) -> bool {
    error.contains("is not pending for Hub session")
        || error.contains("Permission request not found:")
        || error.contains("Session not found:")
}

#[cfg(test)]
mod stale_tests {
    use super::*;

    #[test]
    fn classifies_stale_and_genuine_messages() {
        // Stale class: the engine already auto-denied / the attempt ended.
        assert!(is_stale_permission_error(
            "permission request 'req-1' is not pending for Hub session 'p-1'; \
             it may already be answered or the attempt ended"
        ));
        assert!(is_stale_permission_error("Permission request not found: req-1"));
        assert!(is_stale_permission_error("Session not found: native-1"));
        // Genuine failures must not be labeled expired.
        assert!(!is_stale_permission_error("Process error: codex exited with 1"));
        assert!(!is_stale_permission_error("Bridge process failed"));
    }
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
        member: PermissionMember,
    ) {
        self.pending
            .lock()
            .expect("permission router poisoned")
            .insert(
                (product_session_id.to_string(), request_id.to_string()),
                PendingPermission {
                    engine,
                    engine_session_id: engine_session_id.to_string(),
                    member,
                },
            );
    }

    /// Pending requests, merged by equivalence. Ordering is stable so the
    /// card does not reshuffle under the user's cursor.
    pub fn groups(&self) -> Vec<PermissionGroup> {
        let pending = self.pending.lock().expect("permission router poisoned");
        let mut groups: Vec<PermissionGroup> = Vec::new();
        let mut members: Vec<&PermissionMember> =
            pending.values().map(|entry| &entry.member).collect();
        members.sort_by(|left, right| {
            left.tool_name
                .cmp(&right.tool_name)
                .then(left.resource.cmp(&right.resource))
                .then(left.requester.cmp(&right.requester))
                .then(left.request_id.cmp(&right.request_id))
        });
        for member in members {
            let key = permission_group_key(
                &member.scope,
                &member.tool_name,
                member.resource.as_deref(),
                &member.request_id,
            );
            match groups.iter_mut().find(|group| group.group_key == key) {
                Some(group) => group.members.push(member.clone()),
                None => groups.push(PermissionGroup {
                    group_key: key,
                    tool_name: member.tool_name.clone(),
                    members: vec![member.clone()],
                }),
            }
        }
        groups
    }

    /// Every pending request that belongs to one equivalence group.
    pub fn group_members(&self, group_key: &str) -> Vec<PermissionMember> {
        self.groups()
            .into_iter()
            .find(|group| group.group_key == group_key)
            .map(|group| group.members)
            .unwrap_or_default()
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

    /// Deliver one decision to several equivalent requests. Returns the
    /// members it actually answered, so the caller can audit precisely what
    /// the choice covered; a member that vanished mid-decision is reported
    /// rather than silently dropped.
    pub async fn respond_many(
        &self,
        members: &[PermissionMember],
        allow: bool,
        deny_message: Option<String>,
    ) -> Result<GroupDecisionOutcome, String> {
        let mut answered = Vec::new();
        let mut failures = Vec::new();
        for member in members {
            let decision = if allow {
                PermissionDecision::Allow {
                    updated_input: None,
                }
            } else {
                PermissionDecision::Deny {
                    message: deny_message
                        .clone()
                        .unwrap_or_else(|| "Denied by the user".to_string()),
                }
            };
            match self
                .respond(&member.product_session_id, &member.request_id, decision)
                .await
            {
                Ok(()) => answered.push(member.clone()),
                Err(error) => failures.push(format!("{}: {error}", member.requester)),
            }
        }
        if answered.is_empty() && !failures.is_empty() {
            return Err(failures.join("; "));
        }
        // A partially-applied decision is reported, never swallowed: some
        // engines were told and others were not.
        Ok(GroupDecisionOutcome { answered, failures })
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

/// Release every engine handle a single run registered, together with that
/// session's pending approvals.
///
/// Callers must invoke this on **both** the success and the failure path. It is
/// keyed off the ids the run actually registered rather than off the run's
/// outcome, because the failure path has no outcome to read a session id from —
/// that is exactly why hub-server's old cleanup could not run there, leaving a
/// dead adapter in the map that a later stop would reach and report success on.
pub fn release_run_engines(
    engines: &std::sync::Mutex<std::collections::HashMap<String, Arc<dyn EngineRuntime>>>,
    router: &PermissionRouter,
    ids: impl IntoIterator<Item = String>,
) {
    let mut engines = engines
        .lock()
        .expect("Hub active-engine registry lock poisoned");
    for id in ids {
        engines.remove(&id);
        router.clear_session(&id);
    }
}

/// RAII lease over the engine handles a single run registers.
///
/// Cleanup must survive three exits, not one: the success path, the failure
/// path, and **the run's future being dropped** — an axum handler is cancelled
/// whenever the client disconnects mid-run, and a plain statement after the
/// `.await` never executes in that case. Holding the lease for the duration of
/// the run turns "released exactly once" into a structural property instead of
/// something the next maintainer has to remember.
pub struct RunEngineLease {
    engines: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<dyn EngineRuntime>>>>,
    router: Arc<PermissionRouter>,
    ids: Arc<std::sync::Mutex<Vec<String>>>,
}

impl RunEngineLease {
    pub fn new(
        engines: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<dyn EngineRuntime>>>>,
        router: Arc<PermissionRouter>,
    ) -> Self {
        Self {
            engines,
            router,
            ids: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Handle for the engine-build closure to record what it registered.
    pub fn registry(&self) -> Arc<std::sync::Mutex<Vec<String>>> {
        Arc::clone(&self.ids)
    }
}

impl Drop for RunEngineLease {
    fn drop(&mut self) {
        // Take the ids out first so the registry lock is released before the
        // engines lock is acquired; the build closure takes them in the other
        // order (engines, then registry) as two separate statements.
        let ids: Vec<String> = {
            let mut guard = self
                .ids
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *guard)
        };
        if ids.is_empty() {
            return;
        }
        release_run_engines(&self.engines, &self.router, ids);
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

    fn member(
        session: &str,
        request: &str,
        requester: &str,
        resource: Option<&str>,
    ) -> PermissionMember {
        PermissionMember {
            scope: "run-1".into(),
            product_session_id: session.into(),
            request_id: request.into(),
            requester: requester.into(),
            tool_name: "write_file".into(),
            resource: resource.map(String::from),
        }
    }

    #[tokio::test]
    async fn one_decision_routes_to_every_member_of_an_equivalence_group() {
        let router = PermissionRouter::new();
        let alva = Arc::new(RecordingEngine::new());
        let codex = Arc::new(RecordingEngine::new());
        // Two workers want to write the SAME file: equivalent.
        router.register(
            "worker-a",
            "req-1",
            Arc::clone(&alva) as Arc<dyn EngineRuntime>,
            "alva-native",
            member("worker-a", "req-1", "worker on alva", Some("src/lib.rs")),
        );
        router.register(
            "worker-b",
            "req-2",
            Arc::clone(&codex) as Arc<dyn EngineRuntime>,
            "codex-native",
            member("worker-b", "req-2", "worker on codex", Some("src/lib.rs")),
        );
        // A third wants a different file: must NOT be merged in.
        router.register(
            "worker-c",
            "req-3",
            Arc::clone(&codex) as Arc<dyn EngineRuntime>,
            "codex-native",
            member("worker-c", "req-3", "worker on codex", Some("src/other.rs")),
        );

        let groups = router.groups();
        assert_eq!(groups.len(), 2);
        let shared = groups
            .iter()
            .find(|group| group.group_key.ends_with("::src/lib.rs"))
            .unwrap();
        assert_eq!(shared.members.len(), 2);
        // The card can name every requester, action and resource.
        let requesters: Vec<_> = shared
            .members
            .iter()
            .map(|member| member.requester.as_str())
            .collect();
        assert!(requesters.contains(&"worker on alva"));
        assert!(requesters.contains(&"worker on codex"));

        // One allow answers both members, and only those.
        let outcome = router
            .respond_many(&shared.members.clone(), true, None)
            .await
            .unwrap();
        assert_eq!(outcome.answered.len(), 2);
        assert!(outcome.failures.is_empty());
        assert_eq!(alva.responses.lock().unwrap().len(), 1);
        assert_eq!(codex.responses.lock().unwrap().len(), 1);
        // The unrelated request is still pending and answerable on its own.
        assert_eq!(router.groups().len(), 1);
        assert_eq!(router.group_members(&groups[0].group_key).len(), 0);

        // Members are single-use: replaying the same group is refused.
        let replay = router.respond_many(&shared.members, true, None).await;
        assert!(replay.is_err());
    }

    #[test]
    fn only_same_scope_same_action_same_resource_is_equivalent() {
        assert_eq!(
            permission_group_key("run-1", "write_file", Some("src/lib.rs"), "r1"),
            permission_group_key("run-1", "write_file", Some("src/lib.rs"), "r2")
        );
        // Same tool, different resource: merging these would hide which file
        // the user is approving.
        assert_ne!(
            permission_group_key("run-1", "write_file", Some("src/a.rs"), "r1"),
            permission_group_key("run-1", "write_file", Some("src/b.rs"), "r2")
        );
        // Different tool, same resource.
        assert_ne!(
            permission_group_key("run-1", "write_file", Some("src/a.rs"), "r1"),
            permission_group_key("run-1", "run_command", Some("src/a.rs"), "r2")
        );
        // Different collaboration scope never merges.
        assert_ne!(
            permission_group_key("run-1", "write_file", Some("src/a.rs"), "r1"),
            permission_group_key("run-2", "write_file", Some("src/a.rs"), "r2")
        );
        // UNKNOWN resource is never equivalent to anything, including
        // another unknown: `rm -rf` must not ride along on `ls`.
        assert_ne!(
            permission_group_key("run-1", "execute_shell", None, "r1"),
            permission_group_key("run-1", "execute_shell", None, "r2")
        );
    }

    #[tokio::test]
    async fn requests_with_unknown_resources_never_merge_onto_one_card() {
        let router = PermissionRouter::new();
        let engine = Arc::new(RecordingEngine::new());
        for (index, request) in ["req-rm", "req-ls"].iter().enumerate() {
            router.register(
                "worker-a",
                request,
                Arc::clone(&engine) as Arc<dyn EngineRuntime>,
                "native",
                PermissionMember {
                    scope: "run-1".into(),
                    product_session_id: "worker-a".into(),
                    request_id: (*request).into(),
                    requester: format!("worker {index}"),
                    tool_name: "execute_shell".into(),
                    // The adapter gave us no inspectable resource.
                    resource: None,
                },
            );
        }
        // Two separate cards: one approval can never cover both commands.
        let groups = router.groups();
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.members.len() == 1));
    }

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
            ("pi", PermissionProfile::Interactive, None),
            (
                "pi",
                PermissionProfile::WorkspaceWrite,
                Some((Stricter, EngineEnforcement::PiReadOnly)),
            ),
            (
                "pi",
                PermissionProfile::Unrestricted,
                Some((Equivalent, EngineEnforcement::PiUnrestricted)),
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
            vec!["alva", "claude", "codex", "pi"]
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

    /// A run's engine handle and its pending approvals must be released
    /// together, and this must happen on the failure path too — hub-server's
    /// old cleanup sat after an early `return`, so a turn that failed while
    /// finishing (a SQLite busy timeout in `finish_turn` is the easy one) left a
    /// dead adapter in the map. A later stop on that session then reached the
    /// dead handle and reported success while nothing was actually stopped.
    #[tokio::test]
    async fn releasing_a_run_clears_both_the_handle_and_its_approvals() {
        let engines: StdMutex<std::collections::HashMap<String, Arc<dyn EngineRuntime>>> =
            StdMutex::new(std::collections::HashMap::new());
        let router = PermissionRouter::new();
        let engine = Arc::new(RecordingEngine::new()) as Arc<dyn EngineRuntime>;
        engines
            .lock()
            .unwrap()
            .insert("product-1".into(), Arc::clone(&engine));
        router.register(
            "product-1",
            "req-1",
            Arc::clone(&engine),
            "engine-session-a",
            member("product-1", "req-1", "session", None),
        );
        assert_eq!(
            router.groups().len(),
            1,
            "precondition: one pending approval"
        );

        release_run_engines(&engines, &router, ["product-1".to_string()]);

        assert!(
            engines.lock().unwrap().is_empty(),
            "the handle must not outlive the run that registered it"
        );
        assert!(
            router.groups().is_empty(),
            "the run's pending approvals must be released with it"
        );
    }

    /// An engine that refuses every decision, e.g. because the approval already
    /// timed out on its side and the tool was blocked.
    struct RejectingEngine;

    #[async_trait]
    impl EngineRuntime for RejectingEngine {
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
            _session_id: &str,
            request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            Err(RuntimeError::PermissionNotFound(request_id.to_string()))
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

    /// `answered` is what the callers audit. A member whose engine refused the
    /// decision must never appear there, or the session's audit trail records an
    /// "Allowed ..." for a tool that was in fact never allowed.
    #[tokio::test]
    async fn engine_rejection_keeps_the_member_out_of_answered() {
        let router = PermissionRouter::new();
        let accepting = Arc::new(RecordingEngine::new());
        let rejecting = Arc::new(RejectingEngine);
        let ok_member = member("product-1", "req-ok", "session", None);
        let rejected_member = member("product-2", "req-gone", "session", None);
        router.register(
            "product-1",
            "req-ok",
            Arc::clone(&accepting) as Arc<dyn EngineRuntime>,
            "engine-session-a",
            ok_member.clone(),
        );
        router.register(
            "product-2",
            "req-gone",
            Arc::clone(&rejecting) as Arc<dyn EngineRuntime>,
            "engine-session-b",
            rejected_member.clone(),
        );

        let outcome = router
            .respond_many(&[ok_member, rejected_member], true, None)
            .await
            .expect("a partially answered group still reports its outcome");

        assert_eq!(
            outcome.answered.len(),
            1,
            "only the accepted member may be audited: {:#?}",
            outcome.answered
        );
        assert_eq!(outcome.answered[0].request_id, "req-ok");
        assert_eq!(
            outcome.failures.len(),
            1,
            "the refused member must be surfaced, not dropped"
        );
        // Pin *why* it failed. Without this the test passes even when the engine
        // is never consulted at all (e.g. a router-map miss), which is a
        // different failure with an identical outcome shape.
        assert_eq!(
            outcome.failures[0], "session: Permission request not found: req-gone",
            "the failure must be the engine's refusal, not a router-map miss"
        );
    }

    /// When no engine accepts, the whole call fails so the caller's `?` stops
    /// before it can write any audit line.
    #[tokio::test]
    async fn total_engine_rejection_fails_the_call() {
        let router = PermissionRouter::new();
        let rejecting = Arc::new(RejectingEngine);
        let m = member("product-1", "req-gone", "session", None);
        router.register(
            "product-1",
            "req-gone",
            Arc::clone(&rejecting) as Arc<dyn EngineRuntime>,
            "engine-session-a",
            m.clone(),
        );

        let error = router
            .respond_many(&[m], true, None)
            .await
            .expect_err("a fully refused group must fail the call");
        assert!(
            error.contains("Permission request not found"),
            "the error must name the engine's refusal, got {error:?}"
        );
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
            member("product-1", "req-1", "session", None),
        );
        // A later attempt on another engine must not steal the decision.
        router.register(
            "product-1",
            "req-2",
            Arc::clone(&fallback_engine) as Arc<dyn EngineRuntime>,
            "engine-session-b",
            member("product-1", "req-2", "session", None),
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
            member("product-1", "req-1", "session", None),
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
            member("product-1", "req-1", "session", None),
        );
        router.register(
            "product-2",
            "req-1",
            Arc::clone(&engine) as Arc<dyn EngineRuntime>,
            "engine-session-b",
            member("product-2", "req-1", "session", None),
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
