// INPUT:  Template, extension, and right-workbench presentation data
// OUTPUT: Consistent cards and tool empty states built on gpui-component
// POS:    Reusable card layer for automation, plugins, skills, and dock tools

use std::rc::Rc;

use gpui::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, ClickEvent, FontWeight, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    group_box::{GroupBox, GroupBoxVariants as _},
    tag::Tag,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _,
};

type QuickStartHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct HubQuickStartCard {
    id: SharedString,
    icon: IconName,
    title: SharedString,
    description: SharedString,
    on_click: Option<QuickStartHandler>,
}

impl HubQuickStartCard {
    pub fn new(
        id: impl Into<SharedString>,
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            title: title.into(),
            description: description.into(),
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for HubQuickStartCard {
    fn render(self, _: &mut Window, _cx: &mut App) -> impl IntoElement {
        let content = v_flex()
            .size_full()
            .items_start()
            .justify_between()
            .gap_2()
            .child(Icon::new(self.icon).small())
            .child(div().font_weight(FontWeight(650.)).child(self.title));
        Button::new(self.id)
            .secondary()
            .outline()
            .flex_1()
            .min_w(px(160.))
            .h(px(88.))
            .p_3()
            .tooltip(self.description)
            .child(content)
            .when_some(self.on_click, |button, handler| {
                button.on_click(move |event, window, app| handler(event, window, app))
            })
    }
}

#[derive(IntoElement)]
pub struct HubTemplateCard {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    metadata: SharedString,
    action: Option<AnyElement>,
}

impl HubTemplateCard {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        metadata: impl Into<SharedString>,
    ) -> Self {
        Self {
            icon,
            title: title.into(),
            description: description.into(),
            metadata: metadata.into(),
            action: None,
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for HubTemplateCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        GroupBox::new().outline().flex_1().min_w(px(240.)).child(
            v_flex()
                .gap_3()
                .child(
                    gpui_component::h_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(self.icon).small())
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .font_weight(FontWeight(650.))
                                .child(self.title),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.description),
                )
                .child(
                    gpui_component::h_flex()
                        .items_center()
                        .flex_wrap()
                        .justify_between()
                        .gap_2()
                        .child(Tag::secondary().small().child(self.metadata))
                        .children(self.action),
                ),
        )
    }
}

#[derive(IntoElement)]
pub struct HubExtensionCard {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    status: SharedString,
    action: Option<AnyElement>,
}

#[derive(IntoElement)]
pub struct HubExtensionDetail {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    status: SharedString,
    capabilities: Vec<SharedString>,
}

impl HubExtensionDetail {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        status: impl Into<SharedString>,
    ) -> Self {
        Self {
            icon,
            title: title.into(),
            description: description.into(),
            status: status.into(),
            capabilities: Vec::new(),
        }
    }

    pub fn capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }
}

impl RenderOnce for HubExtensionDetail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                gpui_component::h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size_10()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(self.icon)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight(700.))
                                    .child(self.title),
                            )
                            .child(Tag::secondary().small().child(self.status)),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.description),
            )
            .child(
                GroupBox::new().outline().child(
                    v_flex()
                        .gap_2()
                        .child(div().font_weight(FontWeight(650.)).child("Capabilities"))
                        .children(
                            self.capabilities
                                .into_iter()
                                .map(|capability| Tag::secondary().small().child(capability)),
                        ),
                ),
            )
    }
}

impl HubExtensionCard {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        status: impl Into<SharedString>,
    ) -> Self {
        Self {
            icon,
            title: title.into(),
            description: description.into(),
            status: status.into(),
            action: None,
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for HubExtensionCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        GroupBox::new().outline().flex_1().min_w(px(240.)).child(
            v_flex()
                .gap_3()
                .child(
                    gpui_component::h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .size_9()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(self.icon).small()),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .font_weight(FontWeight(650.))
                                        .child(self.title),
                                )
                                .child(Tag::secondary().small().child(self.status)),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.description),
                )
                .children(self.action),
        )
    }
}

#[derive(IntoElement)]
pub struct HubToolPlaceholder {
    icon: IconName,
    title: SharedString,
    description: SharedString,
}

impl HubToolPlaceholder {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            icon,
            title: title.into(),
            description: description.into(),
        }
    }
}

impl RenderOnce for HubToolPlaceholder {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        super::HubEmptyState::new(self.icon, self.title, self.description)
            .action(Tag::secondary().small().child("Tool workspace"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_builders_keep_supplied_content() {
        let quick = HubQuickStartCard::new(
            "explore",
            IconName::Search,
            "Explore",
            "Understand this project",
        )
        .on_click(|_, _, _| {});
        let template = HubTemplateCard::new(IconName::Info, "Nightly", "Run tests", "02:00")
            .action(div().child("Use"));
        let extension =
            HubExtensionCard::new(IconName::Bot, "Inspector", "Execution records", "Built in")
                .action(div().child("Open"));

        assert!(quick.on_click.is_some());
        assert_eq!(template.title, "Nightly");
        assert!(template.action.is_some());
        assert_eq!(extension.status, "Built in");
        assert!(extension.action.is_some());
    }
}
