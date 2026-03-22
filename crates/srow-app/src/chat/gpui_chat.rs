// INPUT:  gpui (Context, EventEmitter)
// OUTPUT: pub struct GpuiChat, pub struct GpuiChatConfig, pub enum GpuiChatEvent, pub struct SharedRuntime
// POS:    GPUI Entity wrapping chat abstraction.
//         Implementation commented out during migration — depends on deleted srow-ai and UIMessage types.
//         TODO: Rebuild on agent-core.
use std::sync::Arc;

use gpui::{Context, EventEmitter};

/// GPUI Global holding a shared tokio runtime for all GpuiChat instances.
pub struct SharedRuntime(pub Arc<tokio::runtime::Runtime>);

impl gpui::Global for SharedRuntime {}

/// Events emitted by GpuiChat to any GPUI subscriber.
pub enum GpuiChatEvent {
    Updated,
}

impl EventEmitter<GpuiChatEvent> for GpuiChat {}

/// Configuration for creating a GpuiChat instance.
pub struct GpuiChatConfig {
    pub session_id: String,
}

/// GPUI Entity wrapping chat abstraction.
/// TODO: Rebuild on agent-core.
pub struct GpuiChat {
    _session_id: String,
}

impl GpuiChat {
    pub fn new(config: GpuiChatConfig, _cx: &mut Context<Self>) -> Self {
        Self {
            _session_id: config.session_id,
        }
    }

    pub fn send_message(&self, _text: &str) {
        tracing::warn!("GpuiChat::send_message: TODO rebuild on agent-core");
    }

    pub fn stop(&self) {
        tracing::warn!("GpuiChat::stop: TODO rebuild on agent-core");
    }

    pub fn id(&self) -> String {
        self._session_id.clone()
    }
}
