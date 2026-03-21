// INPUT:  (re-exports submodule: settings_panel)
// OUTPUT: pub use settings_panel::* (SettingsPanel)
// POS:    Barrel module for the settings panel, re-exporting the SettingsPanel view.
pub mod settings_panel;

pub use settings_panel::*;
