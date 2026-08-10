// INPUT:  gpui + gpui-component
// OUTPUT: ALVA Hub Desktop Rust-native shell (P0: window + theme + sidebar skeleton)
// POS:    GPUI migration path — same L7 product seam as alva-hub-desktop, backend
//         (store/run) reused later; this crate owns only the native UI.

use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    sidebar::{
        Sidebar, SidebarFooter, SidebarHeader, SidebarMenu, SidebarMenuItem,
    },
    *,
};

/// P0 state: placeholders that P1 replaces with HubStore-backed data.
pub struct HubApp;

impl HubApp {
    fn new() -> Self {
        Self
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
                .children([
                    SidebarMenuItem::new("(no sessions yet)"),
                ]),
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
}

impl Render for HubApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .flex_row()
            .child(
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
                    ),
            )
            // Main column placeholder — P1 replaces with timeline/composer.
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(Icon::new(IconName::ArrowRight).size_5())
                    .child(div().text_lg().child("Start one continuous project session"))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("Hub keeps each engine branch in private local storage."),
                    ),
            )
    }
}

fn main() {
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| HubApp::new());
                cx.new(|cx| {
                    Root::new(view, window, cx).bg(cx.theme().background)
                })
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
