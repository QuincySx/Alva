// INPUT:  HubApp session state, EngineInspector snapshots/live stream, gpui-component controls
// OUTPUT: Experiment-gated historical/live recursive Inspector and distinct Agent child view
// POS:    Right Tool Workspace implementation; reads engine-native records without owning them

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use alva_engine_adapter_alva::{RepositoryInspector, SessionRepository};
use alva_engine_runtime::{
    encode_jsonl_bundle, ChildSessionLink, EngineInspector, InspectionSnapshot, InspectionUpdate,
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    accordion::Accordion,
    button::{Button, ButtonVariants as _},
    description_list::DescriptionList,
    group_box::{GroupBox, GroupBoxVariants as _},
    scroll::ScrollableElement,
    spinner::Spinner,
    tag::Tag,
    ActiveTheme, IconName, Sizable as _, *,
};
use tokio_stream::StreamExt as _;

use super::app::{with_window, HubApp};

impl HubApp {
    pub fn toggle_inspector(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.inspector_enabled = enabled;
        if let Err(error) = self.store.set_preference_bool("inspector_enabled", enabled) {
            self.inspector_error = Some(error);
        }
        self.inspector_generation = self.inspector_generation.wrapping_add(1);
        if enabled {
            self.refresh_inspector(cx);
        } else {
            self.inspector_snapshot = None;
            self.inspector_error = None;
            self.inspector_session_stack.clear();
            self.inspector_loading = false;
        }
        cx.notify();
    }

    pub fn refresh_inspector(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = self.session_detail.as_ref() else {
            self.inspector_error = Some("Open a Hub session to inspect it.".to_string());
            self.inspector_snapshot = None;
            cx.notify();
            return;
        };
        if detail.session.engine_id != "alva" {
            self.inspector_error = Some(format!(
                "{} exposes no native Inspector records. Hub only shows truthful engine detail.",
                detail.session.engine_id
            ));
            self.inspector_snapshot = None;
            cx.notify();
            return;
        }
        let Some(native_session_id) = detail.session.engine_session_id.clone() else {
            self.inspector_error =
                Some("This Alva attempt has not started a native session yet.".to_string());
            self.inspector_snapshot = None;
            cx.notify();
            return;
        };
        self.inspector_session_stack = vec![native_session_id.clone()];
        self.load_inspector_session(native_session_id, cx);
    }

    pub fn open_inspector_child(&mut self, native_session_id: String, cx: &mut Context<Self>) {
        self.inspector_session_stack.push(native_session_id.clone());
        self.load_inspector_session(native_session_id, cx);
    }

    pub fn inspector_back(&mut self, cx: &mut Context<Self>) {
        if self.inspector_session_stack.len() <= 1 {
            return;
        }
        self.inspector_session_stack.pop();
        if let Some(native_session_id) = self.inspector_session_stack.last().cloned() {
            self.load_inspector_session(native_session_id, cx);
        }
    }

    fn inspector_handle(&self) -> Result<(Arc<dyn EngineInspector>, bool), String> {
        let product_session_id = self
            .active_session_id
            .as_ref()
            .ok_or_else(|| "No Hub session is open.".to_string())?;
        if let Some(inspector) = self
            .active_inspectors
            .lock()
            .map_err(|_| "Inspector registry lock poisoned".to_string())?
            .get(product_session_id)
            .cloned()
        {
            return Ok((inspector, true));
        }
        let root = self
            .store
            .path()
            .parent()
            .ok_or_else(|| "Hub data directory is unavailable.".to_string())?;
        let repository = SessionRepository::open(root.join("alva-agent-records.db"))?;
        Ok((Arc::new(RepositoryInspector::new(repository)), false))
    }

    fn load_inspector_session(&mut self, native_session_id: String, cx: &mut Context<Self>) {
        let (inspector, live) = match self.inspector_handle() {
            Ok(value) => value,
            Err(error) => {
                self.inspector_error = Some(error);
                self.inspector_loading = false;
                cx.notify();
                return;
            }
        };
        self.inspector_generation = self.inspector_generation.wrapping_add(1);
        let generation = self.inspector_generation;
        self.inspector_loading = true;
        self.inspector_error = None;
        self.inspector_open_records.clear();
        cx.notify();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let snapshot = inspector.snapshot(&native_session_id).await;
            let high_water = snapshot
                .as_ref()
                .map(|snapshot| snapshot.high_water_seq)
                .unwrap_or(0);
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    if hub.inspector_generation != generation {
                        return;
                    }
                    hub.inspector_loading = false;
                    match snapshot {
                        Ok(snapshot) => hub.inspector_snapshot = Some(snapshot),
                        Err(error) => hub.inspector_error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            }
            if !live {
                return;
            }
            let Ok(mut stream) = inspector.subscribe(&native_session_id, high_water).await else {
                return;
            };
            while let Some(update) = stream.next().await {
                let Some(hub) = weak.upgrade() else {
                    break;
                };
                let keep_running = hub.update(cx, |hub, cx| {
                    if hub.inspector_generation != generation {
                        return false;
                    }
                    if let Some(snapshot) = &mut hub.inspector_snapshot {
                        match update {
                            InspectionUpdate::Record { record } => {
                                snapshot.high_water_seq = snapshot.high_water_seq.max(record.seq);
                                if let alva_kernel_abi::SessionRecordPayload::Execution(
                                    alva_kernel_abi::ExecutionRecord::ChildSessionLinked {
                                        child_session_id,
                                        parent_run_id,
                                        spawn_tool_call_id,
                                    },
                                ) = &record.payload
                                {
                                    if !snapshot
                                        .children
                                        .iter()
                                        .any(|child| child.child_session_id == *child_session_id)
                                    {
                                        snapshot.children.push(ChildSessionLink {
                                            child_session_id: child_session_id.clone(),
                                            parent_session_id: snapshot.native_session_id.clone(),
                                            parent_run_id: parent_run_id.clone(),
                                            spawn_tool_call_id: spawn_tool_call_id.clone(),
                                        });
                                    }
                                }
                                snapshot.records.push(*record);
                            }
                            InspectionUpdate::ChildLinked { link } => snapshot.children.push(link),
                        }
                    }
                    cx.notify();
                    true
                });
                if !keep_running {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn export_inspector(&mut self, cx: &mut Context<Self>) {
        let Some(root_snapshot) = self.inspector_snapshot.clone() else {
            return;
        };
        let (inspector, _) = match self.inspector_handle() {
            Ok(value) => value,
            Err(error) => {
                self.inspector_error = Some(error);
                cx.notify();
                return;
            }
        };
        let product_session_id = self.active_session_id.clone().unwrap_or_default();
        let export_root = self
            .store
            .path()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("exports");
        let handle = self.window_handle;
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let mut snapshots = Vec::new();
            let mut queue = VecDeque::from([root_snapshot]);
            let mut seen = HashSet::new();
            while let Some(snapshot) = queue.pop_front() {
                if !seen.insert(snapshot.native_session_id.clone()) {
                    continue;
                }
                for child in &snapshot.children {
                    if let Ok(snapshot) = inspector.snapshot(&child.child_session_id).await {
                        queue.push_back(snapshot);
                    }
                }
                snapshots.push(snapshot);
            }
            let result = encode_jsonl_bundle(&snapshots[0].native_session_id, &snapshots)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    std::fs::create_dir_all(&export_root).map_err(|error| error.to_string())?;
                    let path = export_root.join(format!("{product_session_id}-inspector.jsonl"));
                    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
                    Ok(path)
                });
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match result {
                    Ok(path) => {
                        let message = format!("Inspector bundle exported to {}", path.display());
                        with_window(cx, handle, move |_, window, cx| {
                            window.push_notification(message, cx);
                        });
                    }
                    Err(error) => {
                        hub.inspector_error = Some(format!("Export failed: {error}"));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub fn inspector_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.inspector_enabled {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .px_4()
                .child(Icon::new(IconName::Search).size_8())
                .child(div().font_weight(FontWeight(600.)).child("Inspector is hidden"))
                .child(
                    div()
                        .text_center()
                        .text_sm()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child("Enable the global Inspector experiment to view Alva execution records."),
                )
                .into_any_element();
        }
        let weak_refresh = cx.entity().downgrade();
        let weak_back = cx.entity().downgrade();
        let weak_export = cx.entity().downgrade();
        let header = h_flex()
            .gap_2()
            .items_center()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                Button::new("inspector-back")
                    .ghost()
                    .icon(IconName::ArrowLeft)
                    .disabled(self.inspector_session_stack.len() <= 1)
                    .on_click(move |_, _, app| {
                        if let Some(hub) = weak_back.upgrade() {
                            hub.update(app, |hub, cx| hub.inspector_back(cx));
                        }
                    }),
            )
            .child(
                div()
                    .font_weight(FontWeight(600.))
                    .child("Execution records"),
            )
            .child(div().flex_1())
            .child(
                Button::new("inspector-export")
                    .ghost()
                    .icon(IconName::ExternalLink)
                    .tooltip("Export recursive redacted JSONL bundle")
                    .disabled(self.inspector_snapshot.is_none())
                    .on_click(move |_, _, app| {
                        if let Some(hub) = weak_export.upgrade() {
                            hub.update(app, |hub, cx| hub.export_inspector(cx));
                        }
                    }),
            )
            .child(
                Button::new("inspector-refresh")
                    .ghost()
                    .icon(IconName::Undo2)
                    .loading(self.inspector_loading)
                    .on_click(move |_, _, app| {
                        if let Some(hub) = weak_refresh.upgrade() {
                            hub.update(app, |hub, cx| hub.refresh_inspector(cx));
                        }
                    }),
            );
        let body = if self.inspector_loading && self.inspector_snapshot.is_none() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new())
                .child("Loading records…")
                .into_any_element()
        } else if let Some(error) = &self.inspector_error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .px_4()
                .text_center()
                .text_sm()
                .text_color(cx.theme().colors.muted_foreground)
                .child(error.clone())
                .into_any_element()
        } else if let Some(snapshot) = &self.inspector_snapshot {
            self.inspector_snapshot_view(snapshot, cx)
                .into_any_element()
        } else {
            div().flex_1().into_any_element()
        };
        v_flex()
            .size_full()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn inspector_snapshot_view(
        &self,
        snapshot: &InspectionSnapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let record_sequences = snapshot
            .records
            .iter()
            .map(|record| record.seq)
            .collect::<Vec<_>>();
        let mut records = Accordion::new("inspector-records").multiple(true).small();
        for record in &snapshot.records {
            let open = self.inspector_open_records.contains(&record.seq);
            let raw = serde_json::to_string_pretty(record)
                .unwrap_or_else(|error| format!("serialize record: {error}"));
            records = records.item(|item| {
                item.open(open)
                    .title(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(Tag::secondary().xsmall().child(format!("#{}", record.seq)))
                            .child(Tag::new().xsmall().child(format!("{:?}", record.category)))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .font_weight(FontWeight(600.))
                                    .child(record.type_name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(raw),
                    )
            });
        }
        let weak_records = cx.entity().downgrade();
        records = records.on_toggle_click(move |open_items, _, app| {
            if let Some(hub) = weak_records.upgrade() {
                let open_sequences = open_items
                    .iter()
                    .filter_map(|index| record_sequences.get(*index).copied())
                    .collect::<HashSet<_>>();
                hub.update(app, |hub, cx| {
                    hub.inspector_open_records = open_sequences;
                    cx.notify();
                });
            }
        });
        let children = snapshot
            .children
            .iter()
            .map(|child| {
                let child_session_id = child.child_session_id.clone();
                let label = format!("Child {}", short_id(&child_session_id));
                let weak = cx.entity().downgrade();
                Button::new(SharedString::from(format!(
                    "child-{}",
                    child.child_session_id
                )))
                .ghost()
                .icon(IconName::Bot)
                .label(label)
                .on_click(move |_, _, app| {
                    if let Some(hub) = weak.upgrade() {
                        let child_session_id = child_session_id.clone();
                        hub.update(app, |hub, cx| {
                            hub.open_inspector_child(child_session_id, cx)
                        });
                    }
                })
            })
            .collect::<Vec<_>>();
        div()
            .flex_1()
            .size_full()
            .v_flex()
            .overflow_y_scrollbar()
            .child(
                GroupBox::new().outline().child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Tag::success().small().child("Alva · Full"))
                                .child(
                                    Tag::secondary()
                                        .small()
                                        .child(format!("{} records", snapshot.records.len())),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().colors.muted_foreground)
                                .child(snapshot.native_session_id.clone()),
                        )
                        .children(children),
                ),
            )
            .child(records)
    }

    pub fn agent_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(snapshot) = &self.inspector_snapshot else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().colors.muted_foreground)
                .child("Open an Alva session to navigate its child agents.")
                .into_any_element();
        };
        let children = snapshot
            .children
            .iter()
            .map(|child| {
                GroupBox::new().outline().child(
                    DescriptionList::vertical()
                        .small()
                        .item(
                            "Agent",
                            h_flex()
                                .gap_2()
                                .child(Icon::new(IconName::Bot))
                                .child(short_id(&child.child_session_id))
                                .into_any_element(),
                            1,
                        )
                        .item("Spawn call", child.spawn_tool_call_id.clone(), 1)
                        .item("Parent run", child.parent_run_id.clone(), 1),
                )
            })
            .collect::<Vec<_>>();
        v_flex()
            .size_full()
            .child(
                div()
                    .px_3()
                    .py_3()
                    .font_weight(FontWeight(600.))
                    .child("Thread child agents"),
            )
            .when(children.is_empty(), |this| {
                this.child(
                    div()
                        .px_3()
                        .text_sm()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child("No child agents are linked to this session."),
                )
            })
            .children(children)
            .into_any_element()
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}
