// INPUT:  gpui, gpui_component, srow_app (models, theme, views), tracing_subscriber
// OUTPUT: (binary entry point -- no public exports)
// POS:    Application entry point; initializes GPUI, gpui-component, shared models, theme, and opens the main window.
use std::sync::Arc;

use gpui::{
    actions, App, Application, Bounds, Entity, KeyBinding, Menu, MenuItem, WindowBounds,
    WindowOptions, prelude::*, px, size,
};

use srow_app::chat::SharedRuntime;
use srow_app::models::{AgentModel, ChatModel, SettingsModel, SettingsModelEvent, WorkspaceModel};
use srow_app::theme::ActiveThemeMode;
use srow_app::views::RootView;

actions!(srow, [Quit]);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    Application::new().run(|cx: &mut App| {
        // Initialize gpui-component (theme, global state, input keybindings, etc.)
        gpui_component::init(cx);
        gpui_component::set_locale("en");

        // Create shared tokio runtime (used by all GpuiChat instances)
        let runtime = Arc::new(
            tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"),
        );
        cx.set_global(SharedRuntime(runtime));

        // Create shared models
        let workspace_model = cx.new(|_| WorkspaceModel::default());
        let chat_model = cx.new(|_| ChatModel::default());
        let agent_model = cx.new(|_| AgentModel::default());
        let settings_model = cx.new(|_| SettingsModel::load());

        // Initialize the global ThemeMode from persisted settings
        let initial_theme = settings_model.read(cx).settings.theme;
        cx.set_global(ActiveThemeMode(initial_theme));

        // Keep the global in sync whenever settings change
        cx.subscribe(&settings_model, |model: Entity<SettingsModel>, event: &SettingsModelEvent, cx: &mut App| {
            match event {
                SettingsModelEvent::SettingsChanged => {
                    let mode = model.read(cx).settings.theme;
                    cx.set_global(ActiveThemeMode(mode));
                }
            }
        })
        .detach();

        // Open main window
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    RootView::new(
                        workspace_model,
                        chat_model,
                        agent_model,
                        settings_model,
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("Failed to open main window");

        // App menu and quit handling
        cx.activate(true);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "Srow Agent".into(),
            items: vec![MenuItem::action("Quit", Quit)],
        }]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
