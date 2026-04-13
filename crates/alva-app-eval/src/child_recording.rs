// INPUT:  std::sync::Mutex, std::collections::HashMap, alva_kernel_core::middleware::MiddlewareStack,
//         alva_app_core::extension::ChildRunRecording, crate::recorder::RecorderMiddleware
// OUTPUT: ChildRunRecordingImpl
// POS:    Per-run service that records sub-agent runs as nested RunRecords.

//! `ChildRunRecording` implementation used by this eval crate.
//!
//! One instance is created per top-level agent run and registered on the
//! run's bus. When the parent agent invokes the `agent` tool,
//! `AgentSpawnTool` finds this service on the bus and asks it for a
//! middleware stack to drive the child with. We hand back a stack
//! containing a fresh `RecorderMiddleware`, remember it keyed by the
//! parent's tool_call_id, and when the tool call finishes we drain the
//! recorder and stash the `RunRecord`. The parent's own recorder then
//! harvests it via `take_child_record` and attaches it to its
//! `ToolCallRecord.sub_run`.
//!
//! Grandchildren work for free: the child's run inherits the parent's
//! bus (via `ChildAgentParams.bus`), so its own `AgentSpawnTool` finds
//! **the same** service instance and the process repeats.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alva_kernel_core::middleware::MiddlewareStack;
use alva_app_core::extension::ChildRunRecording;

use crate::recorder::{RecorderMiddleware, RunRecord};

/// Per-run `ChildRunRecording` implementation.
pub struct ChildRunRecordingImpl {
    /// Recorders currently capturing a live child run, keyed by the
    /// parent tool_call_id that started them.
    active: Mutex<HashMap<String, Arc<RecorderMiddleware>>>,
    /// Finalized records waiting to be harvested, keyed by the same id.
    completed: Mutex<HashMap<String, RunRecord>>,
}

impl ChildRunRecordingImpl {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for ChildRunRecordingImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl ChildRunRecording for ChildRunRecordingImpl {
    fn begin_child_run(&self, parent_tool_call_id: &str) -> MiddlewareStack {
        // Create a fresh recorder for this child run. We do NOT subscribe
        // to its done_rx — the caller (AgentSpawnTool) signals completion
        // by calling `finalize_child_run`, which is strictly synchronous
        // relative to the tool call lifetime.
        let (recorder, _done_rx) = RecorderMiddleware::new();
        let recorder = Arc::new(recorder);

        let mut stack = MiddlewareStack::new();
        stack.push(recorder.clone());

        self.active
            .lock()
            .unwrap()
            .insert(parent_tool_call_id.to_string(), recorder);

        stack
    }

    fn finalize_child_run(&self, parent_tool_call_id: &str) {
        let recorder = self
            .active
            .lock()
            .unwrap()
            .remove(parent_tool_call_id);

        if let Some(recorder) = recorder {
            let record = recorder.take_record();
            self.completed
                .lock()
                .unwrap()
                .insert(parent_tool_call_id.to_string(), record);
        }
    }

    fn take_child_record(&self, parent_tool_call_id: &str) -> Option<serde_json::Value> {
        let record = self
            .completed
            .lock()
            .unwrap()
            .remove(parent_tool_call_id)?;
        serde_json::to_value(record).ok()
    }
}
