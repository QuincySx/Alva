// INPUT:  RuntimeEvent, StreamEvent (via RuntimeEvent::MessageDelta)
// OUTPUT: StreamContract, StreamContractError, validate_event_stream, is_consumer_visible_delta,
//         is_fatal_error, is_terminal_runtime_event, SoftStreamWarning
// POS:    Shared multi-engine stream invariants — adapters must produce streams that pass these checks.

//! Stream contract for multi-engine adapters.
//!
//! # Why this exists
//!
//! Connecting engines is cheap; **keeping three mappers honest is not**.
//! Hub (and any future multi-engine consumer) must be able to rely on one
//! lifecycle, not three dialectics. This module is the single place those
//! rules live — adapters unit-test against it; Hub may enforce it in debug.
//!
//! # Lessons absorbed from cindy (`makecindy/cindy`)
//!
//! cindy already paid for multi-vendor (Claude Code + Codex) event-model bugs.
//! We map their hard-won semantics onto our smaller `RuntimeEvent` surface
//! instead of copying their full taxonomy:
//!
//! | cindy concept | Our mapping |
//! |---------------|-------------|
//! | `done` is turn-terminal | `Completed` is the **only** consumer terminal |
//! | `error.isTerminal` / `willRetry` | `Error.recoverable == false` / `true` |
//! | terminal error must not also `done` in a way that double-ends | fatal `Error` then **only** `Completed` (no mid-stream events after fatal) |
//! | retryable error must not free UI "running" | consumers wait for `Completed`; recoverable Error is informational |
//! | unknown SDK messages: warn + drop | adapters return `[]` for unknown frames |
//! | silent-stop vs empty-response | adapter policy (see claude mapper); contract only requires clean close |
//! | tool args deltas ≠ UI text | `MessageDelta` only Text/Reasoning (`is_consumer_visible_delta`) |
//!
//! # Canonical lifecycle
//!
//! ```text
//! SessionStarted
//!   └── (MessageDelta | Message | ToolStart | ToolEnd | PermissionRequest | Error{recoverable:true})*
//!         └── [ Error{recoverable:false} ]?
//!               └── Completed   ← exactly once, always last
//! ```
//!
//! # Hard rules
//!
//! 1. **`Completed` is the only terminal event** and must appear exactly once,
//!    as the last event. Consumers wait for it before cleanup.
//! 2. **Non-recoverable `Error` must be followed by `Completed`**. After a
//!    fatal Error, **only** `Completed` may still appear (no tools/messages).
//! 3. **No events after `Completed`**.
//! 4. **`SessionStarted` at most once** (zero is tolerated for broken engines
//!    that die before init, but production adapters should emit it).
//! 5. **Tool pairing**: every `ToolEnd.id` should have seen a prior unmatched
//!    `ToolStart` with the same id. Extra `ToolStart` without `ToolEnd` is a
//!    soft warning (cancel/kill paths); duplicate `ToolStart` for the same id
//!    before `ToolEnd` is a hard error.
//! 6. **`MessageDelta` payload**: only consumer-visible stream deltas
//!    (`TextDelta`, `ReasoningDelta`) should be forwarded. Control frames
//!    and tool-arg deltas must be filtered by adapters.
//! 7. **`Message.content`**: no `ToolUse` / `ToolResult` blocks — those are
//!    `ToolStart` / `ToolEnd` only.
//!
//! Soft vs hard: soft violations return `Ok` with warnings collected when
//! using [`StreamContract::check_soft`]; [`validate_event_stream`] enforces
//! hard rules only.

use std::collections::HashSet;
use std::fmt;

use alva_kernel_abi::{ContentBlock, StreamEvent};

use crate::event::RuntimeEvent;

/// Hard stream-contract violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamContractError {
    pub index: usize,
    pub kind: StreamContractViolation,
}

impl fmt::Display for StreamContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stream contract violation at event[{}]: {}",
            self.index, self.kind
        )
    }
}

impl std::error::Error for StreamContractError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamContractViolation {
    /// A second `SessionStarted` was observed.
    DuplicateSessionStarted,
    /// Event after the terminal `Completed`.
    EventAfterCompleted,
    /// Stream ended without `Completed`.
    MissingCompleted,
    /// More than one `Completed`.
    DuplicateCompleted,
    /// Fatal `Error` was not followed by `Completed` as the next (or final) event.
    FatalErrorWithoutCompleted,
    /// Non-`Completed` event after a fatal `Error` (double-end / dirty teardown).
    EventAfterFatalError,
    /// `ToolStart` for an id that is already open.
    DuplicateToolStart { id: String },
    /// `ToolEnd` for an id with no open `ToolStart`.
    OrphanToolEnd { id: String },
    /// `Message.content` still carries tool blocks.
    MessageContainsToolBlocks { message_id: String },
    /// `MessageDelta` carried a non-UI control frame.
    NonVisibleMessageDelta,
}

impl fmt::Display for StreamContractViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSessionStarted => write!(f, "duplicate SessionStarted"),
            Self::EventAfterCompleted => write!(f, "event after Completed"),
            Self::MissingCompleted => write!(f, "stream missing Completed"),
            Self::DuplicateCompleted => write!(f, "duplicate Completed"),
            Self::FatalErrorWithoutCompleted => {
                write!(f, "fatal Error must be followed by Completed")
            }
            Self::EventAfterFatalError => {
                write!(f, "only Completed may follow a fatal Error")
            }
            Self::DuplicateToolStart { id } => {
                write!(f, "duplicate ToolStart for id '{id}' before ToolEnd")
            }
            Self::OrphanToolEnd { id } => write!(f, "ToolEnd without ToolStart for id '{id}'"),
            Self::MessageContainsToolBlocks { message_id } => {
                write!(f, "Message '{message_id}' contains ToolUse/ToolResult blocks")
            }
            Self::NonVisibleMessageDelta => {
                write!(f, "MessageDelta is not a consumer-visible text/reasoning delta")
            }
        }
    }
}

/// Soft (non-fatal) stream hygiene issues — useful in debug / adapter tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoftStreamWarning {
    /// `ToolStart` without matching `ToolEnd` when the stream finished.
    UnclosedTool { id: String },
    /// Stream never emitted `SessionStarted`.
    MissingSessionStarted,
    /// Fatal path closed with a non-empty `Completed.result` (usually wrong).
    FatalCompletedWithResult,
}

impl fmt::Display for SoftStreamWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnclosedTool { id } => write!(f, "unclosed ToolStart id '{id}'"),
            Self::MissingSessionStarted => write!(f, "stream missing SessionStarted"),
            Self::FatalCompletedWithResult => {
                write!(f, "fatal Error path Completed with non-empty result")
            }
        }
    }
}

/// Whether this event is a non-recoverable engine error.
///
/// cindy: `isTerminal === true` / missing flags default terminal.
/// `recoverable: true` ≈ cindy `willRetry` / non-terminal error banner.
pub fn is_fatal_error(event: &RuntimeEvent) -> bool {
    matches!(
        event,
        RuntimeEvent::Error {
            recoverable: false,
            ..
        }
    )
}

/// Whether consumers may treat the turn as finished and free running state.
///
/// **Only `Completed`.** Unlike cindy (where terminal error alone can end a
/// turn), our adapters always pair fatal errors with `Completed` so Hub has
/// one cleanup edge. Do not free UI on recoverable `Error`.
pub fn is_terminal_runtime_event(event: &RuntimeEvent) -> bool {
    matches!(event, RuntimeEvent::Completed { .. })
}

/// Whether a [`StreamEvent`] is useful for product UI streaming.
///
/// Adapters must only emit these as `RuntimeEvent::MessageDelta`.
/// Tool-arg partial JSON (`ToolCallDelta`) and control frames stay adapter-local.
pub fn is_consumer_visible_delta(delta: &StreamEvent) -> bool {
    matches!(
        delta,
        StreamEvent::TextDelta { .. } | StreamEvent::ReasoningDelta { .. }
    )
}

/// Whether a content block is allowed inside `RuntimeEvent::Message`.
pub fn is_message_content_block(block: &ContentBlock) -> bool {
    !matches!(
        block,
        ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
    )
}

/// Incremental checker for a single execute() stream.
#[derive(Debug, Default)]
pub struct StreamContract {
    index: usize,
    session_started: bool,
    completed: bool,
    open_tools: HashSet<String>,
    /// Fatal error seen; next non-Completed hard-fails, Completed closes.
    awaiting_completed_after_fatal: bool,
    fatal_error_index: Option<usize>,
    /// Soft: Completed after fatal had a result string.
    fatal_completed_with_result: bool,
}

impl StreamContract {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one event. Returns hard violations only.
    pub fn observe(&mut self, event: &RuntimeEvent) -> Result<(), StreamContractError> {
        let index = self.index;
        self.index += 1;

        if self.completed {
            return Err(StreamContractError {
                index,
                kind: StreamContractViolation::EventAfterCompleted,
            });
        }

        // cindy empty-response lesson: after a terminal error, do not keep
        // emitting mid-turn content that forces a second "end" path.
        if self.awaiting_completed_after_fatal && !matches!(event, RuntimeEvent::Completed { .. })
        {
            return Err(StreamContractError {
                index,
                kind: StreamContractViolation::EventAfterFatalError,
            });
        }

        match event {
            RuntimeEvent::SessionStarted { .. } => {
                if self.session_started {
                    return Err(StreamContractError {
                        index,
                        kind: StreamContractViolation::DuplicateSessionStarted,
                    });
                }
                self.session_started = true;
            }
            RuntimeEvent::Message { id, content, .. } => {
                if content.iter().any(|b| !is_message_content_block(b)) {
                    return Err(StreamContractError {
                        index,
                        kind: StreamContractViolation::MessageContainsToolBlocks {
                            message_id: id.clone(),
                        },
                    });
                }
            }
            RuntimeEvent::MessageDelta { delta, .. } => {
                if !is_consumer_visible_delta(delta) {
                    return Err(StreamContractError {
                        index,
                        kind: StreamContractViolation::NonVisibleMessageDelta,
                    });
                }
            }
            RuntimeEvent::ToolStart { id, .. } => {
                if !self.open_tools.insert(id.clone()) {
                    return Err(StreamContractError {
                        index,
                        kind: StreamContractViolation::DuplicateToolStart { id: id.clone() },
                    });
                }
            }
            RuntimeEvent::ToolEnd { id, .. } => {
                if !self.open_tools.remove(id) {
                    return Err(StreamContractError {
                        index,
                        kind: StreamContractViolation::OrphanToolEnd { id: id.clone() },
                    });
                }
            }
            RuntimeEvent::PermissionRequest { .. } => {}
            RuntimeEvent::Error { recoverable, .. } => {
                if !*recoverable {
                    self.awaiting_completed_after_fatal = true;
                    self.fatal_error_index = Some(index);
                }
            }
            RuntimeEvent::Completed { result, .. } => {
                if self.completed {
                    return Err(StreamContractError {
                        index,
                        kind: StreamContractViolation::DuplicateCompleted,
                    });
                }
                if self.awaiting_completed_after_fatal
                    && result.as_ref().is_some_and(|s| !s.is_empty())
                {
                    self.fatal_completed_with_result = true;
                }
                self.completed = true;
                self.awaiting_completed_after_fatal = false;
            }
        }

        Ok(())
    }

    /// Finish the stream (no more events). Hard rules only.
    pub fn finish(&self) -> Result<(), StreamContractError> {
        if self.awaiting_completed_after_fatal {
            return Err(StreamContractError {
                index: self.fatal_error_index.unwrap_or(self.index),
                kind: StreamContractViolation::FatalErrorWithoutCompleted,
            });
        }
        if !self.completed {
            return Err(StreamContractError {
                index: self.index.saturating_sub(1),
                kind: StreamContractViolation::MissingCompleted,
            });
        }
        Ok(())
    }

    /// Soft hygiene warnings (call after a successful [`finish`]).
    pub fn soft_warnings(&self) -> Vec<SoftStreamWarning> {
        let mut out = Vec::new();
        if !self.session_started {
            out.push(SoftStreamWarning::MissingSessionStarted);
        }
        for id in &self.open_tools {
            out.push(SoftStreamWarning::UnclosedTool { id: id.clone() });
        }
        if self.fatal_completed_with_result {
            out.push(SoftStreamWarning::FatalCompletedWithResult);
        }
        out
    }

    /// Validate hard rules and return soft warnings (empty if clean).
    pub fn check_soft(
        events: &[RuntimeEvent],
    ) -> Result<Vec<SoftStreamWarning>, StreamContractError> {
        let mut contract = StreamContract::new();
        for event in events {
            contract.observe(event)?;
        }
        contract.finish()?;
        Ok(contract.soft_warnings())
    }
}

/// Validate a complete event sequence against hard stream-contract rules.
pub fn validate_event_stream(events: &[RuntimeEvent]) -> Result<(), StreamContractError> {
    let mut contract = StreamContract::new();
    for event in events {
        contract.observe(event)?;
    }
    contract.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{MessageRole, StreamEvent, ToolOutput};
    use serde_json::json;

    fn session_started() -> RuntimeEvent {
        RuntimeEvent::SessionStarted {
            session_id: "s1".into(),
            model: Some("m".into()),
            tools: vec![],
        }
    }

    fn completed() -> RuntimeEvent {
        RuntimeEvent::Completed {
            session_id: "s1".into(),
            result: Some("ok".into()),
            usage: None,
        }
    }

    fn completed_none() -> RuntimeEvent {
        RuntimeEvent::Completed {
            session_id: "s1".into(),
            result: None,
            usage: None,
        }
    }

    #[test]
    fn happy_path_passes() {
        let events = vec![
            session_started(),
            RuntimeEvent::MessageDelta {
                id: "m1".into(),
                delta: StreamEvent::TextDelta {
                    text: "he".into(),
                },
            },
            RuntimeEvent::Message {
                id: "m1".into(),
                role: MessageRole::Assistant,
                content: vec![alva_kernel_abi::ContentBlock::Text {
                    text: "hello".into(),
                }],
            },
            RuntimeEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            },
            RuntimeEvent::ToolEnd {
                id: "t1".into(),
                name: "bash".into(),
                result: ToolOutput::text("ok"),
                duration_ms: None,
            },
            completed(),
        ];
        assert!(validate_event_stream(&events).is_ok());
        assert!(StreamContract::check_soft(&events).unwrap().is_empty());
    }

    #[test]
    fn fatal_error_requires_completed() {
        let events = vec![
            session_started(),
            RuntimeEvent::Error {
                message: "boom".into(),
                recoverable: false,
            },
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::FatalErrorWithoutCompleted,
                ..
            })
        ));
    }

    #[test]
    fn fatal_error_then_completed_passes() {
        let events = vec![
            session_started(),
            RuntimeEvent::Error {
                message: "boom".into(),
                recoverable: false,
            },
            completed_none(),
        ];
        assert!(validate_event_stream(&events).is_ok());
    }

    #[test]
    fn event_between_fatal_error_and_completed_fails() {
        // cindy empty-response: terminal error must not be followed by more
        // turn content before the single cleanup edge.
        let events = vec![
            session_started(),
            RuntimeEvent::Error {
                message: "empty-response".into(),
                recoverable: false,
            },
            RuntimeEvent::Message {
                id: "late".into(),
                role: MessageRole::Assistant,
                content: vec![alva_kernel_abi::ContentBlock::Text {
                    text: "should not appear".into(),
                }],
            },
            completed_none(),
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::EventAfterFatalError,
                ..
            })
        ));
    }

    #[test]
    fn recoverable_error_does_not_require_immediate_completed() {
        let events = vec![
            session_started(),
            RuntimeEvent::Error {
                message: "Reconnecting... 2/5".into(),
                recoverable: true,
            },
            RuntimeEvent::MessageDelta {
                id: "m".into(),
                delta: StreamEvent::TextDelta {
                    text: "still going".into(),
                },
            },
            completed(),
        ];
        assert!(validate_event_stream(&events).is_ok());
    }

    #[test]
    fn event_after_completed_fails() {
        let events = vec![
            session_started(),
            completed(),
            RuntimeEvent::Error {
                message: "late".into(),
                recoverable: true,
            },
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::EventAfterCompleted,
                ..
            })
        ));
    }

    #[test]
    fn orphan_tool_end_fails() {
        let events = vec![
            session_started(),
            RuntimeEvent::ToolEnd {
                id: "t1".into(),
                name: "bash".into(),
                result: ToolOutput::text("x"),
                duration_ms: None,
            },
            completed(),
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::OrphanToolEnd { .. },
                ..
            })
        ));
    }

    #[test]
    fn duplicate_tool_start_fails() {
        let events = vec![
            session_started(),
            RuntimeEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            },
            RuntimeEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            },
            completed(),
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::DuplicateToolStart { .. },
                ..
            })
        ));
    }

    #[test]
    fn non_visible_delta_fails() {
        let events = vec![
            session_started(),
            RuntimeEvent::MessageDelta {
                id: "m".into(),
                delta: StreamEvent::Start,
            },
            completed(),
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::NonVisibleMessageDelta,
                ..
            })
        ));
    }

    #[test]
    fn tool_call_delta_is_not_consumer_visible() {
        // cindy/tool streaming: arg partials must not leak as MessageDelta.
        let delta = StreamEvent::ToolCallDelta {
            id: "0".into(),
            name: None,
            arguments_delta: "{\"a\":".into(),
        };
        assert!(!is_consumer_visible_delta(&delta));
        let events = vec![
            session_started(),
            RuntimeEvent::MessageDelta {
                id: "m".into(),
                delta,
            },
            completed(),
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::NonVisibleMessageDelta,
                ..
            })
        ));
    }

    #[test]
    fn message_with_tool_use_fails() {
        let events = vec![
            session_started(),
            RuntimeEvent::Message {
                id: "m".into(),
                role: MessageRole::Assistant,
                content: vec![alva_kernel_abi::ContentBlock::ToolUse {
                    id: "t".into(),
                    name: "bash".into(),
                    input: json!({}),
                }],
            },
            completed(),
        ];
        assert!(matches!(
            validate_event_stream(&events),
            Err(StreamContractError {
                kind: StreamContractViolation::MessageContainsToolBlocks { .. },
                ..
            })
        ));
    }

    #[test]
    fn soft_warns_unclosed_tool() {
        let events = vec![
            session_started(),
            RuntimeEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            },
            completed_none(),
        ];
        let warnings = StreamContract::check_soft(&events).unwrap();
        assert!(warnings.iter().any(|w| matches!(
            w,
            SoftStreamWarning::UnclosedTool { id } if id == "t1"
        )));
    }

    #[test]
    fn soft_warns_fatal_completed_with_result() {
        let events = vec![
            session_started(),
            RuntimeEvent::Error {
                message: "boom".into(),
                recoverable: false,
            },
            completed(), // result Some("ok") — wrong for fatal path
        ];
        let warnings = StreamContract::check_soft(&events).unwrap();
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SoftStreamWarning::FatalCompletedWithResult)));
    }

    #[test]
    fn is_terminal_is_only_completed() {
        assert!(!is_terminal_runtime_event(&RuntimeEvent::Error {
            message: "x".into(),
            recoverable: false,
        }));
        assert!(is_terminal_runtime_event(&completed_none()));
        assert!(is_fatal_error(&RuntimeEvent::Error {
            message: "x".into(),
            recoverable: false,
        }));
        assert!(!is_fatal_error(&RuntimeEvent::Error {
            message: "retry".into(),
            recoverable: true,
        }));
    }

    #[test]
    fn is_consumer_visible_delta_filters_control_frames() {
        assert!(is_consumer_visible_delta(&StreamEvent::TextDelta {
            text: "a".into()
        }));
        assert!(is_consumer_visible_delta(&StreamEvent::ReasoningDelta {
            text: "r".into()
        }));
        assert!(!is_consumer_visible_delta(&StreamEvent::Start));
        assert!(!is_consumer_visible_delta(&StreamEvent::Done));
        assert!(!is_consumer_visible_delta(&StreamEvent::ToolCallStart {
            id: "t".into(),
            name: "x".into(),
        }));
    }
}
