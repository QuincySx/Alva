// INPUT:  (re-exports submodules: side_panel, sidebar_tree)
// OUTPUT: pub use side_panel::* (SidePanel)
// POS:    Barrel module for the side panel, re-exporting the SidePanel composite view.
pub mod side_panel;
pub mod sidebar_tree;

pub use side_panel::*;
