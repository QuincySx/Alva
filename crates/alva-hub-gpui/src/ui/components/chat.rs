// INPUT:  Projected chat activity and stateful composer entities
// OUTPUT: Reusable activity-row and composer components
// POS:    Chat-specific component layer composed entirely from gpui-component

use gpui::{
    div, prelude::FluentBuilder as _, px, Anchor, AnyElement, App, FontWeight, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Styled, WeakEntity, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    collapsible::Collapsible,
    group_box::{GroupBox, GroupBoxVariants as _},
    h_flex,
    input::{Input, InputState},
    list::ListItem,
    popover::Popover,
    scroll::ScrollableElement,
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    separator::Separator,
    tag::{Tag, TagVariant},
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _,
};

use alva_hub_gpui::{AgentRuntime, ModelCatalog, ModelRoute, ProviderAuth, ReasoningEffort};

use crate::ui::app::HubApp;

#[derive(IntoElement)]
pub struct HubActivityRow {
    engine: SharedString,
    status: SharedString,
    status_variant: TagVariant,
    summary: SharedString,
    open: bool,
    trigger: Option<AnyElement>,
    content: Vec<AnyElement>,
}

impl HubActivityRow {
    pub fn new(
        engine: impl Into<SharedString>,
        status: impl Into<SharedString>,
        status_variant: TagVariant,
        summary: impl Into<SharedString>,
    ) -> Self {
        Self {
            engine: engine.into(),
            status: status.into(),
            status_variant,
            summary: summary.into(),
            open: false,
            trigger: None,
            content: Vec::new(),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content.push(content.into_any_element());
        self
    }
}

impl RenderOnce for HubActivityRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        Collapsible::new()
            .open(self.open)
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight(600.))
                            .child(self.engine),
                    )
                    .child(
                        Tag::new()
                            .with_variant(self.status_variant)
                            .small()
                            .rounded_full()
                            .child(self.status),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.summary),
                    )
                    .children(self.trigger),
            )
            .content(
                GroupBox::new()
                    .outline()
                    .ml_3()
                    .child(v_flex().gap_2().children(self.content)),
            )
    }
}

#[derive(IntoElement)]
pub struct HubComposer {
    prompt: gpui::Entity<InputState>,
    permission: gpui::Entity<SelectState<SearchableVec<&'static str>>>,
    model_picker: AnyElement,
    permission_disabled: bool,
    project_action: Option<AnyElement>,
    submit_action: Option<AnyElement>,
    metadata: SharedString,
    metadata_visible: bool,
    inset: bool,
}

impl HubComposer {
    pub fn new(
        prompt: gpui::Entity<InputState>,
        permission: gpui::Entity<SelectState<SearchableVec<&'static str>>>,
        model_picker: impl IntoElement,
        metadata: impl Into<SharedString>,
    ) -> Self {
        Self {
            prompt,
            permission,
            model_picker: model_picker.into_any_element(),
            permission_disabled: false,
            project_action: None,
            submit_action: None,
            metadata: metadata.into(),
            metadata_visible: true,
            inset: true,
        }
    }

    pub fn permission_disabled(mut self, disabled: bool) -> Self {
        self.permission_disabled = disabled;
        self
    }

    pub fn project_action(mut self, action: impl IntoElement) -> Self {
        self.project_action = Some(action.into_any_element());
        self
    }

    pub fn submit_action(mut self, action: impl IntoElement) -> Self {
        self.submit_action = Some(action.into_any_element());
        self
    }

    pub fn metadata_visible(mut self, visible: bool) -> Self {
        self.metadata_visible = visible;
        self
    }

    pub fn inset(mut self, inset: bool) -> Self {
        self.inset = inset;
        self
    }
}

impl RenderOnce for HubComposer {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .when(self.inset, |this| this.px_4().pb_3())
            .child(
                GroupBox::new().outline().child(
                    v_flex()
                        .gap_2()
                        .child(Input::new(&self.prompt).appearance(false))
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .children(self.project_action)
                                .child(
                                    Select::new(&self.permission)
                                        .small()
                                        .appearance(false)
                                        .w(px(170.))
                                        .disabled(self.permission_disabled),
                                )
                                .child(div().flex_1())
                                .child(self.model_picker)
                                .children(self.submit_action),
                        ),
                ),
            )
            .when(self.metadata_visible, |this| {
                this.child(
                    div()
                        .px_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.metadata),
                )
            })
    }
}

#[derive(IntoElement)]
pub struct HubModelPicker {
    hub: WeakEntity<HubApp>,
    query: gpui::Entity<InputState>,
    catalog: ModelCatalog,
    runtime: AgentRuntime,
    selected_route: Option<ModelRoute>,
    query_value: String,
}

impl HubModelPicker {
    pub fn new(
        hub: WeakEntity<HubApp>,
        query: gpui::Entity<InputState>,
        catalog: ModelCatalog,
        runtime: AgentRuntime,
        selected_route: Option<ModelRoute>,
        query_value: impl Into<String>,
    ) -> Self {
        Self {
            hub,
            query,
            catalog,
            runtime,
            selected_route,
            query_value: query_value.into(),
        }
    }

    fn detail_panel(&self, cx: &mut App) -> AnyElement {
        let Some(route) = self.selected_route.as_ref() else {
            return v_flex()
                .w(px(300.))
                .h_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Bot).large())
                .child("Select a model to inspect its capabilities")
                .into_any_element();
        };
        let Some(model) = self.catalog.model(&route.provider_id, &route.model_id) else {
            return div().into_any_element();
        };
        let Some(provider) = self.catalog.provider(&route.provider_id) else {
            return div().into_any_element();
        };
        let auth = match provider.auth {
            ProviderAuth::Subscription => "Subscription",
            ProviderAuth::ApiKey => "API key",
            ProviderAuth::Local => "Local",
        };
        let description = format!(
            "Runs through {}.{}{}",
            provider.protocol.label(),
            if model.capabilities.images {
                " Supports image input."
            } else {
                ""
            },
            if model.capabilities.tools {
                " Supports tool calls."
            } else {
                ""
            }
        );
        let weak = self.hub.clone();
        let effort_rows = model
            .capabilities
            .reasoning_efforts
            .iter()
            .copied()
            .map(|effort| {
                let weak = weak.clone();
                let value = effort.id().to_string();
                ListItem::new(format!("model-effort-{value}"))
                    .rounded(cx.theme().radius)
                    .selected(route.reasoning_effort == Some(effort))
                    .confirmed(route.reasoning_effort == Some(effort))
                    .check_icon(IconName::Check)
                    .child(effort.label())
                    .on_click(move |_, window, app| {
                        if let Some(hub) = weak.upgrade() {
                            hub.update(app, |hub, cx| {
                                hub.select_reasoning_effort(&value, window, cx)
                            });
                        }
                    })
            })
            .collect::<Vec<_>>();

        v_flex()
            .w(px(300.))
            .h_full()
            .p_4()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight(600.))
                            .child(model.display_name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .font_weight(FontWeight(600.))
                            .text_color(cx.theme().muted_foreground)
                            .child("REASONING EFFORT"),
                    )
                    .when(effort_rows.is_empty(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("This model does not expose reasoning controls."),
                        )
                    })
                    .children(effort_rows),
            )
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .child(detail_line(
                        "Context",
                        format!("{}K tokens", model.capabilities.context_window / 1_000),
                        cx,
                    ))
                    .child(detail_line(
                        "Max output",
                        format!("{}K tokens", model.capabilities.max_output_tokens / 1_000),
                        cx,
                    ))
                    .child(detail_line("Access", auth, cx))
                    .child(detail_line("Pricing", "Provider-defined", cx)),
            )
            .child(div().flex_1())
            .child(Separator::horizontal())
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Source: {}", provider.display_name))
                    .child(format!("{} compatible", route.runtime.label())),
            )
            .into_any_element()
    }

    fn model_list(&self, cx: &mut App) -> AnyElement {
        let query = self.query_value.trim().to_lowercase();
        let mut groups = Vec::new();
        for provider in self.catalog.providers() {
            let models = self
                .catalog
                .models_for_runtime(self.runtime)
                .into_iter()
                .filter(|model| model.provider_id == provider.provider_id)
                .filter(|model| {
                    query.is_empty()
                        || format!(
                            "{} {} {}",
                            model.display_name, model.model_id, provider.display_name
                        )
                        .to_lowercase()
                        .contains(&query)
                })
                .collect::<Vec<_>>();
            if models.is_empty() {
                continue;
            }
            let mut group = v_flex().gap_1().child(
                div()
                    .px_3()
                    .pt_2()
                    .text_xs()
                    .font_weight(FontWeight(600.))
                    .text_color(cx.theme().muted_foreground)
                    .child(provider.display_name.clone()),
            );
            for model in models {
                let route_key = format!("{}::{}", model.provider_id, model.model_id);
                let selected = self.selected_route.as_ref().is_some_and(|route| {
                    route.provider_id == model.provider_id && route.model_id == model.model_id
                });
                let effort = if selected {
                    self.selected_route
                        .as_ref()
                        .and_then(|route| route.reasoning_effort)
                } else {
                    model.capabilities.default_reasoning_effort
                };
                let weak = self.hub.clone();
                let row = h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(IconName::Asterisk)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(model.display_name.clone()),
                    )
                    .when_some(effort, |this, effort| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(effort.label()),
                        )
                    })
                    .when(provider.auth == ProviderAuth::Subscription, |this| {
                        this.child(
                            Tag::secondary()
                                .small()
                                .rounded_full()
                                .child("Subscription"),
                        )
                    });
                group = group.child(
                    ListItem::new(format!("model-route-{route_key}"))
                        .mx_2()
                        .rounded(cx.theme().radius)
                        .selected(selected)
                        .confirmed(selected)
                        .check_icon(IconName::Check)
                        .child(row)
                        .on_click(move |_, window, app| {
                            if let Some(hub) = weak.upgrade() {
                                hub.update(app, |hub, cx| {
                                    hub.select_model_route(&route_key, window, cx)
                                });
                            }
                        }),
                );
            }
            groups.push(group.into_any_element());
        }

        v_flex()
            .w(px(430.))
            .h_full()
            .child(self.runtime_tabs(cx))
            .child(Separator::horizontal())
            .child(
                div().p_3().child(
                    Input::new(&self.query)
                        .prefix(Icon::new(IconName::Search).small())
                        .small(),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .when(groups.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .h_full()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_color(cx.theme().muted_foreground)
                                .child(Icon::new(IconName::Search).large())
                                .child("No compatible models found"),
                        )
                    })
                    .children(groups),
            )
            .child(Separator::horizontal())
            .child(self.add_model_button())
            .into_any_element()
    }

    fn runtime_tabs(&self, _cx: &mut App) -> AnyElement {
        h_flex()
            .w_full()
            .p_3()
            .gap_1()
            .children(AgentRuntime::ALL.into_iter().map(|runtime| {
                let weak = self.hub.clone();
                Button::new(format!("model-runtime-{}", runtime.id()))
                    .small()
                    .when(runtime == self.runtime, |button| button.primary())
                    .when(runtime != self.runtime, |button| button.ghost())
                    .label(runtime.label())
                    .flex_1()
                    .on_click(move |_, window, app| {
                        if let Some(hub) = weak.upgrade() {
                            hub.update(app, |hub, cx| hub.select_engine(runtime.id(), window, cx));
                        }
                    })
            }))
            .into_any_element()
    }

    fn add_model_button(&self) -> AnyElement {
        let weak = self.hub.clone();
        div()
            .p_2()
            .child(
                Button::new("add-model-from-picker")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add model")
                    .w_full()
                    .on_click(move |_, window, app| {
                        if let Some(hub) = weak.upgrade() {
                            hub.update(app, |hub, cx| {
                                hub.open_provider_dialog(window, cx);
                                cx.notify();
                            });
                        }
                    }),
            )
            .into_any_element()
    }
}

impl RenderOnce for HubModelPicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let model_name = self
            .selected_route
            .as_ref()
            .and_then(|route| self.catalog.model(&route.provider_id, &route.model_id))
            .map(|model| model.display_name.as_str())
            .unwrap_or("No model");
        let effort = self
            .selected_route
            .as_ref()
            .and_then(|route| route.reasoning_effort)
            .map(ReasoningEffort::label)
            .unwrap_or("Default");
        let trigger_label = format!("{} · {} · {}", self.runtime.label(), model_name, effort);
        let detail = self.detail_panel(cx);
        let list = self.model_list(cx);

        Popover::new("composer-model-picker")
            .anchor(Anchor::BottomRight)
            .p_0()
            .w(px(730.))
            .h(px(570.))
            .trigger(
                Button::new("composer-model-trigger")
                    .ghost()
                    .small()
                    .icon(IconName::Asterisk)
                    .label(trigger_label)
                    .dropdown_caret(true),
            )
            .child(
                h_flex()
                    .size_full()
                    .items_stretch()
                    .child(detail)
                    .child(Separator::vertical())
                    .child(list),
            )
    }
}

fn detail_line(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    cx: &mut App,
) -> impl IntoElement {
    let label = label.into();
    let value = value.into();
    h_flex()
        .justify_between()
        .gap_3()
        .child(div().text_color(cx.theme().muted_foreground).child(label))
        .child(div().child(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_builder_keeps_expansion_state() {
        let row = HubActivityRow::new("alva", "Running", TagVariant::Primary, "Using shell")
            .open(true)
            .content(div().child("cargo test"));

        assert!(row.open);
        assert_eq!(row.content.len(), 1);
    }
}
