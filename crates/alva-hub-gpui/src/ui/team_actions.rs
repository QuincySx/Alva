// INPUT:  HubApp store / team_run / inputs
// OUTPUT: team actions — create run, add worker (isolated worktree),
//         dispatch/queue tasks, lifecycle, adoption review/confirm/discard,
//         task edit/cancel, autonomy. The queue pump lives in pump.rs.
// POS:    1:1 GPUI port of the React team handlers in App.tsx.

use std::sync::Arc;

use gpui::*;
use gpui_component::searchable_list::SearchableVec;

use alva_hub_gpui::{
    adopt_into_workspace, adoption_preview, create_worker_worktree, discard_worktree, parse_claims,
    EngineTarget, WorkerLifecycle,
};

use super::app::HubApp;
use super::WorkerOption;

impl HubApp {
    /// Keep the dispatch-target selector aligned with the run's workers.
    pub fn sync_dispatch_workers(&self, window: &mut Window, cx: &mut Context<Self>) {
        let options: Vec<WorkerOption> = self
            .team_run
            .as_ref()
            .map(|run| {
                run.workers
                    .iter()
                    .filter(|worker| worker.lifecycle != WorkerLifecycle::Archived)
                    .map(|worker| WorkerOption {
                        worker_id: worker.worker_id.clone(),
                        label: format!(
                            "To worker on {}{}",
                            worker.engine_id,
                            if worker.lifecycle == WorkerLifecycle::Hibernated {
                                " (hibernated)"
                            } else {
                                ""
                            }
                        ),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let current = self.dispatch_worker.read(cx).selected_value().cloned();
        self.dispatch_worker.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(options.clone()), window, cx);
            let still_valid = current
                .as_ref()
                .map(|value| options.iter().any(|option| &option.worker_id == value))
                .unwrap_or(false);
            if !still_valid {
                let index = if options.is_empty() {
                    None
                } else {
                    Some(gpui_component::IndexPath::new(0))
                };
                state.set_selected_index(index, window, cx);
            }
        });
    }

    pub fn create_team(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lead_id) = self.active_session_id.clone() else {
            return;
        };
        let engine = self
            .team_worker_engine
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| "codex".to_string());
        let target = match EngineTarget::for_engine(&engine) {
            Ok(target) => target,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        match self.store.create_team_run(&lead_id, &target) {
            Ok(run) => {
                self.team_run = Some(run);
                self.sync_dispatch_workers(window, cx);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    /// Add a worker; isolated workers get a managed worktree (git operations
    /// run off the UI thread).
    pub fn add_worker(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.team_run.clone() else {
            return;
        };
        let engine = self
            .team_worker_engine
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| "codex".to_string());
        let target = match EngineTarget::for_engine(&engine) {
            Ok(target) => target,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        let isolated = self.isolate_worker;
        let store = Arc::clone(&self.store);
        let worktree_root = self.worktree_root.clone();
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move {
                    let run_id = run.team_run_id.clone();
                    if !isolated {
                        return store.add_team_worker(&run_id, &target, None);
                    }
                    let registered = store.add_team_worker(&run_id, &target, None)?;
                    let worker_id = registered
                        .workers
                        .last()
                        .map(|worker| worker.worker_id.clone())
                        .ok_or("the worker was not registered")?;
                    let lead = store
                        .get_session(&registered.lead_session_id)?
                        .ok_or("The lead session no longer exists.")?;
                    let workspace = std::path::PathBuf::from(&lead.session.workspace);
                    let worktree = create_worker_worktree(&workspace, &worktree_root, &worker_id)?;
                    match store.attach_worker_worktree(&worker_id, &worktree) {
                        Ok(()) => store
                            .get_team_run(&run_id)?
                            .ok_or_else(|| format!("team run '{run_id}' was not found")),
                        Err(error) => {
                            let _ = discard_worktree(
                                &workspace,
                                &std::path::PathBuf::from(&worktree.path),
                                &worktree.branch,
                                &worktree_root,
                            );
                            Err(error)
                        }
                    }
                })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(run) => {
                        hub.team_run = Some(run);
                        hub.refresh_team_silent(cx);
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    /// Enqueue a task for the selected worker and start its queue pump.
    pub fn dispatch_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.team_run.clone() else {
            return;
        };
        let scope = self.team_scope.read(cx).value().to_string();
        if scope.trim().is_empty() {
            return;
        }
        let claims = parse_claims(&self.team_claims.read(cx).value());
        let worker_id = self
            .dispatch_worker
            .read(cx)
            .selected_value()
            .cloned()
            .or_else(|| {
                run.workers
                    .iter()
                    .find(|worker| worker.lifecycle != WorkerLifecycle::Archived)
                    .map(|worker| worker.worker_id.clone())
            });
        let Some(worker_id) = worker_id else {
            self.push_local_failure(
                "Every worker in this run is archived. Restore one first.",
                cx,
            );
            return;
        };
        self.team_scope.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        self.team_claims.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        let store = Arc::clone(&self.store);
        let weak = cx.entity().downgrade();
        let run_id = run.team_run_id.clone();
        let executor = cx.background_executor().clone();
        let worker_id_pump = worker_id.clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move {
                    store
                        .enqueue_worker_task(&run_id, &worker_id, &scope, &claims)
                        .map(|_task| ())
                })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(()) => {
                        hub.refresh_team_silent(cx);
                        hub.ensure_worker_pump(&worker_id_pump, cx);
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    pub fn change_worker_lifecycle(
        &mut self,
        worker_id: &str,
        lifecycle: WorkerLifecycle,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.store.set_worker_lifecycle(worker_id, lifecycle) {
            Ok(_) => {
                self.refresh_team_silent(cx);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    /// Interrupt a worker's running task (React `interruptWorker`).
    pub fn interrupt_worker(
        &mut self,
        worker_session_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Delegate rather than hand-roll: a worker's turn is stopped exactly
        // like any other attempt, and hub-desktop delegates the same way
        // (`interrupt_worker` -> `stop_attempt_inner`). The previous copy passed
        // `""` as the native session id, so every adapter's `cancel` missed its
        // session table and returned SessionNotFound — which was then dropped by
        // `let _ =`. It also skipped `record_attempt_cancelled` and
        // `permission_router.clear_session`, both of which `stop_attempt` does.
        self.stop_attempt(worker_session_id, cx);
        // `stop_attempt` records the cancellation synchronously before spawning,
        // so the team projection already reflects it here.
        self.refresh_team_silent(cx);
        cx.notify();
    }

    pub fn end_team_run(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.team_run.clone() else {
            return;
        };
        match self.store.complete_team_run(&run.team_run_id) {
            Ok(run) => {
                self.team_run = Some(run);
                self.refresh_team_silent(cx);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    pub fn toggle_autonomy(&mut self, enabled: bool, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.team_run.clone() else {
            return;
        };
        match self.store.set_team_autonomy(&run.team_run_id, enabled) {
            Ok(run) => {
                self.team_run = Some(run);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    /// Review what adopting this worker's worktree would change (git, off
    /// the UI thread).
    pub fn review_adoption(
        &mut self,
        worker_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((workspace, worktree, verification)) = self.adoption_context(worker_id, cx) else {
            return;
        };
        let weak = cx.entity().downgrade();
        let worker_id = worker_id.to_string();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move { adoption_preview(&workspace, &worktree, verification) })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(preview) => {
                        hub.adoption.insert(worker_id.clone(), Some(preview));
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    pub fn confirm_adoption(
        &mut self,
        worker_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.team_run.clone() else {
            return;
        };
        let Some((workspace, worktree, _)) = self.adoption_context(worker_id, cx) else {
            return;
        };
        let store = Arc::clone(&self.store);
        let run_id = run.team_run_id.clone();
        let worker_id = worker_id.to_string();
        let worker_id_ui = worker_id.clone();
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move {
                    let adopted = adopt_into_workspace(&workspace, &worktree)?;
                    store.record_adoption(
                        &run_id,
                        &worker_id,
                        true,
                        &format!("Adopted {} file(s): {}", adopted.len(), adopted.join(", ")),
                    )?;
                    Ok::<_, String>(())
                })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(()) => {
                        hub.adoption.insert(worker_id_ui.clone(), None);
                        hub.refresh_team_silent(cx);
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    pub fn discard_isolated_work(
        &mut self,
        worker_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.team_run.clone() else {
            return;
        };
        let worker = run
            .workers
            .iter()
            .find(|worker| worker.worker_id == worker_id)
            .cloned();
        let Some(worker) = worker else {
            return;
        };
        let (Some(path), Some(branch)) =
            (worker.worktree_path.clone(), worker.worktree_branch.clone())
        else {
            return;
        };
        let Some(lead) = self.store.get_session(&run.lead_session_id).ok().flatten() else {
            return;
        };
        let workspace = std::path::PathBuf::from(&lead.session.workspace);
        let worktree_root = self.worktree_root.clone();
        let store = Arc::clone(&self.store);
        let run_id = run.team_run_id.clone();
        let worker_id = worker_id.to_string();
        let worker_id_ui = worker_id.clone();
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move {
                    discard_worktree(
                        &workspace,
                        &std::path::PathBuf::from(&path),
                        &branch,
                        &worktree_root,
                    )?;
                    store.clear_worker_worktree(&worker_id, &workspace.to_string_lossy())?;
                    store.record_adoption(
                        &run_id,
                        &worker_id,
                        false,
                        "Discarded the isolated worktree without adopting its changes",
                    )?;
                    Ok::<_, String>(())
                })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(()) => {
                        hub.adoption.insert(worker_id_ui.clone(), None);
                        hub.refresh_team_silent(cx);
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    pub fn start_edit_task(
        &mut self,
        task_id: &str,
        scope: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_task = Some(task_id.to_string());
        self.editing_scope.update(cx, |state, cx| {
            state.set_value(scope, window, cx);
        });
        cx.notify();
    }

    pub fn save_task_edit(&mut self, task_id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let scope = self.editing_scope.read(cx).value().to_string();
        match self.store.update_worker_task_scope(task_id, &scope) {
            Ok(_) => {
                self.editing_task = None;
                self.refresh_team_silent(cx);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    pub fn cancel_task(&mut self, task_id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        match self.store.cancel_worker_task(task_id) {
            Ok(_) => {
                self.refresh_team_silent(cx);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    /// Reload the team run for the open lead session (sync store read).
    pub fn refresh_team_silent(&mut self, cx: &mut Context<Self>) {
        let Some(lead_id) = self.active_session_id.clone() else {
            return;
        };
        match self.store.team_run_for_lead(&lead_id) {
            Ok(Some(run)) => self.team_run = Some(run),
            Ok(None) => self.team_run = None,
            Err(error) => self.push_local_failure(&error, cx),
        }
    }

    /// (shared workspace, worktree path, verification evidence) for an
    /// isolated worker (React/Tauri `adoption_context`).
    fn adoption_context(
        &self,
        worker_id: &str,
        _cx: &Context<Self>,
    ) -> Option<(std::path::PathBuf, std::path::PathBuf, Vec<String>)> {
        let run = self.team_run.as_ref()?;
        let worker = run
            .workers
            .iter()
            .find(|worker| worker.worker_id == worker_id)?;
        let worktree = worker.worktree_path.clone()?;
        let lead = self.store.get_session(&run.lead_session_id).ok()??;
        let verification = run
            .tasks
            .iter()
            .filter(|task| task.worker_id == worker_id)
            .flat_map(|task| {
                task.verification.iter().map(|evidence| {
                    format!(
                        "{} {}",
                        evidence.tool_name,
                        match evidence.succeeded {
                            Some(true) => "ok",
                            Some(false) => "failed",
                            None => "ran",
                        }
                    )
                })
            })
            .collect();
        Some((
            std::path::PathBuf::from(&lead.session.workspace),
            std::path::PathBuf::from(worktree),
            verification,
        ))
    }
}
