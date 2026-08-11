// INPUT:  HubApp state entity + gpui-component DockArea/Panel contracts
// OUTPUT: Native three-region Hub workspace with navigation, chat, and tool panels
// POS:    Root GPUI shell; panels project shared Hub state without owning domain logic

use std::sync::Arc;

use gpui::*;
use gpui_component::{
    dock::{DockArea, DockItem, Panel, PanelControl, PanelEvent},
    IconName,
};

use super::app::{HubApp, PrimarySurface};
use super::components::HubToolPlaceholder;

const DOCK_LAYOUT_VERSION: usize = 3;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum WorkspaceTool {
    Files,
    Review,
    Agent,
    Inspector,
    Background,
    Resources,
    Browser,
    Terminal,
}

impl WorkspaceTool {
    pub const fn panel_name(self) -> &'static str {
        match self {
            Self::Files => "hub-files",
            Self::Review => "hub-review",
            Self::Agent => "hub-agent",
            Self::Inspector => "hub-inspector",
            Self::Background => "hub-background",
            Self::Resources => "hub-resources",
            Self::Browser => "hub-browser",
            Self::Terminal => "hub-terminal",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Review => "Review",
            Self::Agent => "Agent",
            Self::Inspector => "Inspector",
            Self::Background => "Background",
            Self::Resources => "Resources",
            Self::Browser => "Browser",
            Self::Terminal => "Terminal",
        }
    }

    pub(crate) const fn icon(self) -> IconName {
        match self {
            Self::Files => IconName::Inbox,
            Self::Review => IconName::Github,
            Self::Agent => IconName::Bot,
            Self::Inspector => IconName::Search,
            Self::Background => IconName::Info,
            Self::Resources => IconName::PanelRightOpen,
            Self::Browser => IconName::Search,
            Self::Terminal => IconName::SquareTerminal,
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Files => "Browse project files and preview them beside the conversation.",
            Self::Review => "Review repository changes produced by the current Hub session.",
            Self::Agent => "Navigate child agents associated with this Hub session.",
            Self::Inspector => "Inspect durable and live engine execution records.",
            Self::Background => "Track background jobs that belong to this Hub session.",
            Self::Resources => "Inspect engine CPU, memory, and process health.",
            Self::Browser => "Open project research without leaving the session.",
            Self::Terminal => "Run project commands without leaving the session.",
        }
    }
}

pub const fn default_workspace_tools() -> [WorkspaceTool; 8] {
    [
        WorkspaceTool::Files,
        WorkspaceTool::Review,
        WorkspaceTool::Agent,
        WorkspaceTool::Inspector,
        WorkspaceTool::Background,
        WorkspaceTool::Resources,
        WorkspaceTool::Browser,
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
            .map(|tool| (tool, cx.new(|cx| ToolPanel::new(tool, hub.clone(), cx))))
            .collect::<Vec<_>>();
        let dock_area =
            cx.new(|cx| DockArea::new("alva-hub-workspace", Some(DOCK_LAYOUT_VERSION), window, cx));
        let weak_dock = dock_area.downgrade();

        let center = DockItem::panel(Arc::new(chat));
        let left = DockItem::panel(Arc::new(navigation));
        let right = DockItem::tabs(Vec::new(), &weak_dock, window, cx);

        dock_area.update(cx, |dock, cx| {
            dock.set_version(DOCK_LAYOUT_VERSION, window, cx);
            dock.set_center(center, window, cx);
            dock.set_left_dock(left, Some(px(224.)), true, window, cx);
            dock.set_right_dock(right, Some(px(520.)), false, window, cx);
            dock.set_dock_collapsible(
                gpui::Edges {
                    right: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
        hub.update(cx, |hub, _| {
            hub.workspace_dock = Some(weak_dock);
            hub.workspace_tool_panels = tools
                .iter()
                .map(|(tool, panel)| (*tool, panel.downgrade()))
                .collect();
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
        self.hub.update(cx, |hub, cx| match hub.primary_surface {
            PrimarySurface::Chat => hub.main_column(window, cx).into_any_element(),
            PrimarySurface::Automation => hub.automation_surface(cx).into_any_element(),
            PrimarySurface::Plugins => hub.plugins_surface(cx).into_any_element(),
            PrimarySurface::Settings => hub.settings_surface(window, cx).into_any_element(),
        })
    }
}

pub(crate) struct ToolPanel {
    tool: WorkspaceTool,
    hub: Entity<HubApp>,
    focus_handle: FocusHandle,
}

impl ToolPanel {
    pub(crate) fn new(tool: WorkspaceTool, hub: Entity<HubApp>, cx: &mut Context<Self>) -> Self {
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
        true
    }

    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock_area = self.hub.update(cx, |hub, cx| {
            hub.workspace_open_tools
                .retain(|candidate| *candidate != self.tool);
            cx.notify();
            if hub.workspace_open_tools.is_empty() {
                hub.workspace_dock.clone()
            } else {
                None
            }
        });
        let Some(dock_area) = dock_area.and_then(|dock| dock.upgrade()) else {
            return;
        };
        let right_dock = dock_area.read(cx).right_dock().cloned();
        if let Some(right_dock) = right_dock {
            right_dock.update(cx, |dock, cx| dock.set_open(false, window, cx));
        }
    }
}

impl Render for ToolPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.tool {
            WorkspaceTool::Inspector => self
                .hub
                .update(cx, |hub, cx| hub.inspector_panel(cx).into_any_element()),
            WorkspaceTool::Agent => self
                .hub
                .update(cx, |hub, cx| hub.agent_panel(cx).into_any_element()),
            WorkspaceTool::Files
            | WorkspaceTool::Review
            | WorkspaceTool::Background
            | WorkspaceTool::Resources
            | WorkspaceTool::Browser
            | WorkspaceTool::Terminal => self.placeholder(cx).into_any_element(),
        }
    }
}

impl ToolPanel {
    fn placeholder(&self, _: &Context<Self>) -> impl IntoElement {
        HubToolPlaceholder::new(self.tool.icon(), self.tool.label(), self.tool.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_workspace_keeps_agent_and_inspector_distinct() {
        let tools = default_workspace_tools();
        assert_eq!(tools[2], WorkspaceTool::Agent);
        assert_eq!(tools[3], WorkspaceTool::Inspector);
        assert_ne!(tools[2].panel_name(), tools[3].panel_name());
    }

    #[test]
    fn default_tool_order_matches_product_navigation() {
        let labels = default_workspace_tools().map(WorkspaceTool::label);
        assert_eq!(
            labels,
            [
                "Files",
                "Review",
                "Agent",
                "Inspector",
                "Background",
                "Resources",
                "Browser",
                "Terminal",
            ]
        );
    }
}
