// INPUT:  Page titles, empty-state copy, optional component-library actions
// OUTPUT: Consistent Hub page headers and empty states
// POS:    Shared visual primitives composed before full page surfaces

use gpui::{
    div, AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled, Window,
};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

#[derive(IntoElement)]
pub struct HubSurfaceHeader {
    title: SharedString,
    description: SharedString,
}

impl HubSurfaceHeader {
    pub fn new(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
        }
    }
}

impl RenderOnce for HubSurfaceHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_4()
            .px_6()
            .py_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight(700.))
                            .child(self.title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.description),
                    ),
            )
    }
}

#[derive(IntoElement)]
pub struct HubEmptyState {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    action: Option<AnyElement>,
}

impl HubEmptyState {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            icon,
            title: title.into(),
            description: description.into(),
            action: None,
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for HubEmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .w_full()
            .items_center()
            .justify_center()
            .gap_3()
            .py_8()
            .child(
                div()
                    .size_12()
                    .rounded_full()
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(self.icon).large()),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight(650.))
                    .child(self.title),
            )
            .child(
                div()
                    .max_w_96()
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.description),
            )
            .children(self.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_builders_keep_supplied_copy() {
        let header = HubSurfaceHeader::new("Automation", "Scheduled work");
        let empty = HubEmptyState::new(IconName::Info, "Empty", "Nothing here")
            .action(div().child("Action"));

        assert_eq!(header.title, "Automation");
        assert_eq!(header.description, "Scheduled work");
        assert_eq!(empty.title, "Empty");
        assert!(empty.action.is_some());
    }
}
