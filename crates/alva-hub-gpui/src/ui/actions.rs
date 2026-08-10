// INPUT:  HubApp store / active_engines / permission_router
// OUTPUT: user actions — approval answers, Stop, handoff expansion, Details
//         toggle, transcript sharing toggle.
// POS:    1:1 GPUI port of the React handlers in App.tsx.

use std::sync::Arc;

use alva_engine_runtime::PermissionDecision;
use gpui::*;

use alva_hub_gpui::TimelineEvent;

use super::app::{with_window, HubApp};
use super::ProjectedEvent;

impl HubApp {
    pub fn details_open(&self) -> bool {
        self.details_open
    }

    /// Append a memory entry and reflect the fresh list on the open
    /// session; the store returns the updated list so the drawer never
    /// guesses (React `addMemory`).
    pub fn add_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pid) = self.active_session_id.clone() else {
            return;
        };
        let entry = self.memory_draft.read(cx).value().to_string();
        if entry.trim().is_empty() {
            return;
        }
        match self.store.add_session_memory(&pid, &entry) {
            Ok(memory) => {
                if let Some(detail) = &mut self.session_detail {
                    detail.memory = memory;
                }
                self.memory_draft.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                });
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
        cx.notify();
    }

    pub fn toggle_details(&mut self, cx: &mut Context<Self>) {
        self.details_open = !self.details_open;
        cx.notify();
    }

    /// Persist the transcript-sharing consent; on failure the display follows
    /// the store (React `setTranscriptSharing_backend`).
    pub fn toggle_transcript_sharing(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(pid) = self.active_session_id.clone() else {
            return;
        };
        match self.store.set_transcript_sharing(&pid, enabled) {
            Ok(()) => self.transcript_sharing = enabled,
            Err(error) => {
                self.transcript_sharing = !enabled;
                self.push_local_failure(&error, cx);
            }
        }
        cx.notify();
    }

    /// Answer one pending approval through the router (React
    /// `answerPermission`), then audit the decision on the session.
    pub fn answer_permission(
        &self,
        product_session_id: &str,
        request_id: &str,
        allow: bool,
        cx: &mut Context<Self>,
    ) {
        let router = Arc::clone(&self.permission_router);
        let pid = product_session_id.to_string();
        let request_id = request_id.to_string();
        let decision = if allow {
            PermissionDecision::Allow {
                updated_input: None,
            }
        } else {
            PermissionDecision::Deny {
                message: "Denied by the user".to_string(),
            }
        };
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let result = router.respond(&pid, &request_id, decision).await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(()) => {
                        let summary =
                            format!("{} {request_id}", if allow { "allowed" } else { "denied" });
                        let _ = hub.store.record_permission_decision(&pid, &summary);
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    /// Stop the active attempt: record the cancel fact, clear its pending
    /// approvals, then cancel the live adapter (React `stopActiveAttempt`).
    pub fn stop_attempt(&mut self, product_session_id: &str, cx: &mut Context<Self>) {
        let Some(detail) = self.store.get_session(product_session_id).ok().flatten() else {
            self.push_local_failure("This Hub session no longer exists.", cx);
            return;
        };
        if detail.session.status != "running" {
            self.push_local_failure("This Hub session has no running attempt to stop.", cx);
            return;
        }
        let engine = self
            .active_engines
            .lock()
            .unwrap()
            .get(product_session_id)
            .cloned();
        let engine_session_id = detail.session.engine_session_id.clone();
        let engine_id = detail.session.engine_id.clone();
        let pid = product_session_id.to_string();
        if let Err(error) = self
            .store
            .record_attempt_cancelled(&pid, "User stopped the attempt")
        {
            self.push_local_failure(&error, cx);
        }
        // The stopped attempt's approvals die with it — clear them before the
        // cancel so a stale Allow can no longer reach the cancelled session.
        self.permission_router.clear_session(&pid);
        let weak = cx.entity().downgrade();
        let handle = self.window_handle;
        cx.spawn(async move |_this, cx| {
            if let (Some(engine), Some(session_id)) = (engine, engine_session_id) {
                let _ = engine.cancel(&session_id).await;
            }
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.timeline.push(ProjectedEvent::from_live(TimelineEvent {
                        status: alva_hub_gpui::TimelineStatus::Interrupted,
                        engine_id: engine_id.clone(),
                        product_session_id: pid.clone(),
                        message: Some("User stopped the attempt".to_string()),
                        permission_request: None,
                    }));
                    hub.refresh_sessions();
                    let reload = hub.active_session_id.clone();
                    with_window(cx, handle, move |hub, window, cx| {
                        if let Some(pid) = reload {
                            let _ = hub.load_session(&pid, window, cx);
                        }
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Expand/collapse a fallback card's handoff. Opening loads the
    /// persisted handoff for this event if the projection did not carry one
    /// (live run events), matching React's on-demand expansion.
    pub fn toggle_handoff(
        &mut self,
        index: usize,
        product_session_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.open_handoffs.get(&index).copied().unwrap_or(false);
        if open {
            self.open_handoffs.insert(index, false);
            cx.notify();
            return;
        }
        let Some(event) = self.timeline.get_mut(index) else {
            return;
        };
        let Some(fallback) = &mut event.fallback else {
            return;
        };
        if fallback.handoff.is_none() {
            if let Ok(Some(detail)) = self.store.get_session(product_session_id) {
                let source = fallback.source_engine_id.clone();
                fallback.handoff = detail
                    .handoffs
                    .iter()
                    .rev()
                    .find(|handoff| handoff.source.engine_id == source)
                    .cloned();
            }
        }
        self.open_handoffs.insert(index, true);
        cx.notify();
    }
}
