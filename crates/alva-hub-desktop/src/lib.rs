// INPUT:  alva, run, store (sub-modules)
// OUTPUT: engine assembly, persisted run seam, timeline and session DTOs
// POS:    Hub Desktop library facade shared by Tauri IPC and product tests.

mod alva;
mod run;
mod store;

pub use alva::build_alva_engine;
pub use run::{run_once, run_persisted, RunOutcome, RunRequest, TimelineEvent, TimelineStatus};
pub use store::{HubSessionDetail, HubSessionSummary, HubStore, StoredTurn};
