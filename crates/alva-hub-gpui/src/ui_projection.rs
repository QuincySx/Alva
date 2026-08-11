// INPUT:  HubSessionDetail / TimelineEvent DTOs
// OUTPUT: projected timeline cards plus chat-first message/activity rows
// POS:    Pure Hub UI projection seam shared by persisted and live surfaces.

use crate::run::{PermissionRequestInfo, TimelineEvent, TimelineStatus};
use crate::store::{HandoffPackage, HubSessionDetail, StoredTimelineEvent, StoredTurn};

/// A fallback card's reversible controls: where it came from and the
/// handoff it actually delivered (loaded from the store on demand).
#[derive(Debug, Clone)]
pub struct FallbackControls {
    pub source_engine_id: String,
    pub handoff: Option<HandoffPackage>,
}

/// One timeline card. `created_at` is absent for live run events (they are
/// already in arrival order); persisted projections always carry it.
#[derive(Debug, Clone)]
pub struct ProjectedEvent {
    pub status: TimelineStatus,
    pub engine_id: String,
    pub product_session_id: String,
    pub message: Option<String>,
    pub permission_request: Option<PermissionRequestInfo>,
    pub created_at: Option<i64>,
    pub fallback: Option<FallbackControls>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatItemKind {
    User,
    Assistant,
    Activity,
}

#[derive(Debug, Clone)]
pub struct ProjectedChatItem {
    pub kind: ChatItemKind,
    pub engine_id: String,
    pub message: String,
    pub status: Option<TimelineStatus>,
    pub timeline_index: Option<usize>,
    pub created_at: Option<i64>,
}

impl ProjectedChatItem {
    pub fn user(message: impl Into<String>, created_at: Option<i64>) -> Self {
        Self {
            kind: ChatItemKind::User,
            engine_id: String::new(),
            message: message.into(),
            status: None,
            timeline_index: None,
            created_at,
        }
    }

    pub fn from_live(event: &ProjectedEvent, timeline_index: usize) -> Self {
        let kind = if matches!(
            event.status,
            TimelineStatus::Completed | TimelineStatus::Streaming
        ) && event.message.is_some()
        {
            ChatItemKind::Assistant
        } else {
            ChatItemKind::Activity
        };
        Self {
            kind,
            engine_id: event.engine_id.clone(),
            message: event.message.clone().unwrap_or_else(|| {
                if event.status == TimelineStatus::Running {
                    "Working in the selected workspace…".to_string()
                } else {
                    "Run finished.".to_string()
                }
            }),
            status: Some(event.status),
            timeline_index: Some(timeline_index),
            created_at: event.created_at,
        }
    }
}

impl ProjectedEvent {
    /// A live run event (no timestamp; already in arrival order).
    pub fn from_live(event: TimelineEvent) -> Self {
        Self {
            status: event.status,
            engine_id: event.engine_id,
            product_session_id: event.product_session_id,
            message: event.message,
            permission_request: event.permission_request,
            created_at: None,
            fallback: None,
        }
    }
}

fn message_for_timeline_event(event: &StoredTimelineEvent) -> String {
    if event.kind == "engine_fallback_skipped" {
        format!("Skipped {}\n\n{}", event.target_engine_id, event.reason)
    } else if matches!(
        event.kind.as_str(),
        "fallback_exhausted"
            | "attempt_cancelled"
            | "worker_outcome"
            | "permission_decision"
            | "resource_wait"
            | "worktree_adopted"
            | "worktree_discarded"
    ) {
        event.reason.clone()
    } else {
        format!(
            "{} → {}\n\n{}",
            event.source_engine_id, event.target_engine_id, event.reason
        )
    }
}

fn status_for_kind(kind: &str) -> TimelineStatus {
    match kind {
        "engine_switch_queued" => TimelineStatus::Queued,
        "engine_switch_cancelled" | "attempt_cancelled" => TimelineStatus::Cancelled,
        "engine_fallback" | "engine_fallback_skipped" => TimelineStatus::Fallback,
        "fallback_exhausted" => TimelineStatus::Exhausted,
        "permission_decision" => TimelineStatus::PermissionRequired,
        "worker_outcome" | "resource_wait" | "worktree_adopted" | "worktree_discarded" => {
            TimelineStatus::Worker
        }
        _ => TimelineStatus::Switched,
    }
}

fn project_turn(turn: &StoredTurn, session_engine: &str) -> ProjectedEvent {
    let status = match turn.status.as_str() {
        "failed" => TimelineStatus::Failed,
        "running" => TimelineStatus::Running,
        "interrupted" => TimelineStatus::Interrupted,
        _ => TimelineStatus::Completed,
    };
    let message = [turn.prompt.as_str(), turn.result.as_deref().unwrap_or("")]
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");
    ProjectedEvent {
        status,
        engine_id: turn
            .attempts
            .last()
            .map(|attempt| attempt.engine_id.clone())
            .unwrap_or_else(|| session_engine.to_string()),
        product_session_id: String::new(),
        message: Some(message),
        permission_request: None,
        created_at: Some(turn.created_at),
        fallback: None,
    }
}

/// Project a stored session detail into timeline cards, matching React
/// `projectTimeline` (turns + timeline events, sorted by created_at).
pub fn project_timeline(detail: &HubSessionDetail) -> Vec<ProjectedEvent> {
    let mut events: Vec<ProjectedEvent> = detail
        .turns
        .iter()
        .map(|turn| project_turn(turn, &detail.session.engine_id))
        .collect();
    for event in &detail.timeline_events {
        let fallback = if event.kind == "engine_fallback" {
            Some(FallbackControls {
                source_engine_id: event.source_engine_id.clone(),
                handoff: detail
                    .handoffs
                    .iter()
                    .find(|handoff| {
                        Some(handoff.target_branch_id.as_str()) == event.target_branch_id.as_deref()
                            && handoff.created_at == event.created_at
                    })
                    .cloned(),
            })
        } else {
            None
        };
        events.push(ProjectedEvent {
            status: status_for_kind(&event.kind),
            engine_id: event.target_engine_id.clone(),
            product_session_id: detail.session.product_session_id.clone(),
            message: Some(message_for_timeline_event(event)),
            permission_request: None,
            created_at: Some(event.created_at),
            fallback,
        });
    }
    events.sort_by_key(|event| event.created_at.unwrap_or(0));
    events
}

/// Project a stored session into the single ordered conversation surface.
/// A turn is deliberately split into user and assistant rows; operational
/// records remain compact activity rows and retain their timeline index so
/// permission and fallback controls keep working.
pub fn project_chat(detail: &HubSessionDetail) -> Vec<ProjectedChatItem> {
    let timeline = project_timeline(detail);
    let mut rows: Vec<(i64, u8, ProjectedChatItem)> = Vec::new();

    for turn in &detail.turns {
        rows.push((
            turn.created_at,
            0,
            ProjectedChatItem::user(turn.prompt.clone(), Some(turn.created_at)),
        ));
        let status = match turn.status.as_str() {
            "failed" => TimelineStatus::Failed,
            "running" => TimelineStatus::Running,
            "interrupted" => TimelineStatus::Interrupted,
            _ => TimelineStatus::Completed,
        };
        let message = turn
            .result
            .clone()
            .or_else(|| turn.error.clone())
            .unwrap_or_else(|| {
                if status == TimelineStatus::Running {
                    "Working in the selected workspace…".to_string()
                } else {
                    "No response was recorded.".to_string()
                }
            });
        rows.push((
            turn.completed_at.unwrap_or(turn.created_at),
            1,
            ProjectedChatItem {
                kind: ChatItemKind::Assistant,
                engine_id: turn
                    .attempts
                    .last()
                    .map(|attempt| attempt.engine_id.clone())
                    .unwrap_or_else(|| detail.session.engine_id.clone()),
                message,
                status: Some(status),
                timeline_index: None,
                created_at: Some(turn.completed_at.unwrap_or(turn.created_at)),
            },
        ));
    }

    for (timeline_index, event) in timeline.iter().enumerate() {
        let Some(created_at) = event.created_at else {
            continue;
        };
        let is_turn_card = detail
            .turns
            .iter()
            .any(|turn| turn.created_at == created_at && event.fallback.is_none());
        if !is_turn_card {
            rows.push((
                created_at,
                2,
                ProjectedChatItem::from_live(event, timeline_index),
            ));
        }
    }

    rows.sort_by_key(|(timestamp, phase, _)| (*timestamp, *phase));
    rows.into_iter().map(|(_, _, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::PermissionProfile;
    use crate::store::{
        EngineTarget, FallbackPolicy, HandoffKind, HubSessionSummary, StoredAttempt, StoredBranch,
    };

    fn summary() -> HubSessionSummary {
        HubSessionSummary {
            product_session_id: "s1".into(),
            workspace: "/tmp/ws".into(),
            active_branch_id: "b1".into(),
            engine_id: "alva".into(),
            model_id: "m".into(),
            provider_id: "p".into(),
            engine_session_id: Some("es1".into()),
            status: "completed".into(),
            updated_at: 1000,
            last_prompt: Some("run tests".into()),
            last_result: Some("ok".into()),
            permission_profile: PermissionProfile::WorkspaceWrite,
            include_full_transcript: false,
        }
    }

    fn handoff() -> HandoffPackage {
        HandoffPackage {
            handoff_id: "h1".into(),
            product_session_id: "s1".into(),
            source_branch_id: "b1".into(),
            target_branch_id: "b2".into(),
            source: EngineTarget::for_engine("alva").unwrap(),
            target: EngineTarget::for_engine("codex").unwrap(),
            kind: HandoffKind::Full,
            reason: "codex continued".into(),
            user_goal: None,
            latest_instruction: None,
            accepted_decisions: vec![],
            completed_work: vec![],
            pending_work: vec![],
            changed_files: vec![],
            recent_commands: vec![],
            verification_results: vec![],
            workspace_summary: String::new(),
            known_side_effects: vec![],
            team_snapshot: None,
            transcript: vec![],
            created_at: 300,
            used_at: None,
            session_memory: vec![],
            available_skills: vec![],
            available_mcp: vec![],
        }
    }

    #[test]
    fn project_timeline_sorts_and_matches_fallback_handoff() {
        let turn = StoredTurn {
            turn_id: "t1".into(),
            prompt: "run tests".into(),
            result: Some("ok".into()),
            error: None,
            status: "completed".into(),
            created_at: 200,
            completed_at: Some(300),
            attempts: vec![StoredAttempt {
                attempt_id: "a1".into(),
                turn_id: "t1".into(),
                branch_id: "b1".into(),
                engine_id: "alva".into(),
                status: "completed".into(),
                failure_reason: None,
                usage: None,
                side_effects: vec![],
                started_at: 200,
                completed_at: Some(300),
            }],
        };
        let fallback_event = StoredTimelineEvent {
            event_id: "e1".into(),
            kind: "engine_fallback".into(),
            source_branch_id: "b1".into(),
            target_branch_id: Some("b2".into()),
            source_engine_id: "alva".into(),
            target_engine_id: "codex".into(),
            reason: "alva failed".into(),
            created_at: 300,
        };
        let detail = HubSessionDetail {
            session: summary(),
            turns: vec![turn],
            branches: vec![StoredBranch {
                branch_id: "b1".into(),
                engine_id: "alva".into(),
                model_id: "m".into(),
                provider_id: "p".into(),
                engine_session_id: Some("es1".into()),
                watermark_turn_rowid: None,
                status: "parked".into(),
                created_at: 100,
                updated_at: 300,
            }],
            handoffs: vec![handoff()],
            timeline_events: vec![fallback_event],
            pending_switch: None,
            fallback_policy: FallbackPolicy {
                targets: vec![],
                updated_at: 0,
            },
            memory: vec![],
        };
        let projected = project_timeline(&detail);
        assert_eq!(projected.len(), 2);
        assert!(matches!(projected[0].status, TimelineStatus::Completed));
        let fallback = projected[1].fallback.as_ref().expect("fallback controls");
        assert_eq!(fallback.source_engine_id, "alva");
        assert_eq!(fallback.handoff.as_ref().unwrap().target.engine_id, "codex");
        assert!(matches!(projected[1].status, TimelineStatus::Fallback));
    }

    #[test]
    fn project_timeline_maps_switch_and_cancel_kinds() {
        let events = vec![
            StoredTimelineEvent {
                event_id: "e1".into(),
                kind: "engine_switch_queued".into(),
                source_branch_id: "b1".into(),
                target_branch_id: Some("b2".into()),
                source_engine_id: "alva".into(),
                target_engine_id: "codex".into(),
                reason: "queued".into(),
                created_at: 400,
            },
            StoredTimelineEvent {
                event_id: "e2".into(),
                kind: "attempt_cancelled".into(),
                source_branch_id: "b2".into(),
                target_branch_id: None,
                source_engine_id: "codex".into(),
                target_engine_id: "codex".into(),
                reason: "user stopped".into(),
                created_at: 500,
            },
        ];
        let detail = HubSessionDetail {
            session: summary(),
            turns: vec![],
            branches: vec![],
            handoffs: vec![],
            timeline_events: events,
            pending_switch: None,
            fallback_policy: FallbackPolicy {
                targets: vec![],
                updated_at: 0,
            },
            memory: vec![],
        };
        let projected = project_timeline(&detail);
        assert!(matches!(projected[0].status, TimelineStatus::Queued));
        assert!(matches!(projected[1].status, TimelineStatus::Cancelled));
    }

    #[test]
    fn project_chat_splits_turns_and_keeps_operational_rows() {
        let mut detail = HubSessionDetail {
            session: summary(),
            turns: vec![StoredTurn {
                turn_id: "t1".into(),
                prompt: "inspect the project".into(),
                result: Some("done".into()),
                error: None,
                status: "completed".into(),
                created_at: 200,
                completed_at: Some(250),
                attempts: vec![],
            }],
            branches: vec![],
            handoffs: vec![],
            timeline_events: vec![],
            pending_switch: None,
            fallback_policy: FallbackPolicy {
                targets: vec![],
                updated_at: 0,
            },
            memory: vec![],
        };
        detail.timeline_events.push(StoredTimelineEvent {
            event_id: "e1".into(),
            kind: "engine_switch_queued".into(),
            source_branch_id: "b1".into(),
            target_branch_id: Some("b2".into()),
            source_engine_id: "alva".into(),
            target_engine_id: "codex".into(),
            reason: "queued".into(),
            created_at: 300,
        });

        let chat = project_chat(&detail);
        assert_eq!(chat.len(), 3);
        assert_eq!(chat[0].kind, ChatItemKind::User);
        assert_eq!(chat[0].message, "inspect the project");
        assert_eq!(chat[1].kind, ChatItemKind::Assistant);
        assert_eq!(chat[1].message, "done");
        assert_eq!(chat[2].kind, ChatItemKind::Activity);
        assert_eq!(chat[2].timeline_index, Some(1));
    }
}
