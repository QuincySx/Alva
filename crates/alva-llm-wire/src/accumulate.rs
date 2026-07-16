// INPUT:  crate::{ContentBlock, Message, MessageRole, StreamEvent, UsageMetadata}, serde_json
// OUTPUT: StreamMessageAccumulator, StreamMessageError, message_from_events
// POS:    Canonical stream-event accumulator shared by the kernel and blocking proxy consumers.

use std::collections::HashMap;
use std::fmt;

use crate::{ContentBlock, Message, MessageRole, StreamEvent, UsageMetadata};

/// Failure while turning a model event stream into one assistant message.
#[derive(Debug)]
pub enum StreamMessageError {
    /// The provider emitted a terminal error event.
    Model(String),
    /// A completed tool call did not contain valid JSON arguments.
    InvalidToolArguments {
        id: String,
        name: String,
        source: serde_json::Error,
    },
}

impl fmt::Display for StreamMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(message) => f.write_str(message),
            Self::InvalidToolArguments { id, name, source } => write!(
                f,
                "invalid tool arguments for tool call '{id}' ({name}): {source}"
            ),
        }
    }
}

impl std::error::Error for StreamMessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Model(_) => None,
            Self::InvalidToolArguments { source, .. } => Some(source),
        }
    }
}

/// Incrementally assembles the canonical assistant message represented by a
/// sequence of [`StreamEvent`] values.
///
/// Provider quirks are normalized here once. In particular, an id-less tool
/// delta attaches to the most recent tool call, while an orphan id-less delta
/// is ignored. That is the historical kernel behavior and therefore the
/// compatibility contract for other consumers.
#[derive(Debug, Default)]
pub struct StreamMessageAccumulator {
    text: String,
    reasoning: Vec<(String, Option<String>)>,
    usage: Option<UsageMetadata>,
    tool_calls: Vec<(String, String, String)>,
    tool_call_indices: HashMap<String, usize>,
    last_tool_call_index: Option<usize>,
}

impl StreamMessageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one event. Boundary/progress-only events intentionally do not
    /// affect the completed message.
    pub fn push(&mut self, event: StreamEvent) -> Result<(), StreamMessageError> {
        match event {
            StreamEvent::TextDelta { text } => self.text.push_str(&text),
            StreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
            } => {
                let target_index = if !id.is_empty() {
                    if let Some(existing) = self.tool_call_indices.get(&id).copied() {
                        existing
                    } else {
                        let next = self.tool_calls.len();
                        self.tool_calls
                            .push((id.clone(), String::new(), String::new()));
                        self.tool_call_indices.insert(id, next);
                        next
                    }
                } else if let Some(last) = self.last_tool_call_index {
                    last
                } else {
                    return Ok(());
                };

                self.last_tool_call_index = Some(target_index);
                if let Some((_, existing_name, existing_arguments)) =
                    self.tool_calls.get_mut(target_index)
                {
                    if let Some(name) = name.filter(|name| !name.is_empty()) {
                        *existing_name = name;
                    }
                    existing_arguments.push_str(&arguments_delta);
                }
            }
            StreamEvent::Usage(usage) => self.usage = Some(usage),
            StreamEvent::Error(error) => return Err(StreamMessageError::Model(error)),
            StreamEvent::ToolCallStart { id, name } => {
                // Anthropic emits the tool name ONLY on `content_block_start`
                // (→ our ToolCallStart); the following ToolCallDelta carries the
                // argument stream with `name: None`. Dropping this arm leaves the
                // builder's name "" and tool dispatch later reports
                // "Tool not found: ".
                if !id.is_empty() {
                    let target_index =
                        if let Some(existing) = self.tool_call_indices.get(&id).copied() {
                            existing
                        } else {
                            let next = self.tool_calls.len();
                            self.tool_calls
                                .push((id.clone(), String::new(), String::new()));
                            self.tool_call_indices.insert(id, next);
                            next
                        };
                    self.last_tool_call_index = Some(target_index);
                    if !name.is_empty() {
                        if let Some((_, existing_name, _)) = self.tool_calls.get_mut(target_index) {
                            *existing_name = name;
                        }
                    }
                }
            }
            StreamEvent::ReasoningBlock { text, signature } => {
                // Authoritative capture of a completed thinking block. Anthropic's
                // extended thinking requires the full text + signature to be
                // round-tripped verbatim on the next turn, or it 400s.
                self.reasoning.push((text, signature));
            }
            StreamEvent::Start
            | StreamEvent::Done
            | StreamEvent::ReasoningDelta { .. }
            | StreamEvent::ToolCallEnd { .. }
            | StreamEvent::Stop { .. } => {}
        }
        Ok(())
    }

    /// Historical kernel emptiness semantics used to decide whether to fall
    /// back to `LanguageModel::complete`: reasoning alone does not count.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tool_calls.is_empty()
    }

    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    pub fn tool_call_count(&self) -> usize {
        self.tool_calls.len()
    }

    pub fn has_usage(&self) -> bool {
        self.usage.is_some()
    }

    /// Finish the assistant message with caller-owned identity and timestamp.
    pub fn finish(self, id: String, timestamp: i64) -> Result<Message, StreamMessageError> {
        let mut content = self
            .reasoning
            .into_iter()
            .map(|(text, signature)| ContentBlock::Reasoning { text, signature })
            .collect::<Vec<_>>();
        if !self.text.is_empty() {
            content.push(ContentBlock::Text { text: self.text });
        }
        for (id, name, arguments) in self.tool_calls {
            let input = serde_json::from_str(&arguments).map_err(|source| {
                StreamMessageError::InvalidToolArguments {
                    id: id.clone(),
                    name: name.clone(),
                    source,
                }
            })?;
            content.push(ContentBlock::ToolUse { id, name, input });
        }

        Ok(Message {
            id,
            role: MessageRole::Assistant,
            content,
            tool_call_id: None,
            usage: self.usage,
            timestamp,
        })
    }
}

/// Assemble a complete assistant message from an already-buffered event list.
pub fn message_from_events(
    events: impl IntoIterator<Item = StreamEvent>,
    id: impl Into<String>,
    timestamp: i64,
) -> Result<Message, StreamMessageError> {
    let mut accumulator = StreamMessageAccumulator::new();
    for event in events {
        accumulator.push(event)?;
    }
    accumulator.finish(id.into(), timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_reasoning_text_and_tool_order() {
        let message = message_from_events(
            [
                StreamEvent::ReasoningBlock {
                    text: "think".into(),
                    signature: Some("sig".into()),
                },
                StreamEvent::TextDelta {
                    text: "done".into(),
                },
                StreamEvent::ToolCallStart {
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallDelta {
                    id: String::new(),
                    name: None,
                    arguments_delta: r#"{"path":"a.txt"}"#.into(),
                },
            ],
            "response-1",
            42,
        )
        .expect("assemble stream");

        assert!(matches!(message.content[0], ContentBlock::Reasoning { .. }));
        assert!(matches!(message.content[1], ContentBlock::Text { .. }));
        assert!(matches!(
            &message.content[2],
            ContentBlock::ToolUse { id, name, input }
                if id == "call-1" && name == "read_file" && input == &json!({"path": "a.txt"})
        ));
    }

    #[test]
    fn ignores_orphan_idless_delta_like_the_kernel() {
        let message = message_from_events(
            [StreamEvent::ToolCallDelta {
                id: String::new(),
                name: Some("ignored".into()),
                arguments_delta: "{}".into(),
            }],
            "response-1",
            0,
        )
        .expect("orphan is ignored");
        assert!(message.content.is_empty());
    }

    /// `is_empty` is the kernel's trigger for re-issuing the turn through
    /// `LanguageModel::complete` (run.rs), so a false "non-empty" here costs a
    /// second billed model call and a false "empty" drops the fallback. Text or
    /// a tool call count; reasoning alone does not. The condition used to live
    /// inline in run.rs — now that it crosses a crate boundary, it needs its own
    /// guard.
    #[test]
    fn emptiness_counts_text_and_tool_calls_but_not_reasoning() {
        let mut only_reasoning = StreamMessageAccumulator::new();
        only_reasoning
            .push(StreamEvent::ReasoningBlock {
                text: "thought hard, said nothing".into(),
                signature: None,
            })
            .expect("reasoning is accepted");
        assert!(
            only_reasoning.is_empty(),
            "reasoning alone must still count as empty, or the kernel skips its complete() fallback"
        );

        let mut with_text = StreamMessageAccumulator::new();
        with_text
            .push(StreamEvent::TextDelta { text: "hi".into() })
            .expect("text is accepted");
        assert!(!with_text.is_empty());

        let mut with_tool_call = StreamMessageAccumulator::new();
        with_tool_call
            .push(StreamEvent::ToolCallStart {
                id: "call-1".into(),
                name: "read_file".into(),
            })
            .expect("tool call start is accepted");
        assert!(
            !with_tool_call.is_empty(),
            "a tool call with no text is a real turn, not an empty stream"
        );

        assert!(StreamMessageAccumulator::new().is_empty());
    }

    #[test]
    fn provider_error_is_returned_verbatim() {
        let error = message_from_events(
            [StreamEvent::Error("provider failed".into())],
            "response-1",
            0,
        )
        .expect_err("error event fails assembly");
        assert_eq!(error.to_string(), "provider failed");
    }
}
