// INPUT:  gpui + gpui-component, hub-gpui backend (lib) + ui modules
// OUTPUT: ALVA Hub Desktop Rust-native shell entry — application bootstrap.
// POS:    Same L7 product seam as alva-hub-desktop, 1:1 UI port.
//         Follows crates/alva-hub-gpui/AGENTS.md: Root first layer,
//         gpui_component::init first, dialog/sheet/notification layers.

#![recursion_limit = "256"]

mod ui;

use gpui::*;
use gpui_component::{ActiveTheme, Root};
use ui::workspace::HubWorkspace;

fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);

            cx.spawn(async move |cx| {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(80.), px(60.)),
                        size(px(1180.), px(760.)),
                    ))),
                    window_min_size: Some(size(px(860.), px(600.))),
                    ..WindowOptions::default()
                };
                cx.open_window(options, |window, cx| {
                    let view = cx.new(|cx| HubWorkspace::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().colors.background))
                })
                .expect("Failed to open window");
            })
            .detach();
        });
}
