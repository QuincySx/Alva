// INPUT:  gpui + gpui-component, hub-gpui backend (lib) + ui modules
// OUTPUT: ALVA Hub Desktop Rust-native shell entry — application bootstrap.
// POS:    Same L7 product seam as alva-hub-desktop, 1:1 UI port.
//         Follows crates/alva-hub-gpui/AGENTS.md: Root first layer,
//         gpui_component::init first, dialog/sheet/notification layers.

mod ui;

use gpui::*;
use gpui_component::{ActiveTheme, Root};
use ui::app::HubApp;

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
