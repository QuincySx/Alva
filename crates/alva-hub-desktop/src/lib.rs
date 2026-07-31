// INPUT:  alva, run (sub-modules)
// OUTPUT: build_alva_engine, run_once, RunOutcome, RunRequest, TimelineEvent, TimelineStatus
// POS:    Hub Desktop library facade shared by Tauri IPC and product-level tests.

mod alva;
mod run;

pub use alva::build_alva_engine;
pub use run::{run_once, RunOutcome, RunRequest, TimelineEvent, TimelineStatus};
