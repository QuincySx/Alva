// INPUT:  HubApp.fallback_policy + per-target model/provider inputs
// OUTPUT: sidebar "Fallback chain" panel — engine catalog toggles, editable
//         model/provider per target, authorize checkbox, ordering and save.
// POS:    1:1 GPUI port of the React fallback panel in App.tsx.

use gpui::*;
use gpui_component::{
    accordion::Accordion,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    input::{Input, InputState},
    tag::Tag,
    ActiveTheme, *,
};

use alva_hub_gpui::EngineTarget;

use super::app::{with_window, HubApp};

const ENGINE_IDS: [&str; 3] = ["alva", "claude", "codex"];

impl HubApp {
    /// Keep the per-target model/provider input states aligned with the
    /// policy targets (length + value). Call after every policy mutation or
    /// load so the inputs never drift from what will actually be saved.
    pub fn sync_policy_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.fallback_policy.targets.clone();
        while self.policy_model_inputs.len() < targets.len() {
            self.policy_model_inputs
                .push(cx.new(|cx| InputState::new(window, cx).placeholder("engine-default")));
            self.policy_provider_inputs
                .push(cx.new(|cx| InputState::new(window, cx).placeholder("provider id")));
        }
        while self.policy_model_inputs.len() > targets.len() {
            self.policy_model_inputs.pop();
            self.policy_provider_inputs.pop();
        }
        for (index, target) in targets.iter().enumerate() {
            let model = target.model_id.clone();
            let provider = target.provider_id.clone();
            if let Some(input) = self.policy_model_inputs.get(index) {
                input.update(cx, |state, cx| {
                    state.set_value(model, window, cx);
                });
            }
            if let Some(input) = self.policy_provider_inputs.get(index) {
                input.update(cx, |state, cx| {
                    state.set_value(provider, window, cx);
                });
            }
        }
    }

    pub fn fallback_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let collapsed = self.collapsed.get("fallback").copied().unwrap_or(true);
        let is_session = self.active_session_id.is_some();
        let count = self.fallback_policy.targets.len();
        let saving = self.policy_saving;
        let weak = cx.entity().downgrade();
        let weak2 = cx.entity().downgrade();
        let content = v_flex()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(
                        "Only checked targets may run automatically. Manual engine switching is always explicit.",
                    ),
            )
            .child(self.policy_catalog(cx))
            .child(self.policy_targets(cx))
            .child(
                Button::new("save-fallback")
                    .secondary()
                    .small()
                    .label(if is_session {
                        "Save session chain"
                    } else {
                        "Save default chain"
                    })
                    .loading(saving)
                    .disabled(saving)
                    .on_click(move |_, window, app| {
                        if weak2.upgrade().is_some() {
                            let handle = window.window_handle();
                            with_window(app, handle, |hub, window, cx| {
                                hub.save_fallback_policy(window, cx);
                            });
                        }
                    }),
            );
        Accordion::new("fallback-chain")
            .small()
            .item(|item| {
                item.open(!collapsed)
                    .title(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(if is_session {
                                "Session fallback chain"
                            } else {
                                "Default fallback chain"
                            })
                            .child(Tag::secondary().xsmall().child(count.to_string())),
                    )
                    .child(content)
            })
            .on_toggle_click(move |open_items, _, app| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(app, |hub, cx| {
                        hub.collapsed
                            .insert("fallback".to_string(), open_items.is_empty());
                        cx.notify();
                    });
                }
            })
    }

    /// Catalog checkboxes: toggle a target in/out of the chain.
    fn policy_catalog(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected: Vec<String> = self
            .fallback_policy
            .targets
            .iter()
            .map(|target| target.engine_id.clone())
            .collect();
        let weak = cx.entity().downgrade();
        h_flex()
            .gap_1()
            .children(ENGINE_IDS.iter().map(|engine_id| {
                let is_selected = selected.iter().any(|id| id == engine_id);
                let engine_id = *engine_id;
                let weak = weak.clone();
                Button::new(SharedString::from(format!("catalog-{engine_id}")))
                    .small()
                    .label(if is_selected {
                        format!("✓ {engine_id}")
                    } else {
                        format!("+ {engine_id}")
                    })
                    .selected(is_selected)
                    .on_click(move |_, window, app| {
                        if weak.upgrade().is_some() {
                            let handle = window.window_handle();
                            with_window(app, handle, move |hub, window, cx| {
                                hub.toggle_fallback_target(engine_id, window, cx);
                            });
                        }
                    })
            }))
    }

    /// Ordered target rows: index, engine, model/provider inputs, account
    /// info, authorize checkbox and up/down ordering.
    fn policy_targets(&self, cx: &Context<Self>) -> impl IntoElement {
        let targets = self.fallback_policy.targets.clone();
        let weak = cx.entity().downgrade();
        v_flex()
            .gap_1_5()
            .children(targets.iter().enumerate().map(|(index, target)| {
                let weak = weak.clone();
                let engine_id = target.engine_id.clone();
                let account = target.account_source.clone();
                let billing = target.billing_route.clone();
                let authorized = target.automatic_use_authorized;
                let is_first = index == 0;
                let is_last = index == targets.len() - 1;
                let model_input = self.policy_model_inputs.get(index).cloned();
                let provider_input = self.policy_provider_inputs.get(index).cloned();
                let weak_row = weak.clone();
                let weak_row2 = weak.clone();
                v_flex()
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
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(600.))
                                    .child(format!("{}. {engine_id}", index + 1)),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "move-up-{engine_id}"
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ChevronUp)
                                        .tooltip("Move this engine earlier")
                                        .disabled(is_first)
                                        .on_click(
                                            move |_, window, app| {
                                                if weak_row.upgrade().is_some() {
                                                    let handle = window.window_handle();
                                                    with_window(
                                                        app,
                                                        handle,
                                                        move |hub, window, cx| {
                                                            hub.move_fallback_target(
                                                                index, -1, window, cx,
                                                            );
                                                        },
                                                    );
                                                }
                                            },
                                        ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "move-down-{engine_id}"
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ChevronDown)
                                        .tooltip("Move this engine later")
                                        .disabled(is_last)
                                        .on_click(
                                            move |_, window, app| {
                                                if weak_row2.upgrade().is_some() {
                                                    let handle = window.window_handle();
                                                    with_window(
                                                        app,
                                                        handle,
                                                        move |hub, window, cx| {
                                                            hub.move_fallback_target(
                                                                index, 1, window, cx,
                                                            );
                                                        },
                                                    );
                                                }
                                            },
                                        ),
                                    ),
                            ),
                    )
                    .child(if let Some(model_input) = model_input {
                        Input::new(&model_input).small().into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .child(if let Some(provider_input) = provider_input {
                        Input::new(&provider_input).small().into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(account),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(billing),
                    )
                    .child(
                        Checkbox::new(SharedString::from(format!("authorize-{engine_id}")))
                            .label("Authorize automatic use")
                            .checked(authorized)
                            .on_click(move |checked, window, app| {
                                if weak.clone().upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let checked = *checked;
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.update_fallback_target(index, checked, window, cx);
                                    });
                                }
                            }),
                    )
            }))
    }
}

impl HubApp {
    pub fn toggle_fallback_target(
        &mut self,
        engine_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut targets = self.fallback_policy.targets.clone();
        if targets.iter().any(|target| target.engine_id == engine_id) {
            targets.retain(|target| target.engine_id != engine_id);
        } else if let Ok(target) = EngineTarget::for_engine(engine_id) {
            targets.push(target);
        }
        self.fallback_policy.targets = targets;
        self.sync_policy_inputs(window, cx);
        cx.notify();
    }

    pub fn move_fallback_target(
        &mut self,
        index: usize,
        direction: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut targets = self.fallback_policy.targets.clone();
        let target_index = index as i32 + direction;
        if target_index < 0 || target_index >= targets.len() as i32 {
            return;
        }
        targets.swap(index, target_index as usize);
        self.fallback_policy.targets = targets;
        self.sync_policy_inputs(window, cx);
        cx.notify();
    }

    pub fn update_fallback_target(
        &mut self,
        index: usize,
        authorized: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = self.fallback_policy.targets.get_mut(index) {
            target.automatic_use_authorized = authorized;
        }
        self.sync_policy_inputs(window, cx);
        cx.notify();
    }

    /// Save the session chain (or the default chain when no session is
    /// open), reading the current model/provider inputs first.
    pub fn save_fallback_policy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut targets = self.fallback_policy.targets.clone();
        for (index, target) in targets.iter_mut().enumerate() {
            if let Some(input) = self.policy_model_inputs.get(index) {
                let value = input.read(cx).value().to_string();
                if !value.trim().is_empty() {
                    target.model_id = value;
                }
            }
            if let Some(input) = self.policy_provider_inputs.get(index) {
                let value = input.read(cx).value().to_string();
                if !value.trim().is_empty() {
                    target.provider_id = value;
                }
            }
        }
        self.policy_saving = true;
        cx.notify();
        let saved = if let Some(pid) = &self.active_session_id {
            self.store.set_session_fallback_policy(pid, targets)
        } else {
            self.store.set_global_fallback_policy(targets)
        };
        self.policy_saving = false;
        match saved {
            Ok(policy) => {
                self.fallback_policy = policy;
                self.sync_policy_inputs(window, cx);
                cx.notify();
            }
            Err(error) => self.push_local_failure(&error, cx),
        }
    }
}
