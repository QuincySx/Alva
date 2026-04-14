//! Agent-layer core: the Extension system and test-grade ToolFs.
//!
//! This crate holds the pure agent-internal extension machinery that used
//! to live inside `alva-app-core/src/extension/`, plus `MockToolFs` which
//! used to live in `alva-agent-tools`. It deliberately does NOT depend on
//! any protocol crate, LLM provider, persistence, or host-specific code.
