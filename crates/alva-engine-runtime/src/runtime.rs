use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;

use crate::error::RuntimeError;
use crate::event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent};
use crate::request::RuntimeRequest;

/// Unified agent engine runtime interface.
///
/// All engine adapters implement this trait. Consumers depend only on
/// this interface and remain agnostic to the underlying engine.
#[async_trait]
pub trait EngineRuntime: Send + Sync {
    /// Execute an agent session and return an event stream.
    ///
    /// Returns `Err` if the engine fails to start (e.g., process spawn failure).
    /// Runtime errors during execution are emitted as `RuntimeEvent::Error`
    /// in the stream, always followed by a terminal `RuntimeEvent::Completed`.
    ///
    /// The returned Stream is `'static` and does not borrow from `&self`.
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError>;

    /// Cancel a running session.
    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError>;

    /// Respond to a permission request from the engine.
    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError>;

    /// Query engine capabilities.
    fn capabilities(&self) -> RuntimeCapabilities;
}

// Compile-time object-safety check.
#[allow(dead_code)]
fn _assert_object_safe(_: &dyn EngineRuntime) {}
