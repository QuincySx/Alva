// INPUT:  HubApp store / active_engines / permission_router / worker_pumps
// OUTPUT: per-worker queue pumps — claim the next task, run it through the
//         engine via run_persisted, finish with result, drain the queue.
// POS:    1:1 GPUI port of the Tauri worker pump (ensure_worker_pump +
//         run_worker_task) using the migrated store seams.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alva_engine_runtime::{EngineRuntime, PermissionDecision};
use gpui::*;

use alva_hub_gpui::{
    build_engine, run_persisted, PermissionMember, RunRequest, TaskClaim, WorkerTask,
};

use super::app::HubApp;

impl HubApp {
    /// Ensure a per-worker pump is draining this worker's durable FIFO
    /// queue. Dormant (hibernated/archived) workers are never respawned.
    pub fn ensure_worker_pump(&self, worker_id: &str, cx: &mut Context<Self>) {
        let worker_id = worker_id.to_string();
        {
            let mut pumps = self.worker_pumps.lock().unwrap();
            if !pumps.insert(worker_id.clone()) {
                return;
            }
        }
        let store = Arc::clone(&self.store);
        let engines = Arc::clone(&self.active_engines);
        let router = Arc::clone(&self.permission_router);
        let history_dir = self.store.path().parent().map(PathBuf::from);
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let mut dormant = false;
            loop {
                let claimed = store.claim_next_worker_task(&worker_id);
                match claimed {
                    Ok(TaskClaim::Claimed(task)) => {
                        run_worker_task(&store, &engines, &router, history_dir.clone(), *task)
                            .await;
                        if let Some(hub) = weak.upgrade() {
                            hub.update(cx, |hub, cx| {
                                hub.refresh_team_silent(cx);
                                cx.notify();
                            });
                        }
                    }
                    Ok(TaskClaim::Blocked { .. }) => {
                        // The head of this queue wants resources another
                        // task holds: wait — never reorder past it.
                        let timer = executor.timer(std::time::Duration::from_millis(250));
                        timer.await;
                    }
                    Ok(TaskClaim::Dormant) => {
                        dormant = true;
                        break;
                    }
                    Ok(TaskClaim::Empty) => break,
                    Err(error) => {
                        tracing::warn!(%error, worker = %worker_id, "worker queue pump stopped");
                        break;
                    }
                }
            }
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.worker_pumps.lock().unwrap().remove(&worker_id);
                    // A task enqueued between the final empty claim and the
                    // removal above would otherwise sit unconsumed. Dormant
                    // workers are skipped: their queue is parked on purpose.
                    if !dormant && hub.store.has_queued_task(&worker_id).unwrap_or(false) {
                        hub.ensure_worker_pump(&worker_id, cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

/// Run one claimed task through the worker's own Hub session, then finish
/// it with the outcome (React/Tauri `run_worker_task`).
async fn run_worker_task(
    store: &alva_hub_gpui::HubStore,
    engines: &Arc<Mutex<HashMap<String, Arc<dyn EngineRuntime>>>>,
    router: &Arc<alva_hub_gpui::PermissionRouter>,
    history_dir: Option<PathBuf>,
    task: WorkerTask,
) {
    let context = (|| -> Result<_, String> {
        let run = store
            .get_team_run(&task.team_run_id)?
            .ok_or("team run vanished")?;
        let worker = run
            .workers
            .iter()
            .find(|worker| worker.worker_id == task.worker_id)
            .cloned()
            .ok_or("worker vanished")?;
        let session = store
            .get_session(&worker.worker_session_id)?
            .ok_or("worker session vanished")?;
        Ok((run, worker, session))
    })();
    let (run, worker, session) = match context {
        Ok(context) => context,
        Err(error) => {
            tracing::warn!(%error, task = %task.task_id, "worker task context unavailable");
            let _ = store.finish_worker_task(&task.task_id, false, Some(&error));
            return;
        }
    };
    let workspace = PathBuf::from(&session.session.workspace);
    let session_key = worker.worker_session_id.clone();
    let approval_scope = store
        .collaboration_scope(&session_key)
        .unwrap_or_else(|_| session_key.clone());
    let autonomous_run = store.is_autonomous_scope(&session_key).unwrap_or(false);
    let requester_label = store
        .team_worker_label(&session_key)
        .ok()
        .flatten()
        .unwrap_or_else(|| format!("{} session", session.session.engine_id));
    let request = RunRequest {
        prompt: task.scope.clone(),
        workspace: workspace.clone(),
        engine_id: session.session.engine_id.clone(),
        product_session_id: Some(worker.worker_session_id.clone()),
        permission_profile: None,
    };
    // Permission requests raised during the run: registered on the router
    // immediately; auto-answered after the run when this scope is
    // autonomous (the user opted this run out of per-request approval).
    let auto_answers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let outcome = run_persisted(
        store,
        {
            let engines = Arc::clone(engines);
            let workspace = workspace.clone();
            let history = history_dir.clone().unwrap_or_else(|| workspace.clone());
            move |target, product_session_id, permission_profile| {
                let engine =
                    build_engine(&target.engine_id, permission_profile, &workspace, &history)?;
                engines
                    .lock()
                    .unwrap()
                    .insert(product_session_id.to_string(), Arc::clone(&engine));
                Ok(engine)
            }
        },
        |_| None,
        request,
        {
            let router = Arc::clone(router);
            let approval_scope = approval_scope.clone();
            let requester_label = requester_label.clone();
            let auto_answers = Arc::clone(&auto_answers);
            move |event| {
                if let Some(info) = &event.permission_request {
                    let engine = engines
                        .lock()
                        .unwrap()
                        .get(&event.product_session_id)
                        .cloned();
                    if let Some(engine) = engine {
                        router.register(
                            &event.product_session_id,
                            &info.request_id,
                            engine,
                            &info.engine_session_id,
                            PermissionMember {
                                scope: approval_scope.clone(),
                                product_session_id: event.product_session_id.clone(),
                                request_id: info.request_id.clone(),
                                requester: requester_label.clone(),
                                tool_name: info.tool_name.clone(),
                                resource: info.resource.clone(),
                            },
                        );
                    }
                    if autonomous_run {
                        auto_answers
                            .lock()
                            .unwrap()
                            .push((event.product_session_id.clone(), info.request_id.clone()));
                    }
                }
            }
        },
    )
    .await;
    let pending_answers = auto_answers.lock().unwrap().clone();
    for (pid, request_id) in pending_answers {
        let _ = router
            .respond(
                &pid,
                &request_id,
                PermissionDecision::Allow {
                    updated_input: None,
                },
            )
            .await;
    }
    let finished = match &outcome {
        Ok(outcome) => {
            store.finish_worker_task(&task.task_id, !outcome.failed, outcome.result.as_deref())
        }
        Err(error) => store.finish_worker_task(&task.task_id, false, Some(error)),
    };
    match finished {
        Ok(task) => {
            let summary = alva_hub_gpui::redact_text(&format!(
                "Worker ({}) {}{}",
                worker.engine_id,
                task.status,
                task.result
                    .as_deref()
                    .map(|result| format!(" — {result}"))
                    .unwrap_or_default()
            ));
            let _ = store.record_adoption(&run.team_run_id, &worker.worker_id, false, &summary);
        }
        Err(error) => tracing::warn!(%error, task = %task.task_id, "worker task finish failed"),
    }
}
