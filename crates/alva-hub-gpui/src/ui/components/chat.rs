// INPUT:  Projected chat activity and stateful composer entities
// OUTPUT: Reusable activity-row and composer components
// POS:    Chat-specific component layer composed entirely from gpui-component

use gpui::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, FontWeight, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Styled, Window,
};
use gpui_component::{
    collapsible::Collapsible,
    group_box::{GroupBox, GroupBoxVariants as _},
    h_flex,
    input::{Input, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    tag::{Tag, TagVariant},
    v_flex, ActiveTheme as _, Sizable as _,
};

use crate::ui::EngineOption;

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
    engine: gpui::Entity<SelectState<SearchableVec<EngineOption>>>,
    permission_disabled: bool,
    project_action: Option<AnyElement>,
    submit_action: Option<AnyElement>,
    summary: Option<SharedString>,
    metadata: SharedString,
}

impl HubComposer {
    pub fn new(
        prompt: gpui::Entity<InputState>,
        permission: gpui::Entity<SelectState<SearchableVec<&'static str>>>,
        engine: gpui::Entity<SelectState<SearchableVec<EngineOption>>>,
        metadata: impl Into<SharedString>,
    ) -> Self {
        Self {
            prompt,
            permission,
            engine,
            permission_disabled: false,
            project_action: None,
            submit_action: None,
            summary: None,
            metadata: metadata.into(),
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

    pub fn summary(mut self, summary: impl Into<SharedString>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

impl RenderOnce for HubComposer {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .px_4()
            .pb_3()
            .child(
                GroupBox::new().outline().child(
                    v_flex().gap_2().child(Input::new(&self.prompt)).child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .children(self.project_action)
                            .child(
                                Select::new(&self.permission)
                                    .small()
                                    .w(px(210.))
                                    .disabled(self.permission_disabled),
                            )
                            .child(div().flex_1())
                            .child(Select::new(&self.engine).small().w(px(170.)))
                            .children(self.submit_action),
                    ),
                ),
            )
            .when_some(self.summary, |this, summary| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(summary),
                )
            })
            .child(
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
    }
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
