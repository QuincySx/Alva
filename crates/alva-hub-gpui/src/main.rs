// INPUT:  gpui + gpui-component
// OUTPUT: ALVA Hub Desktop Rust-native shell (P1: main column + composer)
// POS:    GPUI migration path — same L7 product seam as alva-hub-desktop.
//         Follows crates/alva-hub-gpui/AGENTS.md: Entity<State> for
//         stateful components, theme colors only, no hardcoded hex.

use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    sidebar::{Sidebar, SidebarFooter, SidebarHeader, SidebarMenu, SidebarMenuItem},
    ActiveTheme, Icon, IconName, *,
};

pub struct HubApp {
    engine: Entity<SelectState<SearchableVec<&'static str>>>,
    permission: Entity<SelectState<SearchableVec<&'static str>>>,
    prompt: Entity<InputState>,
}

impl HubApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            engine: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec!["alva", "claude", "codex"]),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            permission: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec!["interactive", "workspace_write", "unrestricted"]),
                    Some(IndexPath::new(1)),
                    window,
                    cx,
                )
            }),
            prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Ask alva to inspect, change, or explain this project…")
            }),
        }
    }

    fn sidebar_menu() -> SidebarMenu {
        SidebarMenu::new().children([
            SidebarMenuItem::new("Workspace")
                .icon(IconName::Folder)
                .click_to_toggle(true)
                .children([SidebarMenuItem::new("Choose a local project to begin")]),
            SidebarMenuItem::new("Sessions")
                .icon(IconName::SquareTerminal)
                .click_to_toggle(true)
                .default_open(true)
                .children([SidebarMenuItem::new("(no sessions yet)")]),
            SidebarMenuItem::new("Engines")
                .icon(IconName::Cpu)
                .click_to_toggle(true)
                .children([
                    SidebarMenuItem::new("Alva · healthy"),
                    SidebarMenuItem::new("Claude · healthy"),
                    SidebarMenuItem::new("Codex · sign-in needed"),
                ]),
            SidebarMenuItem::new("Fallback chain")
                .icon(IconName::Github)
                .click_to_toggle(true)
                .children([
                    SidebarMenuItem::new("1. alva"),
                    SidebarMenuItem::new("2. claude"),
                    SidebarMenuItem::new("3. codex"),
                ]),
            SidebarMenuItem::new("Team")
                .icon(IconName::User)
                .click_to_toggle(true)
                .children([SidebarMenuItem::new("(open a session to lead a team run)")]),
        ])
    }

    fn sidebar(&self) -> Sidebar<SidebarMenu> {
        Sidebar::new("hub-sidebar")
            .header(SidebarHeader::new().child("ALVA Hub"))
            .child(Self::sidebar_menu())
            .footer(
                SidebarFooter::new().child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child("One project. Every engine.")
                        .child("Durable branches · reversible handoff"),
                ),
            )
    }

    /// Top bar: engine + permission selectors, ready pill.
    fn topbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_3()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(label_small("Engine", cx))
                    .child(Select::new(&self.engine).small()),
            )
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(label_small("Permissions", cx))
                    .child(Select::new(&self.permission).small()),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().colors.button)
                    .child(div().size_2().rounded_full().bg(cx.theme().colors.success))
                    .child(div().text_sm().child("Ready")),
            )
    }

    /// Main column: empty state + composer.
    fn main_column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .size_full()
            .child(self.topbar(cx))
            // Timeline placeholder — P2 renders session cards.
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .size_10()
                            .rounded_lg()
                            .bg(cx.theme().colors.button)
                            .v_flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::ArrowRight)
                                    .text_color(cx.theme().colors.muted_foreground),
                            ),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight(600.0))
                            .child("Start one continuous project session"),
                    )
                    .child(div().text_color(cx.theme().colors.muted_foreground).child(
                        "Hub keeps each engine branch in private local storage, so you can \
                                 switch engines and return without losing its native session.",
                    )),
            )
            // Composer
            .child(
                v_flex()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(cx.theme().colors.border)
                    .child(Input::new(&self.prompt))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Button::new("run").primary().label("Run with alva"))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child("fallback: alva → claude → codex"),
                            ),
                    ),
            )
    }
}

fn label_small(text: impl Into<SharedString>, cx: &Context<HubApp>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().colors.muted_foreground)
        .child(text.into())
}

impl Render for HubApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().colors.background)
            .child(self.sidebar())
            .child(self.main_column(cx))
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|cx| HubApp::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().colors.background))
                })
                .expect("Failed to open window");
            })
            .detach();
        });
}
