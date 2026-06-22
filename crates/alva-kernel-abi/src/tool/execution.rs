// INPUT:  async_trait, serde, serde_json, CancellationToken, ToolFs, alva_kernel_bus::BusHandle
// OUTPUT: ProgressEvent, ToolContent, ToolOutput, ToolExecutionContext (trait), MinimalExecutionContext
// POS:    Unified execution context and multi-modal output types — ToolExecutionContext exposes bus() for tool-level capability discovery.
use async_trait::async_trait;
use std::any::Any;
use std::path::Path;

use super::types::ToolFs;
use crate::base::cancel::CancellationToken;
use alva_kernel_bus::BusHandle;

// Re-export pure-serde payload types so the canonical path
// `crate::tool::execution::{ProgressEvent, ToolContent, ToolOutput}` continues to resolve.
pub use super::content_payload::{ProgressEvent, ToolContent, ToolOutput};

// ---------------------------------------------------------------------------
// ToolExecutionContext — unified context trait
// ---------------------------------------------------------------------------

/// Unified execution context passed to `Tool::execute`.
///
/// Merges the old `CancellationToken` + `ToolContext` + `LocalToolContext`
/// into a single trait. Every tool receives one object that provides
/// cancellation, progress reporting, configuration, filesystem access,
/// and downcast support.
#[async_trait]
pub trait ToolExecutionContext: Send + Sync {
    /// Cooperative cancellation token for this execution.
    fn cancel_token(&self) -> &CancellationToken;

    /// Report intermediate progress (no-op by default).
    fn report_progress(&self, _event: ProgressEvent) {}

    /// Session identifier for the current agent session.
    fn session_id(&self) -> &str;

    /// ID of the `ToolCall` currently being executed, if available.
    ///
    /// Used by tools that need to correlate side-channel state with a
    /// specific dispatched call (e.g. sub-agent spawning tools keying
    /// their child run records by the parent tool_call's id so the
    /// parent recorder can attach them later).
    ///
    /// Returns `None` when the context does not track tool call identity
    /// (e.g. `MinimalExecutionContext` used in tests).
    fn tool_call_id(&self) -> Option<&str> {
        None
    }

    /// Read a configuration value by key.
    fn get_config(&self, _key: &str) -> Option<String> {
        None
    }

    /// Workspace / project root path (None for remote or sessionless contexts).
    fn workspace(&self) -> Option<&Path> {
        None
    }

    /// Whether the tool is allowed to perform dangerous operations.
    fn allow_dangerous(&self) -> bool {
        false
    }

    /// Abstract filesystem interface (sandbox, remote, or mock).
    /// When None, tools fall back to direct local operations.
    fn tool_fs(&self) -> Option<&dyn ToolFs> {
        None
    }

    /// Cross-layer coordination bus handle.
    /// Returns None when bus is not wired (e.g., in tests using MinimalExecutionContext).
    fn bus(&self) -> Option<&BusHandle> {
        None
    }

    /// Scoped session handle for this tool invocation.
    ///
    /// Returns `Some` when the runtime has wired an `AgentSession` into
    /// this execution context; events appended through the returned
    /// `ScopedSession` are automatically stamped with
    /// `EmitterKind::Tool` and this tool's registered id.
    ///
    /// Returns `None` for contexts that do not carry a session (tests,
    /// `MinimalExecutionContext`, standalone tool runners).
    fn session(&self) -> Option<&crate::agent_session::ScopedSession> {
        None
    }

    /// Downcast support for application-specific extensions.
    fn as_any(&self) -> &dyn Any;
}

// ---------------------------------------------------------------------------
// MinimalExecutionContext — replaces EmptyToolContext
// ---------------------------------------------------------------------------

/// Minimal execution context for tools that don't need runtime information.
///
/// Provides a cancellation token and empty/no-op defaults for everything else.
/// Useful in tests and for tools that are self-contained.
pub struct MinimalExecutionContext {
    cancel: CancellationToken,
}

impl MinimalExecutionContext {
    /// Create a new minimal context with a fresh cancellation token.
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
        }
    }

    /// Create a minimal context wrapping an existing cancellation token.
    pub fn with_cancel(cancel: CancellationToken) -> Self {
        Self { cancel }
    }
}

impl Default for MinimalExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolExecutionContext for MinimalExecutionContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    fn session_id(&self) -> &str {
        ""
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_context_defaults() {
        let ctx = MinimalExecutionContext::new();
        assert_eq!(ctx.session_id(), "");
        assert!(!ctx.cancel_token().is_cancelled());
        assert!(ctx.get_config("any").is_none());
        assert!(ctx.workspace().is_none());
        assert!(!ctx.allow_dangerous());
        assert!(ctx.tool_fs().is_none());
    }

    #[test]
    fn minimal_context_with_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let ctx = MinimalExecutionContext::with_cancel(cancel);
        assert!(ctx.cancel_token().is_cancelled());
    }
}
