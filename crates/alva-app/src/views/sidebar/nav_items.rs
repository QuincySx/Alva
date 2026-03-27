// INPUT:  gpui, gpui_component (Icon, IconName), crate::theme::Theme
// OUTPUT: pub fn render_nav_items()
// POS:    Renders sidebar navigation items (Search, Schedule, Skills, MCP) with Lucide SVG icons.
use gpui::*;

use gpui_component::{Icon, IconName};
use gpui_component::Sizable;

use crate::theme::Theme;

/// A single navigation item definition.
struct NavItemDef {
    id: &'static str,
    icon: IconName,
    label: &'static str,
}

fn nav_items() -> Vec<NavItemDef> {
    vec![
        NavItemDef { id: "nav-search", icon: IconName::Search, label: "搜索任务" },
        NavItemDef { id: "nav-schedule", icon: IconName::Calendar, label: "定时任务" },
        NavItemDef { id: "nav-skills", icon: IconName::Star, label: "技能" },
        NavItemDef { id: "nav-mcp", icon: IconName::Globe, label: "MCP" },
    ]
}

/// Render a single nav item row.
fn render_nav_item(item: NavItemDef, theme: &Theme) -> impl IntoElement {
    let surface_hover = theme.surface_hover;
    let text_muted: Hsla = theme.text_muted.into();

    div()
        .id(ElementId::Name(item.id.into()))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .py(px(6.))
        .mx(px(8.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_sm()
        .text_color(theme.text_muted)
        .hover(move |s| s.bg(surface_hover).text_color(theme.text))
        .child(
            Icon::new(item.icon)
                .small()
                .text_color(text_muted),
        )
        .child(item.label)
}

/// Render all navigation items as a vertical list.
pub fn render_nav_items(theme: &Theme) -> impl IntoElement {
    let mut container = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(4.));

    for item in nav_items() {
        container = container.child(render_nav_item(item, theme));
    }

    container
}
