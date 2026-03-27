// INPUT:  gpui, gpui_component (Dialog, WindowExt), crate::models::SettingsModel, crate::views::settings_panel::SettingsPanel
// OUTPUT: pub fn open_settings_dialog()
// POS:    Opens the categorized settings panel inside a larger gpui-component Dialog.
use gpui::{prelude::*, Entity, Window, px};

use crate::models::SettingsModel;
use crate::views::settings_panel::SettingsPanel;

/// Opens the Settings dialog using gpui-component's Dialog API.
pub fn open_settings_dialog(
    settings_model: Entity<SettingsModel>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    use gpui_component::WindowExt as _;

    let panel = cx.new(|cx| SettingsPanel::new(settings_model, window, cx));

    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("设置")
            .width(px(680.))
            .child(
                gpui::div()
                    .h(px(480.))
                    .child(panel.clone()),
            )
    });
}
