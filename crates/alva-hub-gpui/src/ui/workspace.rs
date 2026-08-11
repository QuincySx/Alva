// INPUT:  HubApp state entity + gpui-component DockArea/Panel contracts
// OUTPUT: Native three-region Hub workspace with navigation, chat, and tool panels
// POS:    Root GPUI shell; panels project shared Hub state without owning domain logic

use std::sync::Arc;

use gpui::*;
use gpui_component::{
    avatar::Avatar,
    dock::{DockArea, DockItem, Panel, PanelControl, PanelEvent},
    tag::Tag,
    ActiveTheme, IconName, Sizable as _, *,
};

use super::app::HubApp;

const DOCK_LAYOUT_VERSION: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTool {
    Inspector,
    Agent,
    Git,
    Terminal,
}

impl WorkspaceTool {
    pub const fn panel_name(self) -> &'static str {
        match self {
            Self::Inspector => "hub-inspector",
            Self::Agent => "hub-agent",
            Self::Git => "hub-git",
            Self::Terminal => "hub-terminal",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Inspector => "Inspector",
            Self::Agent => "Agent",
            Self::Git => "Git",
            Self::Terminal => "Terminal",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Inspector => IconName::Search,
            Self::Agent => IconName::Bot,
            Self::Git => IconName::Github,
            Self::Terminal => IconName::SquareTerminal,
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Inspector => "Inspect durable and live engine execution records.",
            Self::Agent => "Navigate child agents associated with this Hub session.",
            Self::Git => "Review repository history and workspace changes.",
            Self::Terminal => "Run project commands without leaving the session.",
        }
    }
}

pub const fn default_workspace_tools() -> [WorkspaceTool; 4] {
    [
        WorkspaceTool::Inspector,
        WorkspaceTool::Agent,
        WorkspaceTool::Git,
        WorkspaceTool::Terminal,
    ]
}

pub struct HubWorkspace {
    hub: Entity<HubApp>,
    dock_area: Entity<DockArea>,
}

impl HubWorkspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hub = cx.new(|cx| HubApp::new(window, cx));
        let navigation = cx.new(|cx| NavigationPanel::new(hub.clone(), cx));
        let chat = cx.new(|cx| ChatPanel::new(hub.clone(), cx));
        let tools = default_workspace_tools()
            .into_iter()
            .map(|tool| cx.new(|cx| ToolPanel::new(tool, hub.clone(), cx)))
            .collect::<Vec<_>>();
        let dock_area =
            cx.new(|cx| DockArea::new("alva-hub-workspace", Some(DOCK_LAYOUT_VERSION), window, cx));
        let weak_dock = dock_area.downgrade();

        let center = DockItem::panel(Arc::new(chat));
        let left = DockItem::tab(navigation, &weak_dock, window, cx);
        let right = DockItem::tabs(
            tools
                .into_iter()
                .map(|panel| Arc::new(panel) as Arc<dyn gpui_component::dock::PanelView>)
                .collect(),
            &weak_dock,
            window,
            cx,
        );

        dock_area.update(cx, |dock, cx| {
            dock.set_version(DOCK_LAYOUT_VERSION, window, cx);
            dock.set_center(center, window, cx);
            dock.set_left_dock(left, Some(px(292.)), true, window, cx);
            dock.set_right_dock(right, Some(px(380.)), true, window, cx);
            dock.set_dock_collapsible(
                gpui::Edges {
                    right: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });

        Self { hub, dock_area }
    }

    pub fn hub(&self) -> Entity<HubApp> {
        self.hub.clone()
    }
}

impl Render for HubWorkspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.dock_area.clone()
    }
}

struct NavigationPanel {
    hub: Entity<HubApp>,
    focus_handle: FocusHandle,
}

impl NavigationPanel {
    fn new(hub: Entity<HubApp>, cx: &mut Context<Self>) -> Self {
        Self {
            hub,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for NavigationPanel {}

impl Focusable for NavigationPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for NavigationPanel {
    fn panel_name(&self) -> &'static str {
        "hub-navigation"
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "Projects & sessions"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn inner_padding(&self, _: &App) -> bool {
        false
    }
}

impl Render for NavigationPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.hub
            .update(cx, |hub, cx| hub.sidebar(window, cx).into_any_element())
    }
}

struct ChatPanel {
    hub: Entity<HubApp>,
    focus_handle: FocusHandle,
}

impl ChatPanel {
    fn new(hub: Entity<HubApp>, cx: &mut Context<Self>) -> Self {
        Self {
            hub,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for ChatPanel {}

impl Focusable for ChatPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ChatPanel {
    fn panel_name(&self) -> &'static str {
        "hub-chat"
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "Chat"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn inner_padding(&self, _: &App) -> bool {
        false
    }
}

impl Render for ChatPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.hub
            .update(cx, |hub, cx| hub.main_column(window, cx).into_any_element())
    }
}

struct ToolPanel {
    tool: WorkspaceTool,
    #[allow(dead_code)]
    hub: Entity<HubApp>,
    focus_handle: FocusHandle,
}

impl ToolPanel {
    fn new(tool: WorkspaceTool, hub: Entity<HubApp>, cx: &mut Context<Self>) -> Self {
        Self {
            tool,
            hub,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for ToolPanel {}

impl Focusable for ToolPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ToolPanel {
    fn panel_name(&self) -> &'static str {
        self.tool.panel_name()
    }

    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some(self.tool.label().into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.tool.label()
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for ToolPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(Avatar::new().placeholder(self.tool.icon()).large())
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight(600.))
                    .child(self.tool.label()),
            )
            .child(
                div()
                    .max_w(px(280.))
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(self.tool.description()),
            )
            .child(Tag::secondary().small().child("Tool workspace"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_workspace_keeps_agent_and_inspector_distinct() {
        let tools = default_workspace_tools();
        assert_eq!(tools[0], WorkspaceTool::Inspector);
        assert_eq!(tools[1], WorkspaceTool::Agent);
        assert_ne!(tools[0].panel_name(), tools[1].panel_name());
    }

    #[test]
    fn default_tool_order_matches_product_navigation() {
        let labels = default_workspace_tools().map(WorkspaceTool::label);
        assert_eq!(labels, ["Inspector", "Agent", "Git", "Terminal"]);
    }
}
