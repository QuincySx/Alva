// INPUT:  (none)
// OUTPUT: open_agents_dialog, open_skills_dialog, open_settings_dialog
// POS:    Barrel module for dialog views — re-exports dialog opener functions
mod agents_dialog;
mod skills_dialog;
mod settings_dialog;

pub use agents_dialog::open_agents_dialog;
pub use skills_dialog::open_skills_dialog;
pub use settings_dialog::open_settings_dialog;
