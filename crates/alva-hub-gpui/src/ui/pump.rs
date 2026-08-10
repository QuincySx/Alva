// INPUT:  HubApp store / active_engines / worker_pumps
// OUTPUT: per-worker queue pumps — claim the next task, run it through the
//         engine, finish with result, then drain the queue.
// POS:    1:1 GPUI port of the Tauri worker pump (ensure_worker_pump +
//         run_worker_task) using the migrated store seams. Implemented in
//         a later batch; `ensure_worker_pump` is a no-op placeholder here.

use gpui::*;

use super::app::HubApp;

impl HubApp {
    /// Ensure a per-worker pump is draining this worker's durable FIFO
    /// queue. Currently a no-op placeholder (batch 5b implements the loop).
    pub fn ensure_worker_pump(&self, _worker_id: &str, _cx: &mut Context<Self>) {}
}
