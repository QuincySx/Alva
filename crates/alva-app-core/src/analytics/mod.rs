// INPUT:  events, sink submodules
// OUTPUT: AnalyticsEvent, AnalyticsSink, AnalyticsService, FileAnalyticsSink, event_names
// POS:    Analytics module root — re-exports event types and sink infrastructure.

//! Analytics — event tracking with pluggable sinks.
//!
//! Provides a fail-open analytics pipeline: events are queued when no sink
//! is attached and flushed when one becomes available. Multiple sinks can
//! be attached for multiplexing (e.g., file + remote).

pub mod events;
pub mod sink;

pub use events::*;
pub use sink::*;
