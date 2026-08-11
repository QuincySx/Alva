// INPUT:  HubApp.team_run + store team seams
// OUTPUT: sidebar "Team" panel — create run, workers with lifecycle and
//         worktree controls, task queues, adoption previews, autonomy.
// POS:    1:1 GPUI port of the React team panel in App.tsx (management
//         surface; the queue pump lives in pump.rs).

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    collapsible::Collapsible,
    input::Input,
    select::Select,
    tag::Tag,
    ActiveTheme, *,
};

use alva_hub_gpui::{TeamRunDetail, TeamWorker, WorkerLifecycle, WorkerTask};

use super::app::{with_window, HubApp};

impl HubApp {
    pub fn team_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_dispatch_workers(window, cx);
        let collapsed = self.collapsed.get("team").copied().unwrap_or(true);
        let has_session = self.active_session_id.is_some();
        let count = self
            .team_run
            .as_ref()
            .map(|run| run.tasks.len())
            .unwrap_or(0);
        let weak = cx.entity().downgrade();
        let content = if !has_session {
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child("Open a session to lead a team run.")
                .into_any_element()
        } else if self.team_run.is_none() {
            self.team_create_form(window, cx).into_any_element()
        } else {
            self.team_content(window, cx).into_any_element()
        };
        Collapsible::new()
            .open(!collapsed)
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border)
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("toggle-team")
                            .ghost()
                            .xsmall()
                            .icon(if collapsed {
                                IconName::ChevronRight
                            } else {
                                IconName::ChevronDown
                            })
                            .on_click(move |_, _, app| {
                                if let Some(entity) = weak.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        let open =
                                            hub.collapsed.get("team").copied().unwrap_or(true);
                                        hub.collapsed.insert("team".to_string(), !open);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight(650.))
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Team"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(count.to_string()),
                    ),
            )
            .content(content)
    }

    /// Worker engine picker + Create team run (no run yet).
    fn team_create_form(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        v_flex()
            .gap_1_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Worker engine"),
                    )
                    .child(Select::new(&self.team_worker_engine).small()),
            )
            .child(
                Button::new("create-team-run")
                    .secondary()
                    .small()
                    .label("Create team run")
                    .on_click(move |_, window, app| {
                        if weak.upgrade().is_some() {
                            let handle = window.window_handle();
                            with_window(app, handle, |hub, window, cx| {
                                hub.create_team(window, cx);
                            });
                        }
                    }),
            )
    }

    /// Lead line, worker cards, dispatch queue and add-worker controls.
    fn team_content(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let run = self.team_run.clone();
        let Some(run) = run else {
            return div();
        };
        let lead_engine = self.selected_engine(cx);
        let show_archived = self.show_archived;
        let workers: Vec<TeamWorker> = run
            .workers
            .iter()
            .filter(|worker| show_archived || worker.lifecycle != WorkerLifecycle::Archived)
            .cloned()
            .collect();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(format!("Lead: {lead_engine}")),
                    )
                    .child(
                        Tag::new()
                            .with_variant(super::status_tag_variant(&run.status))
                            .small()
                            .rounded_full()
                            .child(run.status.clone()),
                    ),
            )
            .children(workers.iter().map(|worker| {
                let tasks: Vec<WorkerTask> = run
                    .tasks
                    .iter()
                    .filter(|task| task.worker_id == worker.worker_id)
                    .cloned()
                    .collect();
                self.worker_card(worker, &tasks, cx)
            }))
            .child(self.dispatch_form(&run, cx))
            .child(self.add_worker_form(&run, cx))
    }

    /// One worker: heading, lifecycle/worktree controls, tasks.
    fn worker_card(
        &self,
        worker: &TeamWorker,
        tasks: &[WorkerTask],
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let worker_running = tasks.iter().any(|task| task.status == "running");
        let worker_id = worker.worker_id.clone();
        let session_id = worker.worker_session_id.clone();
        let engine_id = worker.engine_id.clone();
        let lifecycle = worker.lifecycle;
        let worktree_path = worker.worktree_path.clone();
        let worktree_branch = worker.worktree_branch.clone();
        let adoption = self.adoption.get(&worker_id).cloned().flatten();
        // Each button closure gets its own clones (Fn closures cannot move
        // captured values out).
        let session_id_focus = session_id.clone();
        let session_id_interrupt = session_id.clone();
        let worker_id_hibernate = worker_id.clone();
        let worker_id_archive = worker_id.clone();
        let worker_id_restore = worker_id.clone();
        let worker_id_review = worker_id.clone();
        let worker_id_adopt = worker_id.clone();
        let worker_id_discard = worker_id.clone();
        let weak = cx.entity().downgrade();
        let weak_focus = weak.clone();
        let weak_interrupt = weak.clone();
        let weak_hibernate = weak.clone();
        let weak_archive = weak.clone();
        let weak_restore = weak.clone();
        let weak_review = weak.clone();
        let weak_adopt = weak.clone();
        let weak_discard = weak.clone();
        let mut card = v_flex()
            .gap_1()
            .px_2()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(600.))
                                    .child(format!("Worker · {engine_id}")),
                            )
                            .when(worktree_path.is_some(), |this| {
                                this.child(Tag::warning().small().rounded_full().child("isolated"))
                            })
                            .when(lifecycle != WorkerLifecycle::Active, |this| {
                                this.child(
                                    Tag::new()
                                        .with_variant(super::status_tag_variant(lifecycle.as_str()))
                                        .small()
                                        .rounded_full()
                                        .child(lifecycle.as_str()),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!("focus-{worker_id}")))
                                    .ghost()
                                    .xsmall()
                                    .label("Focus")
                                    .on_click(move |_, window, app| {
                                        if weak_focus.upgrade().is_some() {
                                            let handle = window.window_handle();
                                            let session_id_focus = session_id_focus.clone();
                                            with_window(app, handle, move |hub, window, cx| {
                                                let _ =
                                                    hub.load_session(&session_id_focus, window, cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("interrupt-{worker_id}")))
                                    .ghost()
                                    .xsmall()
                                    .label("Interrupt")
                                    .disabled(
                                        !worker_running || lifecycle != WorkerLifecycle::Active,
                                    )
                                    .on_click(move |_, window, app| {
                                        if weak_interrupt.upgrade().is_some() {
                                            let handle = window.window_handle();
                                            let session_id_interrupt = session_id_interrupt.clone();
                                            with_window(app, handle, move |hub, window, cx| {
                                                hub.interrupt_worker(
                                                    &session_id_interrupt,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            )
                            .when(lifecycle == WorkerLifecycle::Active, |this| {
                                this.child(
                                    Button::new(SharedString::from(format!(
                                        "hibernate-{worker_id}"
                                    )))
                                    .ghost()
                                    .xsmall()
                                    .label("Hibernate")
                                    .on_click(
                                        move |_, window, app| {
                                            if weak_hibernate.upgrade().is_some() {
                                                let handle = window.window_handle();
                                                let worker_id_hibernate =
                                                    worker_id_hibernate.clone();
                                                with_window(app, handle, move |hub, window, cx| {
                                                    hub.change_worker_lifecycle(
                                                        &worker_id_hibernate,
                                                        WorkerLifecycle::Hibernated,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        },
                                    ),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("archive-{worker_id}")))
                                        .ghost()
                                        .xsmall()
                                        .label("Archive")
                                        .on_click(move |_, window, app| {
                                            if weak_archive.upgrade().is_some() {
                                                let handle = window.window_handle();
                                                let worker_id_archive = worker_id_archive.clone();
                                                with_window(app, handle, move |hub, window, cx| {
                                                    hub.change_worker_lifecycle(
                                                        &worker_id_archive,
                                                        WorkerLifecycle::Archived,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                            })
                            .when(lifecycle != WorkerLifecycle::Active, |this| {
                                this.child(
                                    Button::new(SharedString::from(format!("restore-{worker_id}")))
                                        .ghost()
                                        .xsmall()
                                        .label("Restore")
                                        .on_click(move |_, window, app| {
                                            if weak_restore.upgrade().is_some() {
                                                let handle = window.window_handle();
                                                let worker_id_restore = worker_id_restore.clone();
                                                with_window(app, handle, move |hub, window, cx| {
                                                    hub.change_worker_lifecycle(
                                                        &worker_id_restore,
                                                        WorkerLifecycle::Active,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                            }),
                    ),
            );
        if let Some(branch) = &worktree_branch {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(format!(
                        "Worktree {branch} — changes stay here until you adopt them."
                    )),
            );
        }
        if worktree_path.is_some() {
            card = card.child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!("review-{worker_id}")))
                            .ghost()
                            .xsmall()
                            .label("Review changes")
                            .on_click(move |_, window, app| {
                                if weak_review.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let worker_id_review = worker_id_review.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.review_adoption(&worker_id_review, window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("adopt-{worker_id}")))
                            .ghost()
                            .xsmall()
                            .label("Adopt")
                            .disabled(adoption.is_none())
                            .on_click(move |_, window, app| {
                                if weak_adopt.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let worker_id_adopt = worker_id_adopt.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.confirm_adoption(&worker_id_adopt, window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("discard-{worker_id}")))
                            .ghost()
                            .xsmall()
                            .label("Discard worktree")
                            .on_click(move |_, window, app| {
                                if weak_discard.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let worker_id_discard = worker_id_discard.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.discard_isolated_work(&worker_id_discard, window, cx);
                                    });
                                }
                            }),
                    ),
            );
        }
        if let Some(preview) = &adoption {
            let mut adoption_view = v_flex()
                .gap_1()
                .px_2()
                .py_2()
                .rounded_md()
                .bg(cx.theme().colors.button)
                .child(div().text_xs().font_weight(FontWeight(600.)).child(format!(
                    "{} change(s){}",
                    preview.changes.len(),
                    if preview.applies_cleanly {
                        " · applies cleanly"
                    } else {
                        " · does NOT apply cleanly"
                    }
                )));
            for change in &preview.changes {
                adoption_view = adoption_view.child(
                    div()
                        .text_xs()
                        .child(format!("{} {}", change.status, change.path)),
                );
            }
            if !preview.conflicts.is_empty() {
                adoption_view = adoption_view.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.danger)
                        .child(format!(
                            "Conflicts with your workspace edits: {}",
                            preview.conflicts.join(", ")
                        )),
                );
            }
            for entry in &preview.verification {
                adoption_view = adoption_view.child(div().text_xs().child(entry.clone()));
            }
            card = card.child(adoption_view);
        }
        for task in tasks {
            card = card.child(self.task_row(task, cx));
        }
        card
    }

    fn task_row(&self, task: &WorkerTask, cx: &Context<Self>) -> impl IntoElement {
        let task_id = task.task_id.clone();
        let scope = task.scope.clone();
        let status = task.status.clone();
        let result = task.result.clone();
        let changed_files = task.changed_files.clone();
        let verification = task.verification.len();
        let claims = task.claims.clone();
        let editing = self.editing_task.as_deref() == Some(task_id.as_str());
        let mut row = v_flex()
            .gap_1()
            .px_2()
            .py_1_5()
            .rounded_md()
            .bg(cx.theme().colors.button)
            .child(if editing {
                Input::new(&self.editing_scope).small().into_any_element()
            } else {
                div()
                    .text_sm()
                    .font_weight(FontWeight(600.))
                    .child(scope.clone())
                    .into_any_element()
            })
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Tag::new()
                            .with_variant(super::status_tag_variant(&status))
                            .small()
                            .rounded_full()
                            .child(status.clone()),
                    )
                    .when_some(result.clone(), |this, result| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().colors.muted_foreground)
                                .child(result),
                        )
                    }),
            );
        if !changed_files.is_empty() {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(format!("Files: {}", changed_files.join(", "))),
            );
        }
        if verification > 0 {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(format!("{verification} evidence entries")),
            );
        }
        if !claims.is_empty() {
            let claim_text = claims
                .iter()
                .map(super::describe_claim)
                .collect::<Vec<_>>()
                .join(", ");
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(format!("Claims: {claim_text}")),
            );
        }
        if status == "queued" {
            let task_id_save = task_id.clone();
            let task_id_edit = task_id.clone();
            let task_id_cancel = task_id.clone();
            let scope_edit = scope.clone();
            let weak_save = cx.entity().downgrade();
            let weak_edit = cx.entity().downgrade();
            let weak_cancel = cx.entity().downgrade();
            row = row.child(
                h_flex()
                    .gap_1()
                    .child(if editing {
                        Button::new(SharedString::from(format!("save-task-{task_id}")))
                            .ghost()
                            .xsmall()
                            .label("Save")
                            .on_click(move |_, window, app| {
                                if weak_save.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let task_id_save = task_id_save.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.save_task_edit(&task_id_save, window, cx);
                                    });
                                }
                            })
                            .into_any_element()
                    } else {
                        Button::new(SharedString::from(format!("edit-task-{task_id}")))
                            .ghost()
                            .xsmall()
                            .label("Edit")
                            .on_click(move |_, window, app| {
                                if weak_edit.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let task_id_edit = task_id_edit.clone();
                                    let scope_edit = scope_edit.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.start_edit_task(&task_id_edit, &scope_edit, window, cx);
                                    });
                                }
                            })
                            .into_any_element()
                    })
                    .child(
                        Button::new(SharedString::from(format!("cancel-task-{task_id}")))
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(move |_, window, app| {
                                if weak_cancel.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let task_id_cancel = task_id_cancel.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.cancel_task(&task_id_cancel, window, cx);
                                    });
                                }
                            }),
                    ),
            );
        }
        row
    }

    /// Dispatch queue: target worker select, scope textarea, claims input.
    fn dispatch_form(&self, run: &TeamRunDetail, cx: &Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let dispatchable: Vec<TeamWorker> = run
            .workers
            .iter()
            .filter(|worker| worker.lifecycle != WorkerLifecycle::Archived)
            .cloned()
            .collect();
        let scope_empty = self.team_scope.read(cx).value().trim().is_empty();
        v_flex()
            .gap_1_5()
            .child(Select::new(&self.dispatch_worker).small())
            .child(Input::new(&self.team_scope).small())
            .child(Input::new(&self.team_claims).small())
            .child(
                Button::new("queue-for-worker")
                    .secondary()
                    .small()
                    .label("Queue for worker")
                    .disabled(scope_empty || dispatchable.is_empty())
                    .on_click(move |_, window, app| {
                        if weak.upgrade().is_some() {
                            let handle = window.window_handle();
                            with_window(app, handle, |hub, window, cx| {
                                hub.dispatch_task(window, cx);
                            });
                        }
                    }),
            )
    }

    /// Add-worker controls: engine select, isolated checkbox, add/end run,
    /// autonomy and archived visibility.
    fn add_worker_form(&self, run: &TeamRunDetail, cx: &Context<Self>) -> impl IntoElement {
        let run_active = run.status == "active";
        let weak = cx.entity().downgrade();
        let weak2 = cx.entity().downgrade();
        let weak3 = cx.entity().downgrade();
        let weak4 = cx.entity().downgrade();
        let weak_archived = cx.entity().downgrade();
        v_flex()
            .gap_1_5()
            .child(Select::new(&self.team_worker_engine).small())
            .child(
                Checkbox::new("isolate-worker")
                    .label("Isolated worktree")
                    .checked(self.isolate_worker)
                    .on_click(move |checked, _, app| {
                        if let Some(entity) = weak.upgrade() {
                            entity.update(app, |hub, cx| {
                                hub.isolate_worker = *checked;
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("add-worker")
                            .secondary()
                            .small()
                            .label("Add worker")
                            .disabled(!run_active)
                            .on_click(move |_, window, app| {
                                if weak2.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    with_window(app, handle, |hub, window, cx| {
                                        hub.add_worker(window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("end-run")
                            .ghost()
                            .small()
                            .label("End run")
                            .disabled(!run_active)
                            .on_click(move |_, window, app| {
                                if weak3.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    with_window(app, handle, |hub, window, cx| {
                                        hub.end_team_run(window, cx);
                                    });
                                }
                            }),
                    ),
            )
            .child(
                Checkbox::new("autonomy")
                    .label("Autonomous run (no per-request approval)")
                    .checked(run.autonomous)
                    .on_click(move |checked, window, app| {
                        if weak4.upgrade().is_some() {
                            let handle = window.window_handle();
                            let checked = *checked;
                            with_window(app, handle, move |hub, window, cx| {
                                hub.toggle_autonomy(checked, window, cx);
                            });
                        }
                    }),
            )
            .child(
                Checkbox::new("show-archived")
                    .label("Show archived")
                    .checked(self.show_archived)
                    .on_click({
                        let weak_archived = weak_archived.clone();
                        move |checked, _, app| {
                            if let Some(entity) = weak_archived.upgrade() {
                                entity.update(app, |hub, cx| {
                                    hub.show_archived = *checked;
                                    cx.notify();
                                });
                            }
                        }
                    }),
            )
    }
}
