// INPUT:  alva_engine_runtime, alva_hub_core, HubStore, serde, tokio_stream
// OUTPUT: multi-engine run seams, delta/full handoff injection, Attempt capture, timeline DTOs
// POS:    Highest product seam for durable Hub Desktop branch execution and native-resume recovery.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use alva_engine_runtime::{EngineRuntime, RuntimeEvent, RuntimeRequest, RuntimeUsage};
use alva_hub_core::{HubRequest, HubRuntime, HubSessionStatus, ProductSessionId};
use alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

use crate::failure::{classify_failure, triggers_fallback, FailureClass};
use crate::permissions::{enforcement_for, PermissionProfile};
use crate::readiness::capability_labels;
use crate::session_workspace::workspace_kind_for_path;
use crate::store::{EngineTarget, HubStore, PreparedTurn, SideEffectEvidence, StoredUsage};

/// No-activity window after which an armed watchdog declares a stall. The
/// timer never runs while a tool is open or a user approval is pending.
const STALL_WINDOW: std::time::Duration = std::time::Duration::from_secs(180);

fn default_engine_id() -> String {
    "alva".to_string()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RunRequest {
    pub prompt: String,
    pub workspace: PathBuf,
    #[serde(default = "default_engine_id")]
    pub engine_id: String,
    #[serde(default)]
    pub product_session_id: Option<String>,
    /// Permission contract for a NEW session (defaults to workspace-write).
    /// On resume it may only restate the stored profile — a mismatch fails.
    #[serde(default)]
    pub permission_profile: Option<PermissionProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineStatus {
    UserMessage,
    AssistantMessage,
    ActivityStarted,
    ActivityCompleted,
    Running,
    Completed,
    Failed,
    Interrupted,
    Fallback,
    Exhausted,
    PermissionRequired,
    Worker,
}

/// Routing identity of a pending engine permission request. The engine
/// session id pins the approval to the exact native session that asked, so a
/// decision can never leak into a later attempt or fallback engine.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PermissionRequestInfo {
    pub request_id: String,
    pub tool_name: String,
    pub description: Option<String>,
    pub engine_session_id: String,
    /// The resource the tool is asking to act on, when the adapter names
    /// one. Two requests are only equivalent if this matches.
    #[serde(default)]
    pub resource: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TimelineEvent {
    pub status: TimelineStatus,
    pub engine_id: String,
    pub product_session_id: String,
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<TimelineActivity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_request: Option<PermissionRequestInfo>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TimelineActivity {
    pub tool_name: String,
    pub succeeded: Option<bool>,
    pub duration_ms: Option<u64>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunOutcome {
    pub product_session_id: String,
    pub result: Option<String>,
    pub failed: bool,
}

struct ExecutedTurn {
    outcome: RunOutcome,
    engine_session_id: Option<String>,
    terminal_error: Option<String>,
    usage: Option<StoredUsage>,
    /// Stable classification of the terminal failure (engine start errors and
    /// fatal stream errors). Persistence failures and successful results —
    /// regardless of quality — never carry a class.
    failure_class: Option<FailureClass>,
}

impl ExecutedTurn {
    fn fallback_class(&self) -> Option<FailureClass> {
        self.failure_class.filter(|class| triggers_fallback(*class))
    }
}

pub async fn run_once<F>(
    engine: Arc<dyn EngineRuntime>,
    request: RunRequest,
    mut emit: F,
) -> Result<RunOutcome, String>
where
    F: FnMut(TimelineEvent) + Send,
{
    validate_prompt(&request.prompt)?;
    let workspace = canonical_workspace(&request.workspace)?;
    let target = EngineTarget::for_engine(&request.engine_id)?;
    let hub = HubRuntime::new();
    hub.register_engine(target.engine_id.as_str(), engine)
        .map_err(|error| error.to_string())?;
    let product_session_id = ProductSessionId::new();
    Ok(execute_turn(
        &hub,
        product_session_id,
        request.prompt,
        workspace,
        &target.engine_id,
        None,
        &mut emit,
    )
    .await
    .outcome)
}

pub async fn run_persisted<F, B, H>(
    store: &HubStore,
    build_engine: B,
    health_veto: H,
    request: RunRequest,
    emit: F,
) -> Result<RunOutcome, String>
where
    F: FnMut(TimelineEvent) + Send,
    B: Fn(&EngineTarget, &str, PermissionProfile) -> Result<Arc<dyn EngineRuntime>, String>,
    H: Fn(&EngineTarget) -> Option<String>,
{
    run_persisted_with_target(store, build_engine, health_veto, request, None, emit).await
}

/// Desktop route-aware variant. Existing callers keep `run_persisted` and
/// therefore retain the engine-default behavior.
pub async fn run_persisted_with_target<F, B, H>(
    store: &HubStore,
    build_engine: B,
    health_veto: H,
    request: RunRequest,
    selected_target: Option<EngineTarget>,
    mut emit: F,
) -> Result<RunOutcome, String>
where
    F: FnMut(TimelineEvent) + Send,
    B: Fn(&EngineTarget, &str, PermissionProfile) -> Result<Arc<dyn EngineRuntime>, String>,
    H: Fn(&EngineTarget) -> Option<String>,
{
    validate_prompt(&request.prompt)?;

    let prepared = if let Some(product_session_id) = request.product_session_id.as_deref() {
        let detail = store
            .get_session(product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        canonical_workspace(PathBuf::from(&detail.session.workspace).as_path())?;
        if request.engine_id != detail.session.engine_id {
            return Err(format!(
                "Hub session '{product_session_id}' is active on '{}'; switch engines before running '{}'",
                detail.session.engine_id, request.engine_id
            ));
        }
        // The profile is session-stable: a resume may restate it but never
        // change it.
        if let Some(requested) = request.permission_profile {
            if requested != detail.session.permission_profile {
                return Err(format!(
                    "Hub session '{product_session_id}' uses permission profile '{}'; it cannot be changed to '{}' after creation",
                    detail.session.permission_profile.as_str(),
                    requested.as_str()
                ));
            }
        }
        store.prepare_resumed_turn(product_session_id, &request.prompt)?
    } else {
        let workspace = canonical_workspace(&request.workspace)?;
        let product_session_id = ProductSessionId::new();
        let target = selected_target.unwrap_or(EngineTarget::for_engine(&request.engine_id)?);
        if target.engine_id != request.engine_id {
            return Err(format!(
                "selected route uses engine '{}' but the run request names '{}'",
                target.engine_id, request.engine_id
            ));
        }
        store.prepare_new_turn_with_workspace_kind(
            product_session_id.as_str(),
            &workspace,
            &request.prompt,
            &target,
            request.permission_profile.unwrap_or_default(),
            workspace_kind_for_path(&workspace),
        )?
    };

    let mut prepared = prepared;
    let mut execution = match build_engine(
        &prepared.target,
        &prepared.product_session_id,
        prepared.permission_profile,
    ) {
        Ok(engine) => {
            execute_persisted_turn(store, engine, &prepared, &request.prompt, &mut emit).await?
        }
        Err(error) => start_failure_execution(&prepared, error, &mut emit),
    };
    if prepared.native_resume
        && execution
            .terminal_error
            .as_deref()
            .is_some_and(is_invalid_native_resume_error)
    {
        let failure_reason = execution
            .terminal_error
            .as_deref()
            .expect("native resume failure was checked above");
        prepared = store.upgrade_failed_native_resume(&prepared, failure_reason)?;
        execution = match build_engine(
            &prepared.target,
            &prepared.product_session_id,
            prepared.permission_profile,
        ) {
            Ok(engine) => {
                execute_persisted_turn(store, engine, &prepared, &request.prompt, &mut emit).await?
            }
            Err(error) => start_failure_execution(&prepared, error, &mut emit),
        };
    }

    // Bounded automatic fallback: a classified operational continuity failure
    // continues the same Turn on the next policy target the user authorized
    // for automatic use. Each target (engine/model/provider triple) is
    // considered at most once per attempt cycle, in policy order, with no
    // loop-back; unhealthy or capability-incompatible targets are skipped
    // with a recorded reason. When the chain runs dry an exhaustion card
    // lists every attempt's terminal state.
    let mut considered = vec![prepared.target.clone()];
    let mut cycle_log: Vec<String> = Vec::new();
    while let Some(class) = execution.fallback_class() {
        let failure_reason = execution
            .terminal_error
            .clone()
            .unwrap_or_else(|| "the engine terminated without a result".to_string());
        cycle_log.push(format!(
            "{}: {} — {}",
            prepared.target.engine_id,
            class.label(),
            brief(&failure_reason)
        ));

        let mut selected = None;
        while let Some(candidate) =
            store.next_authorized_fallback_target(&prepared.product_session_id, &considered)?
        {
            considered.push(candidate.clone());
            // Permission contract first: continuity never runs a target that
            // would weaken the session profile, no matter how healthy it is.
            if let Err(reason) = enforcement_for(&candidate.engine_id, prepared.permission_profile)
            {
                store.record_fallback_skip(
                    &prepared.product_session_id,
                    &prepared.branch_id,
                    &prepared.target.engine_id,
                    &candidate,
                    &reason,
                )?;
                cycle_log.push(format!(
                    "{}: skipped — {}",
                    candidate.engine_id,
                    brief(&reason)
                ));
                continue;
            }
            if let Some(veto) = health_veto(&candidate) {
                store.record_fallback_skip(
                    &prepared.product_session_id,
                    &prepared.branch_id,
                    &prepared.target.engine_id,
                    &candidate,
                    &veto,
                )?;
                cycle_log.push(format!(
                    "{}: skipped — {}",
                    candidate.engine_id,
                    brief(&veto)
                ));
                continue;
            }
            let built = build_engine(
                &candidate,
                &prepared.product_session_id,
                prepared.permission_profile,
            );
            if let Ok(engine) = &built {
                let available = capability_labels(&engine.capabilities());
                let missing: Vec<String> = candidate
                    .required_capabilities
                    .iter()
                    .filter(|capability| !available.contains(capability))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    let veto =
                        format!("target lacks required capabilities: {}", missing.join(", "));
                    store.record_fallback_skip(
                        &prepared.product_session_id,
                        &prepared.branch_id,
                        &prepared.target.engine_id,
                        &candidate,
                        &veto,
                    )?;
                    cycle_log.push(format!(
                        "{}: skipped — {}",
                        candidate.engine_id,
                        brief(&veto)
                    ));
                    continue;
                }
            }
            selected = Some((candidate, built));
            break;
        }
        let Some((target, built)) = selected else {
            break;
        };
        let source_engine_id = prepared.target.engine_id.clone();
        prepared = store.fallback_failed_attempt(&prepared, &failure_reason, &target)?;
        let handoff_summary = prepared
            .handoff
            .as_ref()
            .map(|handoff| handoff.fallback_summary())
            .unwrap_or_else(|| "full context".to_string());
        emit(TimelineEvent {
            status: TimelineStatus::Fallback,
            engine_id: target.engine_id.clone(),
            product_session_id: prepared.product_session_id.clone(),
            message: Some(format!(
                "{source_engine_id} → {}\n\n{failure_reason}\n\nHandoff: {handoff_summary}",
                target.engine_id
            )),
            item_id: Some(prepared.attempt_id.clone()),
            activity: None,
            permission_request: None,
        });
        execution = match built {
            Ok(engine) => {
                // The handoff counts as delivered only once the target engine
                // actually starts executing with it; on a start failure it
                // stays pending for recovery.
                if let Some(handoff) = &prepared.handoff {
                    store.mark_handoff_used(&handoff.handoff_id)?;
                }
                execute_persisted_turn(store, engine, &prepared, &request.prompt, &mut emit).await?
            }
            Err(error) => start_failure_execution(&prepared, error, &mut emit),
        };
    }

    // The chain engaged (attempted or skipped at least one candidate) but the
    // turn still ends failed: record one card listing every terminal state.
    // A final unclassified failure is not in the log yet — the loop exited on
    // it — so it is appended here.
    if execution.terminal_error.is_some() && considered.len() > 1 {
        if execution.fallback_class().is_none() {
            if let Some(error) = &execution.terminal_error {
                let label = execution
                    .failure_class
                    .map(|class| class.label())
                    .unwrap_or("failure");
                cycle_log.push(format!(
                    "{}: {} — {}",
                    prepared.target.engine_id,
                    label,
                    brief(error)
                ));
            }
        }
        let title = if execution.fallback_class().is_some() {
            "Fallback chain exhausted"
        } else {
            "Fallback stopped — the last failure is not eligible for automatic fallback"
        };
        let summary = format!("{title}\n{}", cycle_log.join("\n"));
        store.record_fallback_exhaustion(
            &prepared.product_session_id,
            &prepared.branch_id,
            &prepared.target.engine_id,
            &summary,
        )?;
        emit(TimelineEvent {
            status: TimelineStatus::Exhausted,
            engine_id: prepared.target.engine_id.clone(),
            product_session_id: prepared.product_session_id.clone(),
            message: Some(summary),
            item_id: Some(prepared.attempt_id.clone()),
            activity: None,
            permission_request: None,
        });
    }

    store.finish_turn(
        &prepared,
        execution.engine_session_id.as_deref(),
        execution.outcome.result.as_deref(),
        execution.terminal_error.as_deref(),
        execution.usage.as_ref(),
    )?;
    store.apply_pending_switch(&prepared.product_session_id)?;
    Ok(execution.outcome)
}

/// The engine could not even be assembled — a synchronous start failure. The
/// prepared attempt, parked source branch, and persisted handoff all survive,
/// so the session stays recoverable.
fn start_failure_execution<F>(prepared: &PreparedTurn, error: String, emit: &mut F) -> ExecutedTurn
where
    F: FnMut(TimelineEvent) + Send,
{
    let message = format!("start Hub engine '{}': {error}", prepared.target.engine_id);
    emit(TimelineEvent {
        status: TimelineStatus::Failed,
        engine_id: prepared.target.engine_id.clone(),
        product_session_id: prepared.product_session_id.clone(),
        message: Some(message.clone()),
        item_id: Some(prepared.attempt_id.clone()),
        activity: None,
        permission_request: None,
    });
    ExecutedTurn {
        outcome: RunOutcome {
            product_session_id: prepared.product_session_id.clone(),
            result: None,
            failed: true,
        },
        engine_session_id: None,
        failure_class: Some(classify_failure(&message)),
        terminal_error: Some(message),
        usage: None,
    }
}

/// File path a tool call names in its input, across the field spellings the
/// three adapters use.
fn tool_input_path(input: &serde_json::Value) -> Option<String> {
    for field in ["path", "file_path", "filePath", "filename"] {
        if let Some(path) = input.get(field).and_then(serde_json::Value::as_str) {
            return Some(path.to_string());
        }
    }
    None
}

/// What a permission request is asking to act ON — a path, or the command
/// itself for shell-shaped tools. Approval grouping keys off this, so a
/// missing value must mean "cannot be judged equivalent", never "same".
fn permission_resource(input: &serde_json::Value) -> Option<String> {
    if let Some(path) = tool_input_path(input) {
        return Some(path);
    }
    for field in ["command", "cmd", "script", "query", "url"] {
        if let Some(value) = input.get(field).and_then(serde_json::Value::as_str) {
            return Some(value.to_string());
        }
    }
    None
}

/// Bounded excerpt for timeline log lines.
fn brief(text: &str) -> String {
    let text = text.trim().replace('\n', " ");
    if text.chars().count() <= 160 {
        text
    } else {
        let mut excerpt: String = text.chars().take(159).collect();
        excerpt.push('…');
        excerpt
    }
}

async fn execute_persisted_turn<F>(
    store: &HubStore,
    engine: Arc<dyn EngineRuntime>,
    prepared: &PreparedTurn,
    user_prompt: &str,
    emit: &mut F,
) -> Result<ExecutedTurn, String>
where
    F: FnMut(TimelineEvent) + Send,
{
    let hub = HubRuntime::new();
    hub.register_engine(prepared.target.engine_id.as_str(), engine)
        .map_err(|error| error.to_string())?;
    let product_session_id = ProductSessionId::from_existing(&prepared.product_session_id);
    if let Some(engine_session_id) = &prepared.engine_session_id {
        hub.restore_session(
            product_session_id.clone(),
            prepared.target.engine_id.as_str(),
            engine_session_id,
            HubSessionStatus::Completed,
        )
        .map_err(|error| error.to_string())?;
    }
    let prompt = prepared
        .handoff
        .as_ref()
        .map(|handoff| {
            format!(
                "{}\n\n<current-user-instruction>\n{}\n</current-user-instruction>",
                handoff.render_for_engine(),
                user_prompt
            )
        })
        .unwrap_or_else(|| user_prompt.to_string());
    Ok(execute_turn(
        &hub,
        product_session_id,
        prompt,
        canonical_workspace(&prepared.workspace)?,
        &prepared.target.engine_id,
        Some((store, prepared)),
        emit,
    )
    .await)
}

fn is_invalid_native_resume_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("session not found")
        || error.contains("invalid session")
        || error.contains("unknown session")
        || (error.contains("resume") && error.contains("session"))
}

async fn execute_turn<F>(
    hub: &HubRuntime,
    product_session_id: ProductSessionId,
    prompt: String,
    workspace: PathBuf,
    engine_id: &str,
    persistence: Option<(&HubStore, &PreparedTurn)>,
    emit: &mut F,
) -> ExecutedTurn
where
    F: FnMut(TimelineEvent) + Send,
{
    let product_id = product_session_id.as_str().to_string();
    emit(TimelineEvent {
        status: TimelineStatus::UserMessage,
        engine_id: engine_id.to_string(),
        product_session_id: product_id.clone(),
        message: Some(prompt.clone()),
        item_id: persistence.map(|(_, prepared)| format!("{}:user", prepared.turn_id)),
        activity: None,
        permission_request: None,
    });
    let runtime_request = RuntimeRequest::new(prompt)
        .with_cwd(workspace)
        .with_streaming();
    let hub_request = HubRequest::new(engine_id, runtime_request).resume(product_session_id);
    let mut stream = match hub.execute(hub_request) {
        Ok(stream) => stream,
        Err(error) => {
            let message = error.to_string();
            emit(TimelineEvent {
                status: TimelineStatus::Failed,
                engine_id: engine_id.to_string(),
                product_session_id: product_id.clone(),
                message: Some(message.clone()),
                item_id: None,
                activity: None,
                permission_request: None,
            });
            return ExecutedTurn {
                outcome: RunOutcome {
                    product_session_id: product_id,
                    result: None,
                    failed: true,
                },
                engine_session_id: None,
                failure_class: Some(classify_failure(&message)),
                terminal_error: Some(message),
                usage: None,
            };
        }
    };

    let mut terminal_error = None;
    let mut result = None;
    let mut engine_session_id = None;
    let mut usage = None;
    let mut failure_class = None;
    // Stall watchdog state: the timer only runs while nothing is legitimately
    // in flight. Open tools and a pending user approval disarm it; every
    // received event (including streamed deltas) resets the window. Once the
    // attempt carries a terminal error the loop is only draining the stream,
    // so the window always applies — a doomed attempt must not wait forever
    // on an approval nobody can answer anymore.
    let mut open_tools: HashSet<String> = HashSet::new();
    let mut delta_message_ids: HashSet<String> = HashSet::new();
    let mut last_assistant_message_id: Option<String> = None;
    let mut permission_pending = false;
    loop {
        let armed = terminal_error.is_some() || (open_tools.is_empty() && !permission_pending);
        let next = if armed {
            match tokio::time::timeout(STALL_WINDOW, stream.next()).await {
                Ok(next) => next,
                Err(_elapsed) => {
                    let message = format!(
                        "engine stalled: no activity for {}s",
                        STALL_WINDOW.as_secs()
                    );
                    if terminal_error.is_none() {
                        emit(TimelineEvent {
                            status: TimelineStatus::Failed,
                            engine_id: engine_id.to_string(),
                            product_session_id: product_id.clone(),
                            message: Some(message.clone()),
                            item_id: persistence.map(|(_, prepared)| prepared.attempt_id.clone()),
                            activity: None,
                            permission_request: None,
                        });
                        failure_class = Some(FailureClass::Stall);
                        terminal_error = Some(message);
                    }
                    // Best-effort: ask the engine to stop the stalled run
                    // before the branch is abandoned or fallback engages.
                    if let Err(error) = hub
                        .cancel(&ProductSessionId::from_existing(&product_id))
                        .await
                    {
                        tracing::warn!(%error, "failed to cancel stalled Hub engine");
                    }
                    break;
                }
            }
        } else {
            stream.next().await
        };
        let Some(hub_event) = next else {
            break;
        };
        // Only events that evidence the approval was resolved clear the
        // pending flag — an unrelated ToolEnd or a streamed delta from a
        // concurrent tool must not re-arm the watchdog while the user is
        // still deciding.
        if matches!(
            &hub_event.event,
            RuntimeEvent::SessionStarted { .. }
                | RuntimeEvent::Message { .. }
                | RuntimeEvent::ToolStart { .. }
                | RuntimeEvent::Completed { .. }
                | RuntimeEvent::Error { .. }
        ) {
            permission_pending = false;
        }
        match hub_event.event {
            RuntimeEvent::PermissionRequest {
                request_id,
                tool_name,
                description,
                tool_input,
            } => {
                // Surface the request with its engine-session routing identity
                // so the Desktop layer can deliver the decision to the exact
                // native session that asked — never to a later fallback engine.
                // Without a native session id the request is unanswerable, so
                // no controls are attached and the watchdog stays armed — the
                // dead wait then resolves as a bounded stall instead of a hang.
                match engine_session_id
                    .clone()
                    .filter(|id: &String| !id.is_empty())
                {
                    Some(routable_session_id) => {
                        permission_pending = true;
                        emit(TimelineEvent {
                            status: TimelineStatus::PermissionRequired,
                            engine_id: hub_event.engine_id.to_string(),
                            product_session_id: product_id.clone(),
                            message: Some(format!("'{tool_name}' is waiting for your approval")),
                            item_id: Some(request_id.clone()),
                            activity: None,
                            permission_request: Some(PermissionRequestInfo {
                                request_id,
                                tool_name,
                                description,
                                engine_session_id: routable_session_id,
                                resource: permission_resource(&tool_input),
                            }),
                        });
                    }
                    None => {
                        emit(TimelineEvent {
                            status: TimelineStatus::PermissionRequired,
                            engine_id: hub_event.engine_id.to_string(),
                            product_session_id: product_id.clone(),
                            message: Some(format!(
                                "'{tool_name}' requested permission before its engine session \
                                 started; the request cannot be answered"
                            )),
                            item_id: Some(request_id),
                            activity: None,
                            permission_request: None,
                        });
                    }
                }
            }
            RuntimeEvent::SessionStarted { session_id, .. } => {
                if session_id.trim().is_empty() {
                    let message = "engine started without a native session id".to_string();
                    if terminal_error.is_none() {
                        emit(TimelineEvent {
                            status: TimelineStatus::Failed,
                            engine_id: hub_event.engine_id.to_string(),
                            product_session_id: product_id.clone(),
                            message: Some(message.clone()),
                            item_id: persistence.map(|(_, prepared)| prepared.attempt_id.clone()),
                            activity: None,
                            permission_request: None,
                        });
                        terminal_error = Some(message);
                    }
                } else {
                    if let Some((store, prepared)) = persistence {
                        capture_persistence_error(
                            store.record_engine_session(prepared, &session_id),
                            &mut terminal_error,
                            &product_id,
                            engine_id,
                            emit,
                        );
                    }
                    if terminal_error.is_none() {
                        engine_session_id = Some(session_id);
                        emit(TimelineEvent {
                            status: TimelineStatus::Running,
                            engine_id: hub_event.engine_id.to_string(),
                            product_session_id: product_id.clone(),
                            message: None,
                            item_id: persistence.map(|(_, prepared)| prepared.turn_id.clone()),
                            activity: None,
                            permission_request: None,
                        });
                    }
                }
            }
            RuntimeEvent::Message { id, role, content } => {
                if role == MessageRole::Assistant {
                    last_assistant_message_id = Some(id.clone());
                }
                if role == MessageRole::Assistant && !delta_message_ids.contains(&id) {
                    let text = content
                        .iter()
                        .filter_map(ContentBlock::as_text)
                        .collect::<String>();
                    if !text.is_empty() {
                        emit(TimelineEvent {
                            status: TimelineStatus::AssistantMessage,
                            engine_id: hub_event.engine_id.to_string(),
                            product_session_id: product_id.clone(),
                            message: Some(text),
                            item_id: Some(id),
                            activity: None,
                            permission_request: None,
                        });
                    }
                }
            }
            RuntimeEvent::MessageDelta {
                id,
                delta: StreamEvent::TextDelta { text },
            } if !text.is_empty() => {
                last_assistant_message_id = Some(id.clone());
                delta_message_ids.insert(id.clone());
                emit(TimelineEvent {
                    status: TimelineStatus::AssistantMessage,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: Some(text),
                    item_id: Some(id),
                    activity: None,
                    permission_request: None,
                });
            }
            RuntimeEvent::ToolStart { id, name, input } => {
                open_tools.insert(id.clone());
                let path = tool_input_path(&input);
                emit(TimelineEvent {
                    status: TimelineStatus::ActivityStarted,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: Some(format!("{name} running")),
                    item_id: Some(id.clone()),
                    activity: Some(TimelineActivity {
                        tool_name: name.clone(),
                        succeeded: None,
                        duration_ms: None,
                        path: path.clone(),
                    }),
                    permission_request: None,
                });
                if let Some((store, prepared)) = persistence {
                    capture_persistence_error(
                        store.record_side_effect(
                            prepared,
                            SideEffectEvidence {
                                tool_call_id: id,
                                tool_name: name,
                                succeeded: None,
                                duration_ms: None,
                                path,
                            },
                        ),
                        &mut terminal_error,
                        &product_id,
                        engine_id,
                        emit,
                    );
                }
            }
            RuntimeEvent::ToolEnd {
                id,
                name,
                result: tool_result,
                duration_ms,
            } => {
                open_tools.remove(&id);
                let succeeded = !tool_result.is_error;
                emit(TimelineEvent {
                    status: TimelineStatus::ActivityCompleted,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: Some(if succeeded {
                        format!("{name} completed")
                    } else {
                        format!("{name} failed")
                    }),
                    item_id: Some(id.clone()),
                    activity: Some(TimelineActivity {
                        tool_name: name.clone(),
                        succeeded: Some(succeeded),
                        duration_ms,
                        path: None,
                    }),
                    permission_request: None,
                });
                if let Some((store, prepared)) = persistence {
                    capture_persistence_error(
                        store.record_side_effect(
                            prepared,
                            SideEffectEvidence {
                                tool_call_id: id,
                                tool_name: name,
                                succeeded: Some(succeeded),
                                duration_ms,
                                path: None,
                            },
                        ),
                        &mut terminal_error,
                        &product_id,
                        engine_id,
                        emit,
                    );
                }
            }
            RuntimeEvent::Error {
                message,
                recoverable: false,
            } => {
                if terminal_error.is_none() {
                    emit(TimelineEvent {
                        status: TimelineStatus::Failed,
                        engine_id: hub_event.engine_id.to_string(),
                        product_session_id: product_id.clone(),
                        message: Some(message.clone()),
                        item_id: persistence.map(|(_, prepared)| prepared.attempt_id.clone()),
                        activity: None,
                        permission_request: None,
                    });
                    failure_class = Some(classify_failure(&message));
                    terminal_error = Some(message);
                }
            }
            RuntimeEvent::Completed {
                session_id,
                result: completed_result,
                usage: completed_usage,
            } if terminal_error.is_none() => {
                if !session_id.is_empty() {
                    engine_session_id = Some(session_id);
                }
                usage = completed_usage.as_ref().map(stored_usage);
                result = completed_result;
                emit(TimelineEvent {
                    status: TimelineStatus::Completed,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: result.clone(),
                    item_id: last_assistant_message_id.clone().or_else(|| {
                        persistence.map(|(_, prepared)| format!("{}:assistant", prepared.turn_id))
                    }),
                    activity: None,
                    permission_request: None,
                });
                break;
            }
            RuntimeEvent::Completed {
                session_id,
                usage: completed_usage,
                ..
            } => {
                if !session_id.is_empty() {
                    engine_session_id = Some(session_id);
                }
                usage = completed_usage.as_ref().map(stored_usage);
                break;
            }
            _ => {}
        }
    }

    ExecutedTurn {
        outcome: RunOutcome {
            product_session_id: product_id,
            result,
            failed: terminal_error.is_some(),
        },
        engine_session_id,
        terminal_error,
        usage,
        failure_class,
    }
}

fn capture_persistence_error<F>(
    result: Result<(), String>,
    terminal_error: &mut Option<String>,
    product_session_id: &str,
    engine_id: &str,
    emit: &mut F,
) where
    F: FnMut(TimelineEvent),
{
    if let Err(error) = result {
        if terminal_error.is_none() {
            let message = format!("persist engine attempt progress: {error}");
            emit(TimelineEvent {
                status: TimelineStatus::Failed,
                engine_id: engine_id.to_string(),
                product_session_id: product_session_id.to_string(),
                message: Some(message.clone()),
                item_id: None,
                activity: None,
                permission_request: None,
            });
            *terminal_error = Some(message);
        }
    }
}

fn stored_usage(usage: &RuntimeUsage) -> StoredUsage {
    StoredUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_cost_usd: usage.total_cost_usd,
        duration_ms: usage.duration_ms,
        num_turns: usage.num_turns,
    }
}

fn validate_prompt(prompt: &str) -> Result<(), String> {
    if prompt.trim().is_empty() {
        Err("prompt must not be empty".into())
    } else {
        Ok(())
    }
}

fn canonical_workspace(workspace: &std::path::Path) -> Result<PathBuf, String> {
    if !workspace.is_dir() {
        return Err(format!(
            "workspace is not a directory: {}",
            workspace.display()
        ));
    }
    workspace
        .canonicalize()
        .map_err(|error| format!("cannot resolve workspace: {error}"))
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::Mutex;

    use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
    use alva_engine_runtime::{
        PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent, RuntimeRequest,
    };
    use async_trait::async_trait;
    use futures_core::Stream;

    use super::*;

    /// Reopen with retry: the previous handle's instance-lock release can
    /// lag behind drop under parallel load (source of a known flake).
    fn reopen(database: &std::path::Path) -> crate::HubStore {
        let mut last = String::new();
        for _ in 0..50 {
            match crate::HubStore::open(database) {
                Ok(store) => return store,
                Err(error) => {
                    last = error;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        panic!("could not reopen {}: {last}", database.display());
    }

    fn single_engine<E: EngineRuntime + 'static>(
        engine: Arc<E>,
    ) -> impl Fn(&EngineTarget, &str, PermissionProfile) -> Result<Arc<dyn EngineRuntime>, String>
    {
        move |_, _, _| Ok(Arc::clone(&engine) as Arc<dyn EngineRuntime>)
    }

    fn healthy(_target: &EngineTarget) -> Option<String> {
        None
    }

    struct FakeEngine {
        events: Vec<RuntimeEvent>,
        requests: Arc<Mutex<Vec<RuntimeRequest>>>,
    }

    impl FakeEngine {
        fn new(events: Vec<RuntimeEvent>) -> Self {
            Self {
                events,
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl EngineRuntime for FakeEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request);
            Ok(Box::pin(tokio_stream::iter(self.events.clone())))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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

    struct ResumeFailureThenFreshEngine {
        requests: Arc<Mutex<Vec<RuntimeRequest>>>,
    }

    #[async_trait]
    impl EngineRuntime for ResumeFailureThenFreshEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request.clone());
            if request.resume_session.is_some() {
                return Err(RuntimeError::SessionNotFound(
                    "expired native session".into(),
                ));
            }
            Ok(Box::pin(tokio_stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "replacement-native".into(),
                    model: None,
                    tools: Vec::new(),
                },
                RuntimeEvent::Completed {
                    session_id: "replacement-native".into(),
                    result: Some("continued after fresh start".into()),
                    usage: None,
                },
            ])))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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
    async fn projects_successful_engine_run_to_timeline() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-1".into(),
                model: Some("fake-model".into()),
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-1".into(),
                result: Some("done".into()),
                usage: None,
            },
        ]));
        let requests = engine.requests.clone();
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "ship it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("done"));
        assert_eq!(
            timeline
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![
                TimelineStatus::UserMessage,
                TimelineStatus::Running,
                TimelineStatus::Completed,
            ]
        );
        assert!(timeline
            .iter()
            .all(|event| event.product_session_id == outcome.product_session_id));

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].prompt, "ship it");
        assert_eq!(
            requests[0].working_directory.as_deref(),
            Some(workspace.path().canonicalize().unwrap().as_path())
        );
        assert!(requests[0].options.streaming);
    }

    #[tokio::test]
    async fn projects_streamed_assistant_text_and_tool_activity() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::MessageDelta {
                id: "assistant-1".into(),
                delta: StreamEvent::TextDelta {
                    text: "Hello ".into(),
                },
            },
            RuntimeEvent::MessageDelta {
                id: "assistant-1".into(),
                delta: StreamEvent::TextDelta {
                    text: "world".into(),
                },
            },
            RuntimeEvent::ToolStart {
                id: "tool-1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "src/lib.rs"}),
            },
            RuntimeEvent::ToolEnd {
                id: "tool-1".into(),
                name: "read_file".into(),
                result: alva_kernel_abi::ToolOutput::text("done"),
                duration_ms: Some(12),
            },
            RuntimeEvent::Completed {
                session_id: "native-activity".into(),
                result: Some("Hello world".into()),
                usage: None,
            },
        ]));
        let mut timeline = Vec::new();

        run_once(
            engine,
            RunRequest {
                prompt: "Inspect it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        let streamed = timeline
            .iter()
            .filter(|event| event.status == TimelineStatus::AssistantMessage)
            .collect::<Vec<_>>();
        assert_eq!(streamed.len(), 2);
        assert!(streamed
            .iter()
            .all(|event| event.item_id.as_deref() == Some("assistant-1")));
        let completed = timeline
            .iter()
            .find(|event| event.status == TimelineStatus::Completed)
            .unwrap();
        assert_eq!(completed.item_id.as_deref(), Some("assistant-1"));
        let completed_activity = timeline
            .iter()
            .find(|event| event.status == TimelineStatus::ActivityCompleted)
            .and_then(|event| event.activity.as_ref())
            .unwrap();
        assert_eq!(completed_activity.tool_name, "read_file");
        assert_eq!(completed_activity.succeeded, Some(true));
        assert_eq!(completed_activity.duration_ms, Some(12));
    }

    #[tokio::test]
    async fn empty_native_session_id_fails_without_advertising_cancellation() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: String::new(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: String::new(),
                result: None,
                usage: None,
            },
        ]));
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "Inspect it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        assert!(timeline
            .iter()
            .any(|event| event.status == TimelineStatus::Failed));
        assert!(!timeline
            .iter()
            .any(|event| event.status == TimelineStatus::Running));
    }

    #[tokio::test]
    async fn projects_terminal_engine_error_as_failed() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-2".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Error {
                message: "provider unavailable".into(),
                recoverable: false,
            },
            RuntimeEvent::Completed {
                session_id: "native-2".into(),
                result: Some("must not surface".into()),
                usage: None,
            },
        ]));
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "ship it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        assert_eq!(outcome.result, None);
        assert_eq!(
            timeline
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![
                TimelineStatus::UserMessage,
                TimelineStatus::Running,
                TimelineStatus::Failed,
            ]
        );
        assert_eq!(
            timeline.last().and_then(|event| event.message.as_deref()),
            Some("provider unavailable")
        );
    }

    #[tokio::test]
    async fn treats_stream_without_terminal_event_as_failed() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::SessionStarted {
            session_id: "native-3".into(),
            model: None,
            tools: Vec::new(),
        }]));
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "ship it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        assert_eq!(outcome.result, None);
        assert_eq!(
            timeline
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![
                TimelineStatus::UserMessage,
                TimelineStatus::Running,
                TimelineStatus::Failed,
            ]
        );
        assert_eq!(
            timeline.last().and_then(|event| event.message.as_deref()),
            Some("engine stream ended without terminal Completed event")
        );
    }

    #[tokio::test]
    async fn persists_reopens_and_resumes_a_product_session() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        let first_engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-1".into(),
                model: Some("fake-model".into()),
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-1".into(),
                result: Some("first result".into()),
                usage: None,
            },
        ]));

        let first = run_persisted(
            &store,
            single_engine(first_engine),
            healthy,
            RunRequest {
                prompt: "first turn".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_ne!(first.product_session_id, "native-1");
        drop(store);

        // Reopen with retry: under parallel load the previous handle's
        // instance-lock release can lag behind drop (the flake this fixes).
        let reopened = reopen(&database);
        let sessions = reopened.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].product_session_id, first.product_session_id);
        assert_eq!(sessions[0].engine_session_id.as_deref(), Some("native-1"));

        let second_engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-1".into(),
                model: Some("fake-model".into()),
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-1".into(),
                result: Some("second result".into()),
                usage: None,
            },
        ]));
        let second_requests = second_engine.requests.clone();

        let second = run_persisted(
            &reopened,
            single_engine(second_engine),
            healthy,
            RunRequest {
                prompt: "second turn".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some(first.product_session_id.clone()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(second.product_session_id, first.product_session_id);
        assert_eq!(second.result.as_deref(), Some("second result"));
        assert_eq!(
            second_requests.lock().unwrap()[0].resume_session.as_deref(),
            Some("native-1")
        );

        let detail = reopened
            .get_session(&first.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns.len(), 2);
        assert_eq!(detail.turns[0].prompt, "first turn");
        assert_eq!(detail.turns[0].result.as_deref(), Some("first result"));
        assert_eq!(detail.turns[1].prompt, "second turn");
        assert_eq!(detail.turns[1].result.as_deref(), Some("second result"));
    }

    #[tokio::test]
    async fn selected_provider_route_builds_and_pins_the_new_session_target() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "native-provider-route".into(),
            result: Some("provider route completed".into()),
            usage: None,
        }]));
        let observed = Arc::new(Mutex::new(Vec::<EngineTarget>::new()));
        let build_observed = Arc::clone(&observed);
        let build_engine = Arc::clone(&engine);
        let target = EngineTarget {
            engine_id: "alva".into(),
            model_id: "claude-opus-4-7".into(),
            provider_id: "anthropic".into(),
            account_source: "Anthropic API key".into(),
            billing_route: "Anthropic API usage".into(),
            automatic_use_authorized: false,
            required_capabilities: vec!["tools".into()],
            reasoning_effort: Some("high".into()),
            fast_mode: false,
        };

        let outcome = run_persisted_with_target(
            &store,
            move |selected, _, _| {
                build_observed.lock().unwrap().push(selected.clone());
                Ok(Arc::clone(&build_engine) as Arc<dyn EngineRuntime>)
            },
            healthy,
            RunRequest {
                prompt: "use the selected provider".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            Some(target.clone()),
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(observed.lock().unwrap().as_slice(), &[target.clone()]);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        let branch = detail
            .branches
            .iter()
            .find(|branch| branch.branch_id == detail.session.active_branch_id)
            .unwrap();
        assert_eq!(branch.engine_id, target.engine_id);
        assert_eq!(branch.provider_id, target.provider_id);
        assert_eq!(branch.model_id, target.model_id);
    }

    #[tokio::test]
    async fn persists_attempt_usage_and_side_effect_evidence() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-evidence".into(),
                model: Some("fake-model".into()),
                tools: vec!["write_file".into()],
            },
            RuntimeEvent::ToolStart {
                id: "tool-1".into(),
                name: "write_file".into(),
                input: serde_json::json!({"path": "changed.txt"}),
            },
            RuntimeEvent::ToolEnd {
                id: "tool-1".into(),
                name: "write_file".into(),
                result: serde_json::from_value(serde_json::json!({
                    "content": [{"type": "text", "text": "written"}],
                    "is_error": false
                }))
                .unwrap(),
                duration_ms: Some(14),
            },
            RuntimeEvent::Completed {
                session_id: "native-evidence".into(),
                result: Some("done".into()),
                usage: Some(alva_engine_runtime::RuntimeUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                    total_cost_usd: Some(0.03),
                    duration_ms: 28,
                    num_turns: 1,
                }),
            },
        ]));

        let outcome = run_persisted(
            &store,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "change it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.turns[0].attempts.len(), 1);
        let attempt = &detail.turns[0].attempts[0];
        assert_eq!(attempt.status, "completed");
        assert!(attempt.started_at > 0);
        assert!(attempt
            .completed_at
            .is_some_and(|end| end >= attempt.started_at));
        assert_eq!(attempt.failure_reason, None);
        assert_eq!(attempt.usage.as_ref().unwrap().input_tokens, 11);
        assert_eq!(attempt.side_effects.len(), 1);
        assert_eq!(attempt.side_effects[0].tool_name, "write_file");
        assert_eq!(attempt.side_effects[0].succeeded, Some(true));
    }

    #[tokio::test]
    async fn recovered_attempt_without_native_session_continues_fresh() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        store
            .prepare_new_turn(
                "product-before-start",
                workspace.path(),
                "first",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        drop(store);

        let reopened = crate::HubStore::open(&database).unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-fresh".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-fresh".into(),
                result: Some("continued".into()),
                usage: None,
            },
        ]));
        let requests = Arc::clone(&engine.requests);

        let outcome = run_persisted(
            &reopened,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "continue".into(),
                workspace: PathBuf::from("ignored-for-resume"),
                engine_id: "alva".into(),
                product_session_id: Some("product-before-start".into()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(outcome.product_session_id, "product-before-start");
        assert_eq!(requests.lock().unwrap()[0].resume_session, None);
        let detail = reopened
            .get_session("product-before-start")
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].status, "interrupted");
        assert_eq!(detail.turns[1].status, "completed");
        assert_eq!(
            detail.session.engine_session_id.as_deref(),
            Some("native-fresh")
        );
    }

    #[tokio::test]
    async fn first_turn_on_a_new_branch_receives_full_then_native_return_receives_delta() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "alva-native-1".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "alva-native-1".into(),
                result: Some("Alva finished the first task".into()),
                usage: None,
            },
        ]));
        let first = run_persisted(
            &store,
            single_engine(alva),
            healthy,
            RunRequest {
                prompt: "Implement the first task".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        store
            .request_engine_switch(
                &first.product_session_id,
                &EngineTarget::for_engine("claude").unwrap(),
                "User explicitly selected Claude",
            )
            .unwrap();
        let claude = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "claude-native-1".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "claude-native-1".into(),
                result: Some("Claude continued".into()),
                usage: None,
            },
        ]));
        let claude_requests = Arc::clone(&claude.requests);
        run_persisted(
            &store,
            single_engine(claude),
            healthy,
            RunRequest {
                prompt: "Continue with Claude".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "claude".into(),
                product_session_id: Some(first.product_session_id.clone()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();
        let claude_request = claude_requests.lock().unwrap()[0].clone();
        assert!(claude_request
            .prompt
            .contains("<alva-hub-handoff kind=\"full\">"));
        assert!(claude_request
            .prompt
            .contains("User explicitly selected Claude"));
        assert!(claude_request
            .prompt
            .contains("Alva finished the first task"));
        assert!(claude_request.prompt.contains("Continue with Claude"));
        assert_eq!(claude_request.resume_session, None);

        store
            .request_engine_switch(
                &first.product_session_id,
                &EngineTarget::for_engine("alva").unwrap(),
                "User returned to Alva",
            )
            .unwrap();
        let resumed_alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "alva-native-1".into(),
            result: Some("Alva resumed".into()),
            usage: None,
        }]));
        let resumed_requests = Arc::clone(&resumed_alva.requests);
        run_persisted(
            &store,
            single_engine(resumed_alva),
            healthy,
            RunRequest {
                prompt: "Resume the Alva branch".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some(first.product_session_id),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();
        let resumed_request = resumed_requests.lock().unwrap()[0].clone();
        assert_eq!(
            resumed_request.resume_session.as_deref(),
            Some("alva-native-1")
        );
        assert!(resumed_request
            .prompt
            .contains("<alva-hub-handoff kind=\"delta\">"));
        assert!(resumed_request.prompt.contains("User returned to Alva"));
        assert!(resumed_request.prompt.contains("Claude continued"));
    }

    #[tokio::test]
    async fn invalid_native_resume_replaces_delta_with_full_handoff_before_fresh_execution() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();
        let initial = store
            .prepare_new_turn(
                "product-resume-upgrade",
                workspace.path(),
                "initial Alva work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&initial, "expired-native")
            .unwrap();
        store
            .finish_turn(
                &initial,
                Some("expired-native"),
                Some("Alva result"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-resume-upgrade", &claude, "switch to Claude")
            .unwrap();
        let claude_turn = store
            .prepare_resumed_turn("product-resume-upgrade", "Claude follow-up")
            .unwrap();
        store
            .finish_turn(&claude_turn, None, Some("Claude progress"), None, None)
            .unwrap();
        store
            .request_engine_switch("product-resume-upgrade", &alva, "return to Alva")
            .unwrap();

        let requests = Arc::new(Mutex::new(Vec::new()));
        let engine = Arc::new(ResumeFailureThenFreshEngine {
            requests: Arc::clone(&requests),
        });
        let outcome = run_persisted(
            &store,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "finish it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some("product-resume-upgrade".into()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].resume_session.as_deref(),
            Some("expired-native")
        );
        assert!(requests[0].prompt.contains("kind=\"delta\""));
        assert!(requests[1].resume_session.is_none());
        assert!(requests[1].prompt.contains("kind=\"full\""));
        drop(requests);

        let detail = store
            .get_session("product-resume-upgrade")
            .unwrap()
            .unwrap();
        assert_eq!(
            detail.session.engine_session_id.as_deref(),
            Some("replacement-native")
        );
        let latest_turn = detail.turns.last().unwrap();
        assert_eq!(latest_turn.attempts.len(), 2);
        assert_eq!(latest_turn.attempts[0].status, "failed");
        assert_eq!(latest_turn.attempts[1].status, "completed");
    }

    struct StartFailingEngine;

    #[async_trait]
    impl EngineRuntime for StartFailingEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Err(RuntimeError::ProcessError("alva process died".into()))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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

    fn authorized_claude() -> crate::store::EngineTarget {
        let mut claude = crate::store::EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        claude
    }

    #[tokio::test]
    async fn terminal_continuity_failure_falls_back_to_next_authorized_engine_in_same_turn() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "alva-native".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Error {
                message: "engine stream died mid-run".into(),
                recoverable: false,
            },
        ]));
        let claude = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "claude-native".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "claude-native".into(),
                result: Some("claude finished the work".into()),
                usage: None,
            },
        ]));
        let claude_requests = Arc::clone(&claude.requests);

        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                let claude = Arc::clone(&claude);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    "claude" => Ok(Arc::clone(&claude) as Arc<dyn EngineRuntime>),
                    other => Err(format!("unexpected engine '{other}'")),
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("claude finished the work"));

        // The persisted handoff was injected before the target executed.
        let claude_prompt = claude_requests.lock().unwrap()[0].prompt.clone();
        assert!(claude_prompt.contains("<alva-hub-handoff kind=\"full\""));
        assert!(claude_prompt.contains("do the work"));

        let fallback = timeline
            .iter()
            .find(|event| event.status == TimelineStatus::Fallback)
            .expect("a fallback timeline card was emitted");
        assert_eq!(fallback.engine_id, "claude");
        let message = fallback.message.as_deref().unwrap();
        assert!(message.contains("alva → claude"));
        assert!(message.contains("engine stream died mid-run"));
        assert!(message.contains("Handoff: full context"));

        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.engine_id, "claude");
        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.turns[0].status, "completed");
        let attempts = &detail.turns[0].attempts;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].engine_id, "alva");
        assert_eq!(attempts[0].status, "failed");
        assert_eq!(
            attempts[0].failure_reason.as_deref(),
            Some("engine stream died mid-run")
        );
        assert_eq!(attempts[1].engine_id, "claude");
        assert_eq!(attempts[1].status, "completed");
        assert!(detail
            .branches
            .iter()
            .any(|branch| branch.engine_id == "alva" && branch.status == "parked"));
        assert!(detail
            .branches
            .iter()
            .any(|branch| branch.engine_id == "claude" && branch.status == "active"));
        assert!(detail
            .timeline_events
            .iter()
            .any(|event| event.kind == "engine_fallback"));
        // The delivered handoff is marked used.
        assert!(detail
            .handoffs
            .iter()
            .find(|handoff| handoff.target.engine_id == "claude")
            .unwrap()
            .used_at
            .is_some());
    }

    #[tokio::test]
    async fn fallback_chain_tries_each_authorized_engine_once_in_order() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![authorized_claude(), codex])
            .unwrap();

        let dying_events = vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }];
        let alva = Arc::new(FakeEngine::new(dying_events.clone()));
        let claude = Arc::new(FakeEngine::new(dying_events));
        let codex_engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "codex-native".into(),
            result: Some("codex finished it".into()),
            usage: None,
        }]));

        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                let claude = Arc::clone(&claude);
                let codex_engine = Arc::clone(&codex_engine);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    "claude" => Ok(Arc::clone(&claude) as Arc<dyn EngineRuntime>),
                    "codex" => Ok(Arc::clone(&codex_engine) as Arc<dyn EngineRuntime>),
                    other => Err(format!("unexpected engine '{other}'")),
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("codex finished it"));
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.engine_id, "codex");
        assert_eq!(detail.turns[0].status, "completed");
        let engines: Vec<_> = detail.turns[0]
            .attempts
            .iter()
            .map(|attempt| attempt.engine_id.as_str())
            .collect();
        assert_eq!(engines, vec!["alva", "claude", "codex"]);
        // Each engine executed exactly once.
        assert_eq!(alva.requests.lock().unwrap().len(), 1);
        assert_eq!(claude.requests.lock().unwrap().len(), 1);
        assert_eq!(codex_engine.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn failed_engine_build_after_native_resume_upgrade_finishes_the_turn() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = crate::store::EngineTarget::for_engine("alva").unwrap();
        let first = store
            .prepare_new_turn(
                "product-upgrade-build",
                workspace.path(),
                "start",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&first, "expired-native")
            .unwrap();
        store
            .finish_turn(&first, Some("expired-native"), Some("done"), None, None)
            .unwrap();

        let builds = Arc::new(Mutex::new(0usize));
        let outcome = run_persisted(
            &store,
            {
                let builds = Arc::clone(&builds);
                move |_target: &crate::store::EngineTarget, _: &str, _| {
                    let mut builds = builds.lock().unwrap();
                    *builds += 1;
                    if *builds == 1 {
                        Ok(Arc::new(ResumeFailureThenFreshEngine {
                            requests: Arc::new(Mutex::new(Vec::new())),
                        }) as Arc<dyn EngineRuntime>)
                    } else {
                        Err("provider configuration vanished".to_string())
                    }
                }
            },
            healthy,
            RunRequest {
                prompt: "continue".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some("product-upgrade-build".into()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        // The turn is finished — the session must not be wedged in running.
        let detail = store.get_session("product-upgrade-build").unwrap().unwrap();
        assert_ne!(detail.session.status, "running");
        let latest_turn = detail.turns.last().unwrap();
        assert_eq!(latest_turn.status, "failed");
        assert!(latest_turn
            .attempts
            .iter()
            .all(|attempt| attempt.status != "running"));
    }

    #[tokio::test]
    async fn fallback_target_start_failure_keeps_source_and_handoff_recoverable() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        let outcome = run_persisted(
            &store,
            |target: &crate::store::EngineTarget, _: &str, _| match target.engine_id.as_str() {
                "alva" => Ok(Arc::new(StartFailingEngine) as Arc<dyn EngineRuntime>),
                _ => Err("claude binary is missing".to_string()),
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(outcome.failed);

        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].status, "failed");
        let attempts = &detail.turns[0].attempts;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].engine_id, "alva");
        assert_eq!(attempts[0].status, "failed");
        assert_eq!(attempts[1].engine_id, "claude");
        assert_eq!(attempts[1].status, "failed");
        assert!(attempts[1]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("claude binary is missing")));

        // Source branch and the pending handoff survive for recovery — the
        // handoff was never delivered, so it must still be unused.
        assert!(detail
            .branches
            .iter()
            .any(|branch| branch.engine_id == "alva" && branch.status == "parked"));
        let handoff = detail
            .handoffs
            .iter()
            .find(|handoff| handoff.target.engine_id == "claude")
            .unwrap();
        assert!(handoff.used_at.is_none());
        assert_ne!(detail.session.status, "running");
    }

    #[tokio::test]
    async fn continuity_failure_without_authorization_fails_without_fallback() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut unauthorized = crate::store::EngineTarget::for_engine("claude").unwrap();
        unauthorized.automatic_use_authorized = false;
        store
            .set_global_fallback_policy(vec![unauthorized])
            .unwrap();

        let outcome = run_persisted(
            &store,
            |target: &crate::store::EngineTarget, _: &str, _| match target.engine_id.as_str() {
                "alva" => Ok(Arc::new(StartFailingEngine) as Arc<dyn EngineRuntime>),
                other => Err(format!(
                    "engine '{other}' must not be built without authorization"
                )),
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.session.engine_id, "alva");
        // No fallback happened and no exhaustion card is recorded — nothing
        // was ever authorized, so the plain failure card tells the story.
        assert!(detail.timeline_events.iter().all(|event| {
            event.kind != "engine_fallback" && event.kind != "fallback_exhausted"
        }));
    }

    /// Reports every capability except permission_callback; used to trigger
    /// the capability-compatibility skip.
    struct NoPermissionEngine;

    #[async_trait]
    impl EngineRuntime for NoPermissionEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(tokio_stream::iter(Vec::new())))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn capabilities(&self) -> RuntimeCapabilities {
            RuntimeCapabilities {
                streaming: true,
                tool_control: true,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
    }

    #[tokio::test]
    async fn unhealthy_target_is_skipped_with_reason_and_the_chain_continues() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![authorized_claude(), codex])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }]));
        let codex_engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "codex-native".into(),
            result: Some("codex finished it".into()),
            usage: None,
        }]));

        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                let codex_engine = Arc::clone(&codex_engine);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    "codex" => Ok(Arc::clone(&codex_engine) as Arc<dyn EngineRuntime>),
                    other => Err(format!("engine '{other}' must not be built when unhealthy")),
                }
            },
            |target: &crate::store::EngineTarget| {
                (target.engine_id == "claude").then(|| "engine binary not found".to_string())
            },
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("codex finished it"));
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        // Claude was never attempted — skipped with the recorded reason.
        let engines: Vec<_> = detail.turns[0]
            .attempts
            .iter()
            .map(|attempt| attempt.engine_id.as_str())
            .collect();
        assert_eq!(engines, vec!["alva", "codex"]);
        let skip = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "engine_fallback_skipped")
            .expect("skip fact recorded");
        assert_eq!(skip.target_engine_id, "claude");
        assert!(skip.reason.contains("engine binary not found"));
    }

    #[tokio::test]
    async fn capability_incompatible_target_is_skipped_with_reason() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut claude = authorized_claude();
        claude.required_capabilities = vec!["permission_callback".to_string()];
        let mut codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![claude, codex])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }]));
        let codex_engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "codex-native".into(),
            result: Some("codex finished it".into()),
            usage: None,
        }]));

        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                let codex_engine = Arc::clone(&codex_engine);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    "claude" => Ok(Arc::new(NoPermissionEngine) as Arc<dyn EngineRuntime>),
                    "codex" => Ok(Arc::clone(&codex_engine) as Arc<dyn EngineRuntime>),
                    other => Err(format!("unexpected engine '{other}'")),
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        let engines: Vec<_> = detail.turns[0]
            .attempts
            .iter()
            .map(|attempt| attempt.engine_id.as_str())
            .collect();
        assert_eq!(engines, vec!["alva", "codex"]);
        let skip = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "engine_fallback_skipped")
            .expect("skip fact recorded");
        assert_eq!(skip.target_engine_id, "claude");
        assert!(skip
            .reason
            .contains("lacks required capabilities: permission_callback"));
    }

    #[tokio::test]
    async fn exhausted_chain_records_a_card_with_every_terminal_state() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            |target: &crate::store::EngineTarget, _: &str, _| match target.engine_id.as_str() {
                "alva" => Ok(Arc::new(StartFailingEngine) as Arc<dyn EngineRuntime>),
                other => Err(format!("engine '{other}' must not be built when unhealthy")),
            },
            |target: &crate::store::EngineTarget| {
                (target.engine_id == "claude")
                    .then(|| "not signed in — run `claude` to sign in".to_string())
            },
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        let exhausted = timeline
            .iter()
            .find(|event| event.status == TimelineStatus::Exhausted)
            .expect("live exhaustion card emitted");
        let message = exhausted.message.as_deref().unwrap();
        assert!(message.contains("Fallback chain exhausted"));
        assert!(message.contains("alva: engine crash"));
        assert!(message.contains("claude: skipped — not signed in"));

        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        let fact = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "fallback_exhausted")
            .expect("exhaustion fact recorded");
        assert!(fact.reason.contains("alva: engine crash"));
        assert!(fact.reason.contains("claude: skipped"));
        // Only the source engine ever ran an attempt.
        assert_eq!(detail.turns[0].attempts.len(), 1);
    }

    #[tokio::test]
    async fn same_engine_alternate_provider_target_is_a_valid_fallback() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut claude_alt = authorized_claude();
        claude_alt.provider_id = "claude-alt-provider".into();
        store.set_global_fallback_policy(vec![claude_alt]).unwrap();

        // The session starts on the regular claude target and dies mid-run.
        let claude = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }]));
        let claude_alt_engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "claude-alt-native".into(),
            result: Some("alternate provider finished it".into()),
            usage: None,
        }]));

        let calls = Arc::new(Mutex::new(0usize));
        let outcome = run_persisted(
            &store,
            {
                let claude = Arc::clone(&claude);
                let claude_alt_engine = Arc::clone(&claude_alt_engine);
                let calls = Arc::clone(&calls);
                move |target: &crate::store::EngineTarget, _: &str, _| {
                    let mut calls = calls.lock().unwrap();
                    *calls += 1;
                    if target.provider_id == "claude-alt-provider" {
                        Ok(Arc::clone(&claude_alt_engine) as Arc<dyn EngineRuntime>)
                    } else {
                        Ok(Arc::clone(&claude) as Arc<dyn EngineRuntime>)
                    }
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "claude".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(
            outcome.result.as_deref(),
            Some("alternate provider finished it")
        );
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.provider_id, "claude-alt-provider");
        assert_eq!(detail.turns[0].attempts.len(), 2);
        assert_eq!(detail.turns[0].attempts[1].status, "completed");
    }

    #[tokio::test]
    async fn unclassified_mid_chain_failure_stops_and_still_summarizes_the_cycle() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![authorized_claude(), codex])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }]));
        // Claude fails with something the matrix cannot classify — the chain
        // must stop without spending codex.
        let claude = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "the answer refused to rhyme".into(),
            recoverable: false,
        }]));

        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                let claude = Arc::clone(&claude);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    "claude" => Ok(Arc::clone(&claude) as Arc<dyn EngineRuntime>),
                    other => Err(format!(
                        "engine '{other}' must not run after an Other failure"
                    )),
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        // codex was never attempted.
        let engines: Vec<_> = detail.turns[0]
            .attempts
            .iter()
            .map(|attempt| attempt.engine_id.as_str())
            .collect();
        assert_eq!(engines, vec!["alva", "claude"]);

        let card = timeline
            .iter()
            .find(|event| event.status == TimelineStatus::Exhausted)
            .expect("cycle summary card emitted");
        let message = card.message.as_deref().unwrap();
        assert!(message.contains("Fallback stopped"));
        assert!(message.contains("alva: transport failure"));
        assert!(message.contains("claude: unclassified failure — the answer refused to rhyme"));
    }

    /// Emits scripted events with paused-clock delays; optionally keeps the
    /// stream open (never terminating) to simulate a stalled engine.
    struct ScriptedEngine {
        script: Vec<(u64, RuntimeEvent)>,
        hold_open_at_end: bool,
    }

    #[async_trait]
    impl EngineRuntime for ScriptedEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            let script = self.script.clone();
            let hold_open = self.hold_open_at_end;
            let (sender, receiver) = tokio::sync::mpsc::channel(16);
            tokio::spawn(async move {
                for (delay_seconds, event) in script {
                    tokio::time::sleep(std::time::Duration::from_secs(delay_seconds)).await;
                    if sender.send(event).await.is_err() {
                        return;
                    }
                }
                if hold_open {
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
                }
            });
            Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(
                receiver,
            )))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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

    fn session_started(session_id: &str) -> RuntimeEvent {
        RuntimeEvent::SessionStarted {
            session_id: session_id.into(),
            model: None,
            tools: Vec::new(),
        }
    }

    fn completed(session_id: &str, result: &str) -> RuntimeEvent {
        RuntimeEvent::Completed {
            session_id: session_id.into(),
            result: Some(result.into()),
            usage: None,
        }
    }

    fn text_delta(id: &str) -> RuntimeEvent {
        serde_json::from_value(serde_json::json!({
            "event_type": "MessageDelta",
            "id": id,
            "delta": {"TextDelta": {"text": "still thinking…"}}
        }))
        .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn confirmed_stall_classifies_stably_and_falls_back() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        let claude = Arc::new(FakeEngine::new(vec![completed(
            "claude-native",
            "claude finished it",
        )]));
        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            {
                let claude = Arc::clone(&claude);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::new(ScriptedEngine {
                        script: vec![(0, session_started("alva-native"))],
                        hold_open_at_end: true,
                    }) as Arc<dyn EngineRuntime>),
                    "claude" => Ok(Arc::clone(&claude) as Arc<dyn EngineRuntime>),
                    other => Err(format!("unexpected engine '{other}'")),
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("claude finished it"));
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        let attempts = &detail.turns[0].attempts;
        assert_eq!(attempts.len(), 2);
        assert!(attempts[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("stalled")));
        assert!(timeline.iter().any(|event| {
            event.status == TimelineStatus::Failed
                && event
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("stalled"))
        }));
        assert!(timeline
            .iter()
            .any(|event| event.status == TimelineStatus::Fallback));
        let fallback_fact = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "engine_fallback")
            .unwrap();
        assert!(fallback_fact.reason.contains("stalled"));
    }

    #[tokio::test(start_paused = true)]
    async fn long_running_tool_does_not_trip_the_watchdog() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();

        let tool_end: RuntimeEvent = serde_json::from_value(serde_json::json!({
            "event_type": "ToolEnd",
            "id": "tool-1",
            "name": "run_tests",
            "result": {"content": [{"type": "text", "text": "ok"}], "is_error": false},
            "duration_ms": 600_000
        }))
        .unwrap();
        let engine = Arc::new(ScriptedEngine {
            script: vec![
                (0, session_started("alva-native")),
                (
                    0,
                    RuntimeEvent::ToolStart {
                        id: "tool-1".into(),
                        name: "run_tests".into(),
                        input: serde_json::json!({}),
                    },
                ),
                // Far beyond the stall window, but the tool is open.
                (600, tool_end),
                (0, completed("alva-native", "took a long tool")),
            ],
            hold_open_at_end: false,
        });

        let outcome = run_persisted(
            &store,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("took a long tool"));
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.turns[0].attempts[0].status, "completed");
    }

    #[tokio::test(start_paused = true)]
    async fn pending_user_approval_does_not_trip_the_watchdog() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();

        let engine = Arc::new(ScriptedEngine {
            script: vec![
                (0, session_started("alva-native")),
                (
                    0,
                    RuntimeEvent::PermissionRequest {
                        request_id: "perm-1".into(),
                        tool_name: "write_file".into(),
                        tool_input: serde_json::json!({}),
                        description: None,
                    },
                ),
                // The user takes far longer than the stall window to approve.
                (600, completed("alva-native", "approved eventually")),
            ],
            hold_open_at_end: false,
        });

        let outcome = run_persisted(
            &store,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("approved eventually"));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_tool_finishing_does_not_rearm_watchdog_during_pending_approval() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();

        let tool_end: RuntimeEvent = serde_json::from_value(serde_json::json!({
            "event_type": "ToolEnd",
            "id": "tool-1",
            "name": "run_tests",
            "result": {"content": [{"type": "text", "text": "ok"}], "is_error": false},
            "duration_ms": 5
        }))
        .unwrap();
        // A concurrent tool finishes AFTER the approval request; the user
        // still takes far longer than the stall window to decide.
        let engine = Arc::new(ScriptedEngine {
            script: vec![
                (0, session_started("alva-native")),
                (
                    0,
                    RuntimeEvent::ToolStart {
                        id: "tool-1".into(),
                        name: "run_tests".into(),
                        input: serde_json::json!({}),
                    },
                ),
                (
                    0,
                    RuntimeEvent::PermissionRequest {
                        request_id: "perm-1".into(),
                        tool_name: "write_file".into(),
                        tool_input: serde_json::json!({}),
                        description: None,
                    },
                ),
                (0, tool_end),
                (600, completed("alva-native", "approved eventually")),
            ],
            hold_open_at_end: false,
        });

        let outcome = run_persisted(
            &store,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("approved eventually"));
    }

    #[tokio::test(start_paused = true)]
    async fn slow_but_recovering_output_resets_the_watchdog() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();

        // Deltas arrive every 100s: each gap is inside the 180s window, but
        // the total run is far beyond it.
        let engine = Arc::new(ScriptedEngine {
            script: vec![
                (0, session_started("alva-native")),
                (100, text_delta("m1")),
                (100, text_delta("m1")),
                (100, text_delta("m1")),
                (100, completed("alva-native", "streamed slowly")),
            ],
            hold_open_at_end: false,
        });

        let outcome = run_persisted(
            &store,
            single_engine(engine),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("streamed slowly"));
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].attempts.len(), 1);
    }

    /// Streams SessionStarted, then waits until `cancel` is called before
    /// emitting its terminal events — the live-Stop shape of a real adapter.
    struct CancellableEngine {
        cancelled: Arc<tokio::sync::Notify>,
        cancel_calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl EngineRuntime for CancellableEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            let cancelled = Arc::clone(&self.cancelled);
            let (sender, receiver) = tokio::sync::mpsc::channel(4);
            tokio::spawn(async move {
                let _ = sender.send(session_started("cancel-native")).await;
                cancelled.notified().await;
                let _ = sender
                    .send(RuntimeEvent::Error {
                        message: "Cancelled by user".into(),
                        recoverable: false,
                    })
                    .await;
                let _ = sender
                    .send(RuntimeEvent::Completed {
                        session_id: "cancel-native".into(),
                        result: None,
                        usage: None,
                    })
                    .await;
            });
            Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(
                receiver,
            )))
        }

        async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
            self.cancel_calls
                .lock()
                .unwrap()
                .push(session_id.to_string());
            self.cancelled.notify_one();
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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
    async fn user_stop_cancels_the_attempt_without_fallback_and_survives_restart() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        // Claude is authorized — a user Stop must still never fall back.
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        let engine = Arc::new(CancellableEngine {
            cancelled: Arc::new(tokio::sync::Notify::new()),
            cancel_calls: Arc::new(Mutex::new(Vec::new())),
        });
        let run_store = store.clone();
        let run_engine = Arc::clone(&engine);
        let run = tokio::spawn(async move {
            run_persisted(
                &run_store,
                move |_: &crate::store::EngineTarget, _: &str, _| {
                    Ok(Arc::clone(&run_engine) as Arc<dyn EngineRuntime>)
                },
                healthy,
                RunRequest {
                    prompt: "long work".into(),
                    workspace: workspace.path().to_path_buf(),
                    engine_id: "alva".into(),
                    product_session_id: None,
                    permission_profile: None,
                },
                |_| {},
            )
            .await
        });

        // Wait for the run to persist its session and native session id —
        // the same facts the Stop IPC reads before cancelling.
        let product_session_id = loop {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let sessions = store.list_sessions().unwrap();
            if let Some(session) = sessions.first() {
                if session.engine_session_id.is_some() && session.status == "running" {
                    break session.product_session_id.clone();
                }
            }
        };

        // The Stop control: record the fact, then cancel the live engine.
        store
            .record_attempt_cancelled(&product_session_id, "User stopped the attempt")
            .unwrap();
        engine.cancel("cancel-native").await.unwrap();

        let outcome = run.await.unwrap().unwrap();
        assert!(outcome.failed);
        assert_eq!(
            engine.cancel_calls.lock().unwrap().as_slice(),
            ["cancel-native"]
        );

        let detail = store.get_session(&product_session_id).unwrap().unwrap();
        // Cancellation is not a continuity failure: exactly one attempt.
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert!(detail.turns[0].attempts[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("Cancelled")));
        let cancelled_facts = detail
            .timeline_events
            .iter()
            .filter(|event| event.kind == "attempt_cancelled")
            .count();
        assert_eq!(cancelled_facts, 1);

        // Restart: the recorded control action must not replay.
        drop(store);
        let reopened = crate::HubStore::open(&database).unwrap();
        let detail = reopened.get_session(&product_session_id).unwrap().unwrap();
        assert_eq!(
            detail
                .timeline_events
                .iter()
                .filter(|event| event.kind == "attempt_cancelled")
                .count(),
            1
        );
        assert_ne!(detail.session.status, "running");
        // Stop against a terminal session is refused (no repeat execution).
        assert!(reopened
            .record_attempt_cancelled(&product_session_id, "again")
            .is_err());
    }

    #[tokio::test]
    async fn canary_secrets_never_reach_the_fallback_engine() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        // Seed a completed turn whose prompt and result both carry canaries.
        let alva_target = crate::store::EngineTarget::for_engine("alva").unwrap();
        let seeded = store
            .prepare_new_turn(
                "product-canary",
                workspace.path(),
                "deploy with API_KEY=sk-live-canary9999",
                &alva_target,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(
                &seeded,
                Some("alva-native"),
                Some("done; GITHUB_TOKEN=ghp_canarytoken99x worked"),
                None,
                None,
            )
            .unwrap();

        // The next turn dies on alva and falls back to claude.
        let alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }]));
        let claude = Arc::new(FakeEngine::new(vec![completed(
            "claude-native",
            "claude finished it",
        )]));
        let claude_requests = Arc::clone(&claude.requests);
        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                let claude = Arc::clone(&claude);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    "claude" => Ok(Arc::clone(&claude) as Arc<dyn EngineRuntime>),
                    other => Err(format!("unexpected engine '{other}'")),
                }
            },
            healthy,
            RunRequest {
                prompt: "continue the deployment".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some("product-canary".into()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        let claude_prompt = claude_requests.lock().unwrap()[0].prompt.clone();
        // The handoff crossed engines — filtered.
        assert!(claude_prompt.contains("<alva-hub-handoff"));
        assert!(claude_prompt.contains("[redacted]"));
        assert!(!claude_prompt.contains("sk-live-canary9999"));
        assert!(!claude_prompt.contains("ghp_canarytoken99x"));
        // Adapter-private billing metadata never crosses either.
        assert!(!claude_prompt.contains("Workspace provider account"));
        assert!(!claude_prompt.contains("Anthropic account via Claude CLI"));
        // Transcript stays off by default — parse the actual package the
        // adapter received rather than matching serde formatting.
        let package_json = claude_prompt
            .split_once(">\n")
            .and_then(|(_, rest)| rest.rsplit_once("\n</alva-hub-handoff>"))
            .map(|(json, _)| json)
            .expect("handoff JSON embedded in the prompt");
        let package: crate::store::HandoffPackage = serde_json::from_str(package_json).unwrap();
        assert!(package.transcript.is_empty());
    }

    #[tokio::test]
    async fn team_worker_executes_on_its_own_engine_and_reports_to_the_lead() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = crate::store::EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "team-lead",
                workspace.path(),
                "plan the sprint",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();

        // Lead runs on alva; the worker gets its own engine (codex).
        let codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        let run = store.create_team_run("team-lead", &codex).unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "add integration tests",
                &[],
            )
            .unwrap();
        let task = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();

        let worker_engine = Arc::new(FakeEngine::new(vec![
            session_started("codex-native"),
            RuntimeEvent::ToolStart {
                id: "t1".into(),
                name: "write_file".into(),
                input: serde_json::json!({"path": "tests/integration.rs"}),
            },
            serde_json::from_value(serde_json::json!({
                "event_type": "ToolEnd",
                "id": "t1",
                "name": "write_file",
                "result": {"content": [{"type": "text", "text": "ok"}], "is_error": false},
                "duration_ms": 4
            }))
            .unwrap(),
            completed("codex-native", "integration tests added"),
        ]));
        let worker_session_id = run.workers[0].worker_session_id.clone();
        let outcome = run_persisted(
            &store,
            single_engine(Arc::clone(&worker_engine)),
            healthy,
            RunRequest {
                prompt: task.scope.clone(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "codex".into(),
                product_session_id: Some(worker_session_id.clone()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();
        assert!(!outcome.failed);
        let finished = store
            .finish_worker_task(&task.task_id, !outcome.failed, outcome.result.as_deref())
            .unwrap();

        assert_eq!(finished.status, "completed");
        assert_eq!(finished.result.as_deref(), Some("integration tests added"));
        assert_eq!(
            finished.changed_files,
            vec!["tests/integration.rs".to_string()]
        );
        assert!(!finished.verification.is_empty());

        // The worker is a durable Hub session with its own recorded turn.
        let worker_detail = store.get_session(&worker_session_id).unwrap().unwrap();
        assert_eq!(worker_detail.turns.len(), 1);
        assert_eq!(worker_detail.session.engine_id, "codex");

        // The outcome crossed the execution channel back to the lead.
        let lead_detail = store.get_session("team-lead").unwrap().unwrap();
        let outcome_fact = lead_detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "worker_outcome")
            .expect("worker outcome visible on the lead timeline");
        assert!(outcome_fact.reason.contains("integration tests added"));
        assert_eq!(outcome_fact.target_engine_id, "codex");
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_worker_tasks_genuinely_overlap_and_keep_attribution() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = crate::store::EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "parallel-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run(
                "parallel-lead",
                &crate::store::EngineTarget::for_engine("claude").unwrap(),
            )
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &crate::store::EngineTarget::for_engine("codex").unwrap(),
                None,
            )
            .unwrap();
        let claude_worker = run.workers[0].clone();
        let codex_worker = run.workers[1].clone();

        store
            .enqueue_worker_task(
                &run.team_run_id,
                &claude_worker.worker_id,
                "claude job",
                &[],
            )
            .unwrap();
        store
            .enqueue_worker_task(&run.team_run_id, &codex_worker.worker_id, "codex job", &[])
            .unwrap();

        // Each engine takes simulated wall time; the total-time bound proves
        // real overlap under the paused clock.
        let make_engine = |name: &'static str, seconds: u64, result: &'static str| {
            Arc::new(ScriptedEngine {
                script: vec![
                    (0, session_started(&format!("{name}-native"))),
                    (seconds, completed(&format!("{name}-native"), result)),
                ],
                hold_open_at_end: false,
            }) as Arc<dyn EngineRuntime>
        };
        let claude_engine = make_engine("claude", 100, "claude finished");
        let codex_engine = make_engine("codex", 60, "codex finished");

        let claude_run = {
            let store = store.clone();
            let worker = claude_worker.clone();
            let engine = Arc::clone(&claude_engine);
            async move {
                let task = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
                let _started = tokio::time::Instant::now();
                let outcome = run_persisted(
                    &store,
                    move |_: &crate::store::EngineTarget, _: &str, _| {
                        Ok(Arc::clone(&engine) as Arc<dyn EngineRuntime>)
                    },
                    healthy,
                    RunRequest {
                        prompt: task.scope.clone(),
                        workspace: PathBuf::from(""),
                        engine_id: "claude".into(),
                        product_session_id: Some(worker.worker_session_id.clone()),
                        permission_profile: None,
                    },
                    |_| {},
                )
                .await
                .unwrap();
                store
                    .finish_worker_task(&task.task_id, !outcome.failed, outcome.result.as_deref())
                    .unwrap()
            }
        };
        let codex_run = {
            let store = store.clone();
            let worker = codex_worker.clone();
            let engine = Arc::clone(&codex_engine);
            async move {
                let task = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
                let _started = tokio::time::Instant::now();
                let outcome = run_persisted(
                    &store,
                    move |_: &crate::store::EngineTarget, _: &str, _| {
                        Ok(Arc::clone(&engine) as Arc<dyn EngineRuntime>)
                    },
                    healthy,
                    RunRequest {
                        prompt: task.scope.clone(),
                        workspace: PathBuf::from(""),
                        engine_id: "codex".into(),
                        product_session_id: Some(worker.worker_session_id.clone()),
                        permission_profile: None,
                    },
                    |_| {},
                )
                .await
                .unwrap();
                store
                    .finish_worker_task(&task.task_id, !outcome.failed, outcome.result.as_deref())
                    .unwrap()
            }
        };

        let overall = tokio::time::Instant::now();
        let (claude_task, codex_task) = tokio::join!(claude_run, codex_run);
        let total = overall.elapsed().as_secs();

        // Genuine overlap: serial execution would take ~160 simulated
        // seconds; concurrent execution is bounded by the slower engine.
        assert!(total < 160, "workers ran serially: {total}s");
        assert!(total >= 100, "claude engine takes 100s: {total}s");

        // Attribution: each outcome keeps its task, engine, and validation.
        assert_eq!(claude_task.status, "completed");
        assert_eq!(claude_task.result.as_deref(), Some("claude finished"));
        assert_eq!(claude_task.worker_id, claude_worker.worker_id);
        assert_eq!(codex_task.status, "completed");
        assert_eq!(codex_task.result.as_deref(), Some("codex finished"));
        assert_eq!(codex_task.worker_id, codex_worker.worker_id);
        let lead_detail = store.get_session("parallel-lead").unwrap().unwrap();
        assert_eq!(
            lead_detail
                .timeline_events
                .iter()
                .filter(|event| event.kind == "worker_outcome")
                .count(),
            2
        );
    }

    #[tokio::test(start_paused = true)]
    async fn one_worker_falls_back_while_a_sibling_keeps_progressing() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = crate::store::EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "conc-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        // Both workers may fall back to claude.
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();
        let run = store
            .create_team_run(
                "conc-lead",
                &crate::store::EngineTarget::for_engine("alva").unwrap(),
            )
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &crate::store::EngineTarget::for_engine("codex").unwrap(),
                None,
            )
            .unwrap();
        let (failing, healthy_worker) = (run.workers[0].clone(), run.workers[1].clone());
        let workspace_path = workspace.path().to_path_buf();
        store
            .enqueue_worker_task(&run.team_run_id, &failing.worker_id, "doomed work", &[])
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &healthy_worker.worker_id,
                "steady work",
                &[],
            )
            .unwrap();

        // Worker A's engine dies mid-run and falls back to claude.
        let dying = Arc::new(ScriptedEngine {
            script: vec![
                (0, session_started("alva-native")),
                (
                    20,
                    RuntimeEvent::Error {
                        message: "engine stream died mid-run".into(),
                        recoverable: false,
                    },
                ),
            ],
            hold_open_at_end: false,
        });
        let rescuer = Arc::new(FakeEngine::new(vec![completed(
            "claude-native",
            "claude rescued it",
        )]));
        // Worker B outlasts A's whole failure+fallback cycle (A fails at
        // 20s) so it is provably still in flight while A falls back — but
        // stays inside the 180s stall window so the watchdog stays out of it.
        let sibling_engine = Arc::new(ScriptedEngine {
            script: vec![
                (0, session_started("codex-native")),
                (120, completed("codex-native", "sibling finished")),
            ],
            hold_open_at_end: false,
        });

        // Observed AT the moment the fallback card is emitted, so the proof
        // cannot be satisfied by serialized execution.
        let sibling_state_at_fallback: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let fallback_session: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let sibling_task_id = store
            .claim_for_test(&healthy_worker.worker_id)
            .unwrap()
            .unwrap()
            .task_id;

        let failing_run = {
            let store = store.clone();
            let worker = failing.clone();
            let dying = Arc::clone(&dying);
            let rescuer = Arc::clone(&rescuer);
            let workspace_path = workspace_path.clone();
            let observed = Arc::clone(&sibling_state_at_fallback);
            let observed_session = Arc::clone(&fallback_session);
            let observed_store = store.clone();
            let sibling_session_id = healthy_worker.worker_session_id.clone();
            async move {
                let task = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
                let outcome = run_persisted(
                    &store,
                    move |target: &crate::store::EngineTarget, _: &str, _| {
                        Ok(match target.engine_id.as_str() {
                            "claude" => Arc::clone(&rescuer) as Arc<dyn EngineRuntime>,
                            _ => Arc::clone(&dying) as Arc<dyn EngineRuntime>,
                        })
                    },
                    healthy,
                    RunRequest {
                        prompt: task.scope.clone(),
                        workspace: workspace_path.clone(),
                        engine_id: "alva".into(),
                        product_session_id: Some(worker.worker_session_id.clone()),
                        permission_profile: None,
                    },
                    |event| {
                        if event.status == TimelineStatus::Fallback {
                            // The sibling SESSION being "running" means its
                            // engine turn is actually in flight — a claimed
                            // task row alone would prove nothing.
                            *observed.lock().unwrap() = observed_store
                                .get_session(&sibling_session_id)
                                .unwrap()
                                .map(|detail| detail.session.status);
                            *observed_session.lock().unwrap() =
                                Some(event.product_session_id.clone());
                        }
                    },
                )
                .await
                .unwrap();
                store
                    .finish_worker_task(&task.task_id, !outcome.failed, outcome.result.as_deref())
                    .unwrap()
            }
        };
        let sibling_run = {
            let store = store.clone();
            let worker = healthy_worker.clone();
            let engine = Arc::clone(&sibling_engine);
            let workspace_path = workspace_path.clone();
            let task_id = sibling_task_id.clone();
            async move {
                let task = store.worker_task_by_id(&task_id).unwrap().unwrap();
                let outcome = run_persisted(
                    &store,
                    move |_: &crate::store::EngineTarget, _: &str, _| {
                        Ok(Arc::clone(&engine) as Arc<dyn EngineRuntime>)
                    },
                    healthy,
                    RunRequest {
                        prompt: task.scope.clone(),
                        workspace: workspace_path.clone(),
                        engine_id: "codex".into(),
                        product_session_id: Some(worker.worker_session_id.clone()),
                        permission_profile: None,
                    },
                    |_| {},
                )
                .await
                .unwrap();
                store
                    .finish_worker_task(&task.task_id, !outcome.failed, outcome.result.as_deref())
                    .unwrap()
            }
        };

        let (failed_over, sibling) = tokio::join!(failing_run, sibling_run);

        // THE proof of overlap: when A's fallback landed, B's task was
        // observed still running. Serialized execution could not produce
        // this — B would have been "completed" (or not yet claimed).
        assert_eq!(
            sibling_state_at_fallback.lock().unwrap().as_deref(),
            Some("running"),
            "the sibling's engine turn must still be in flight when the fallback lands"
        );
        // And the fallback event was attributed to the failing worker.
        assert_eq!(
            fallback_session.lock().unwrap().as_deref(),
            Some(failing.worker_session_id.as_str())
        );

        // A fell back and still completed its task on the rescue engine.
        assert_eq!(failed_over.status, "completed");
        assert_eq!(failed_over.result.as_deref(), Some("claude rescued it"));
        let failing_detail = store
            .get_session(&failing.worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(failing_detail.session.engine_id, "claude");
        assert_eq!(failing_detail.turns[0].attempts.len(), 2);

        // B was never cancelled: it finished its own work on its own engine.
        assert_eq!(sibling.status, "completed");
        assert_eq!(sibling.result.as_deref(), Some("sibling finished"));
        let sibling_detail = store
            .get_session(&healthy_worker.worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(sibling_detail.session.engine_id, "codex");
        assert_eq!(sibling_detail.turns[0].attempts.len(), 1);
        assert!(sibling_detail
            .timeline_events
            .iter()
            .all(|event| event.kind != "engine_fallback"));

        // The lead never fell back and still owns the run.
        let lead_detail = store.get_session("conc-lead").unwrap().unwrap();
        assert_eq!(lead_detail.session.engine_id, "alva");
        let run = store.team_run_for_lead("conc-lead").unwrap().unwrap();
        assert_eq!(run.workers.len(), 2);
        assert!(run.tasks.iter().all(|task| task.status == "completed"));
    }

    #[tokio::test]
    async fn poor_quality_success_never_triggers_fallback() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_claude()])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "alva-native".into(),
            result: Some("an unhelpful but successfully delivered answer".into()),
            usage: None,
        }]));

        let outcome = run_persisted(
            &store,
            single_engine(alva),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.session.engine_id, "alva");
        assert!(detail.timeline_events.iter().all(|event| {
            event.kind != "engine_fallback"
                && event.kind != "engine_fallback_skipped"
                && event.kind != "fallback_exhausted"
        }));
    }

    /// Continuity never bypasses the permission contract: an authorized,
    /// healthy fallback target that can only weaken the session profile is
    /// skipped with the reason on record, and its engine is never even built.
    #[tokio::test]
    async fn fallback_skips_targets_that_would_weaken_the_permission_profile() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![authorized_claude(), codex])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Error {
            message: "engine stream died mid-run".into(),
            recoverable: false,
        }]));

        let outcome = run_persisted(
            &store,
            {
                let alva = Arc::clone(&alva);
                move |target: &crate::store::EngineTarget, _: &str, _| match target
                    .engine_id
                    .as_str()
                {
                    "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                    other => Err(format!(
                        "engine '{other}' must not be built for a weaken-only profile mapping"
                    )),
                }
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: Some(PermissionProfile::Interactive),
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        // Only the source attempt exists — no fallback attempt ever started.
        assert_eq!(detail.turns[0].attempts.len(), 1);
        let skips: Vec<_> = detail
            .timeline_events
            .iter()
            .filter(|event| event.kind == "engine_fallback_skipped")
            .collect();
        assert_eq!(
            skips
                .iter()
                .map(|event| event.target_engine_id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude", "codex"]
        );
        for skip in &skips {
            assert!(
                skip.reason.contains("cannot preserve or tighten")
                    && skip.reason.contains("interactive"),
                "skip reason must explain the permission ineligibility: {}",
                skip.reason
            );
        }
        let exhausted = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "fallback_exhausted")
            .expect("chain exhaustion card recorded");
        assert!(exhausted.reason.contains("claude: skipped"));
        assert!(exhausted.reason.contains("codex: skipped"));
    }

    #[tokio::test]
    async fn permission_request_event_carries_engine_session_routing_identity() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "alva-native-7".into(),
                model: None,
                tools: vec![],
            },
            RuntimeEvent::PermissionRequest {
                request_id: "req-42".into(),
                tool_name: "bash".into(),
                tool_input: serde_json::json!({"command": "rm -rf build"}),
                description: Some("Run a shell command".into()),
            },
            RuntimeEvent::Completed {
                session_id: "alva-native-7".into(),
                result: Some("done".into()),
                usage: None,
            },
        ]));

        let mut events = Vec::new();
        run_persisted(
            &store,
            single_engine(alva),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: Some(PermissionProfile::Interactive),
            },
            |event| events.push(event),
        )
        .await
        .unwrap();

        let request_event = events
            .iter()
            .find(|event| event.status == TimelineStatus::PermissionRequired)
            .expect("permission request surfaced on the timeline");
        let info = request_event
            .permission_request
            .as_ref()
            .expect("routing identity attached");
        assert_eq!(info.request_id, "req-42");
        assert_eq!(info.tool_name, "bash");
        assert_eq!(info.description.as_deref(), Some("Run a shell command"));
        // The identity pins the decision to the emitting native session.
        assert_eq!(info.engine_session_id, "alva-native-7");
        assert_eq!(request_event.engine_id, "alva");
    }

    /// A PermissionRequest that arrives before SessionStarted has no routing
    /// identity: the card must carry no controls (nothing can answer it) and
    /// must never substitute an empty engine session id.
    #[tokio::test]
    async fn premature_permission_request_surfaces_without_routing_controls() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::PermissionRequest {
                request_id: "req-early".into(),
                tool_name: "bash".into(),
                tool_input: serde_json::json!({}),
                description: None,
            },
            RuntimeEvent::SessionStarted {
                session_id: "alva-native-1".into(),
                model: None,
                tools: vec![],
            },
            RuntimeEvent::Completed {
                session_id: "alva-native-1".into(),
                result: Some("done".into()),
                usage: None,
            },
        ]));

        let mut events = Vec::new();
        run_persisted(
            &store,
            single_engine(alva),
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: Some(PermissionProfile::Interactive),
            },
            |event| events.push(event),
        )
        .await
        .unwrap();

        let request_event = events
            .iter()
            .find(|event| event.status == TimelineStatus::PermissionRequired)
            .expect("the premature request still surfaces");
        assert!(request_event.permission_request.is_none());
        assert!(request_event
            .message
            .as_deref()
            .is_some_and(|message| message.contains("cannot be answered")));
    }

    /// Once an attempt carries a terminal error the stream is only being
    /// drained — a pending approval must not disarm the watchdog forever.
    #[tokio::test(start_paused = true)]
    async fn doomed_attempt_with_pending_approval_drains_within_the_stall_window() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();

        let outcome = run_persisted(
            &store,
            |_: &crate::store::EngineTarget, _: &str, _| {
                Ok(Arc::new(ScriptedEngine {
                    script: vec![
                        (0, session_started("alva-native")),
                        (
                            0,
                            RuntimeEvent::PermissionRequest {
                                request_id: "req-1".into(),
                                tool_name: "bash".into(),
                                tool_input: serde_json::json!({}),
                                description: None,
                            },
                        ),
                        (
                            0,
                            RuntimeEvent::Error {
                                message: "engine backend died".into(),
                                recoverable: false,
                            },
                        ),
                    ],
                    hold_open_at_end: true,
                }) as Arc<dyn EngineRuntime>)
            },
            healthy,
            RunRequest {
                prompt: "do the work".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: Some(PermissionProfile::Interactive),
            },
            |_| {},
        )
        .await
        .unwrap();

        // Completing at all proves the drain is bounded; the recorded failure
        // stays the original terminal error, not a stall.
        assert!(outcome.failed);
        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert!(detail.turns[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("engine backend died")));
    }

    #[tokio::test]
    async fn resume_rejects_a_changed_permission_profile_loudly() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "alva-native-1".into(),
                model: None,
                tools: vec![],
            },
            RuntimeEvent::Completed {
                session_id: "alva-native-1".into(),
                result: Some("done".into()),
                usage: None,
            },
        ]));

        let outcome = run_persisted(
            &store,
            single_engine(Arc::clone(&alva)),
            healthy,
            RunRequest {
                prompt: "first".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: Some(PermissionProfile::Interactive),
            },
            |_| {},
        )
        .await
        .unwrap();

        let error = run_persisted(
            &store,
            single_engine(alva),
            healthy,
            RunRequest {
                prompt: "second".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some(outcome.product_session_id.clone()),
                permission_profile: Some(PermissionProfile::Unrestricted),
            },
            |_| {},
        )
        .await
        .expect_err("a session profile must not be changeable after creation");
        assert!(error.contains("'interactive'"));
        assert!(error.contains("'unrestricted'"));

        // Restating the stored profile (or omitting it) still resumes.
        run_persisted(
            &store,
            single_engine(Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
                session_id: "alva-native-1".into(),
                result: Some("again".into()),
                usage: None,
            }]))),
            healthy,
            RunRequest {
                prompt: "third".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some(outcome.product_session_id),
                permission_profile: Some(PermissionProfile::Interactive),
            },
            |_| {},
        )
        .await
        .unwrap();
    }
    /// Real-CLI smoke gate: the codex binary on PATH plus a signed-in
    /// `~/.codex/auth.json`. Like the worktree git gate, a missing engine
    /// panics with an explanation instead of silently skipping — these tests
    /// are the only packaged-runtime observation of a real fallback target.
    fn real_codex_available() -> bool {
        let on_path = std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths).any(|directory| directory.join("codex").is_file())
            })
            .unwrap_or(false);
        let authed = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".codex/auth.json").is_file())
            .unwrap_or(false);
        let opted_out = std::env::var("ALVA_ALLOW_SKIP_REAL_ENGINE_TESTS")
            .map(|value| matches!(value.trim(), "1" | "true" | "yes"))
            .unwrap_or(false);
        if (!on_path || !authed) && !opted_out {
            panic!(
                "real-engine tests need the codex CLI on PATH with ~/.codex/auth.json; \
                 they are the only packaged-runtime observation of a real fallback target. \
                 Set ALVA_ALLOW_SKIP_REAL_ENGINE_TESTS=1 to skip deliberately."
            );
        }
        on_path && authed
    }

    fn real_codex_adapter(workspace: &std::path::Path) -> Arc<dyn EngineRuntime> {
        Arc::new(CodexAdapter::new(CodexAdapterConfig {
            default_cwd: Some(workspace.to_path_buf()),
            ..Default::default()
        }))
    }

    fn authorized_codex() -> crate::store::EngineTarget {
        let mut codex = crate::store::EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        codex
    }

    /// Real-CLI gate for the claude light (print) mode: binary on PATH plus
    /// either a credentials file or an oauthAccount in ~/.claude.json — the
    /// same detection the readiness probe uses.
    fn real_claude_available() -> bool {
        use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
        let on_path = std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths).any(|directory| directory.join("claude").is_file())
            })
            .unwrap_or(false);
        let authed = std::env::var_os("HOME")
            .map(|home| {
                let home = PathBuf::from(home);
                home.join(".claude/.credentials.json").is_file()
                    || std::fs::read_to_string(home.join(".claude.json"))
                        .map(|config| config.contains("oauthAccount"))
                        .unwrap_or(false)
            })
            .unwrap_or(false);
        let opted_out = std::env::var("ALVA_ALLOW_SKIP_REAL_ENGINE_TESTS")
            .map(|value| matches!(value.trim(), "1" | "true" | "yes"))
            .unwrap_or(false);
        if (!on_path || !authed) && !opted_out {
            panic!(
                "real-claude tests need the claude CLI on PATH with credentials; \
                 set ALVA_ALLOW_SKIP_REAL_ENGINE_TESTS=1 to skip deliberately."
            );
        }
        let _ = ClaudePrintAdapter::new(ClaudePrintConfig::default());
        on_path && authed
    }

    #[tokio::test]
    async fn real_claude_runs_and_completes_a_turn() {
        if !real_claude_available() {
            return;
        }
        use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            {
                let workspace = workspace.path().to_path_buf();
                move |target: &crate::store::EngineTarget, _: &str, _| {
                    assert_eq!(target.engine_id, "claude");
                    Ok(Arc::new(ClaudePrintAdapter::new(ClaudePrintConfig {
                        default_cwd: Some(workspace.clone()),
                        permission_mode:
                            alva_engine_adapter_claude::ClaudePrintPermissionMode::DontAsk,
                        ..Default::default()
                    })) as Arc<dyn EngineRuntime>)
                }
            },
            healthy,
            RunRequest {
                prompt: "Answer in one short sentence: what is 7+7?".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "claude".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();
        assert!(
            !outcome.failed,
            "real claude turn must not fail: {timeline:?}"
        );
        let result = outcome.result.unwrap_or_default();
        assert!(
            result.contains('1') && result.contains('4'),
            "real claude answered 14: {result}"
        );
    }

    #[tokio::test]
    async fn real_codex_runs_and_completes_a_turn() {
        if !real_codex_available() {
            return;
        }
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            {
                let workspace = workspace.path().to_path_buf();
                move |target: &crate::store::EngineTarget, _: &str, _| {
                    assert_eq!(target.engine_id, "codex");
                    Ok(real_codex_adapter(&workspace))
                }
            },
            healthy,
            RunRequest {
                prompt: "Answer in one short sentence: what is 2+2?".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "codex".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();
        assert!(
            !outcome.failed,
            "real codex turn must not fail: {timeline:?}"
        );
        let result = outcome.result.unwrap_or_default();
        assert!(
            result.contains('4'),
            "real codex answered the arithmetic: {result}"
        );
    }

    /// Acceptance scenario 3, packaged-runtime half: a failing engine falls
    /// back onto the REAL codex CLI, and the real binary completes the turn.
    #[tokio::test]
    async fn real_codex_receives_a_fallback_after_engine_failure() {
        if !real_codex_available() {
            return;
        }
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        store
            .set_global_fallback_policy(vec![authorized_codex()])
            .unwrap();

        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "alva-native".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Error {
                message: "engine stream died mid-run".into(),
                recoverable: false,
            },
        ]));
        let workspace_path = workspace.path().to_path_buf();
        let mut timeline = Vec::new();
        let outcome = run_persisted(
            &store,
            move |target: &crate::store::EngineTarget, _: &str, _| match target.engine_id.as_str() {
                "alva" => Ok(Arc::clone(&alva) as Arc<dyn EngineRuntime>),
                "codex" => Ok(real_codex_adapter(&workspace_path)),
                other => Err(format!("unexpected engine '{other}'")),
            },
            healthy,
            RunRequest {
                prompt: "Answer in one short sentence: what is 2+2?".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
                permission_profile: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();
        assert!(
            !outcome.failed,
            "fallback to the real codex CLI must complete: {timeline:?}"
        );
        let result = outcome.result.unwrap_or_default();
        assert!(
            result.contains('4'),
            "real codex answered after fallback: {result}"
        );
    }

    /// Worker queue → REAL engine execution → result write-back: the
    /// worker session runs run_persisted on the real codex CLI and the
    /// queued task closes with the real result (the Lead channel then sees
    /// it). Same gate as the other real-engine tests.
    #[tokio::test]
    async fn real_codex_worker_task_executes_and_closes_with_result() {
        if !real_codex_available() {
            return;
        }
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = crate::store::EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "worker-lead",
                workspace.path(),
                "lead plans",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("plan done"), None, None)
            .unwrap();

        let run = store
            .create_team_run("worker-lead", &authorized_codex())
            .unwrap();
        let queued = store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "Answer in one short sentence: what is 2+2?",
                &[],
            )
            .unwrap();
        assert_eq!(queued.status, "queued");
        let claimed = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.status, "running");

        // The worker session runs its task on the REAL codex CLI.
        let workspace_path = workspace.path().to_path_buf();
        let workspace_for_build = workspace_path.clone();
        let worker_session_id = run.workers[0].worker_session_id.clone();
        let outcome = run_persisted(
            &store,
            move |target: &crate::store::EngineTarget, _: &str, _| {
                assert_eq!(target.engine_id, "codex");
                Ok(real_codex_adapter(&workspace_for_build))
            },
            healthy,
            RunRequest {
                prompt: claimed.scope.clone(),
                workspace: workspace_path,
                engine_id: "codex".into(),
                product_session_id: Some(worker_session_id.clone()),
                permission_profile: None,
            },
            |_| {},
        )
        .await
        .unwrap();
        assert!(!outcome.failed, "real worker turn must not fail");

        // The task closes with the real result for the Lead channel.
        let finished = store
            .finish_worker_task(&claimed.task_id, !outcome.failed, outcome.result.as_deref())
            .unwrap();
        assert_eq!(finished.status, "completed");
        let result = finished.result.unwrap_or_default();
        assert!(
            result.contains('4'),
            "worker result reached the queue: {result}"
        );
        // The worker session persisted its turn evidence too.
        let detail = store.get_session(&worker_session_id).unwrap().unwrap();
        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.turns[0].status, "completed");
    }
}
