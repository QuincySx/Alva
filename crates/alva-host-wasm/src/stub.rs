// INPUT:  std::pin::Pin, async_trait, alva_kernel_abi::*, futures_core::Stream
// OUTPUT: StubLanguageModel
// POS:    Stateless stub LanguageModel for tests, demos, and the smoke probe. Compiles on every target.

//! `StubLanguageModel` — trivial `LanguageModel` implementation that
//! returns a fixed text response with no tool calls. Used by:
//!
//! - Native tests in `agent::tests` (as `EchoOnceModel`'s replacement)
//! - The compile-time smoke probe in `smoke::_wasm_smoke_probe`
//! - wasm-bindgen `entry` demos that need "a working agent" without
//!   wiring a real LLM provider
//!
//! Compiles on every target (native + wasm32). Not gated.

use std::pin::Pin;

use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::{Message, MessageRole};
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;
use futures_core::Stream;

/// A `LanguageModel` that always returns the same response text on
/// both `complete()` and `stream()`, with no tool calls. Useful for
/// tests and wasm demos.
pub struct StubLanguageModel {
    response: String,
}

impl StubLanguageModel {
    /// Construct with a custom fixed response.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

impl Default for StubLanguageModel {
    fn default() -> Self {
        Self {
            response: "stub-response".into(),
        }
    }
}

#[async_trait]
impl LanguageModel for StubLanguageModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse::from_message(Message {
            id: "stub".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: self.response.clone(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        let text = self.response.clone();
        Box::pin(StubStream::new(text))
    }

    fn model_id(&self) -> &str {
        "stub"
    }
}

/// Tiny 2-event stream: one `TextDelta` carrying the response, then
/// `Done`. Avoids pulling in the `futures` umbrella crate just for a
/// one-shot iterator.
struct StubStream {
    events: [Option<StreamEvent>; 2],
    next: usize,
}

impl StubStream {
    fn new(text: String) -> Self {
        Self {
            events: [
                Some(StreamEvent::TextDelta { text }),
                Some(StreamEvent::Done),
            ],
            next: 0,
        }
    }
}

impl Stream for StubStream {
    type Item = StreamEvent;
    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.next >= self.events.len() {
            return std::task::Poll::Ready(None);
        }
        let idx = self.next;
        self.next += 1;
        std::task::Poll::Ready(self.events[idx].take())
    }
}

#[cfg(test)]
mod tests {
    //! StubLanguageModel powers the smoke probe + native agent.rs tests +
    //! wasm-bindgen entry demos. Pin its contract so a refactor that
    //! quietly changes the response shape (e.g. drops `Done`, emits two
    //! `TextDelta`s) doesn't slip through.

    use super::*;
    use std::future::poll_fn;
    use std::pin::Pin;

    #[test]
    fn default_response_is_stub_response() {
        let m = StubLanguageModel::default();
        assert_eq!(m.response, "stub-response");
    }

    #[test]
    fn model_id_is_constant_stub() {
        let m = StubLanguageModel::default();
        assert_eq!(m.model_id(), "stub");
        // Custom-response instance still reports "stub" — model_id is
        // about identity, not output.
        let custom = StubLanguageModel::new("anything");
        assert_eq!(custom.model_id(), "stub");
    }

    #[tokio::test]
    async fn complete_returns_custom_response_as_assistant_message() {
        let m = StubLanguageModel::new("hello");
        let config = ModelConfig::default();
        let resp = m.complete(&[], &[], &config).await.expect("complete");
        let msg = &resp.message;
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            other => panic!("expected text block, got {other:?}"),
        }
        assert!(msg.tool_call_id.is_none(), "stub never sets tool_call_id");
    }

    #[tokio::test]
    async fn stream_yields_exactly_text_delta_then_done() {
        let m = StubLanguageModel::new("hi");
        let config = ModelConfig::default();
        let mut s = m.stream(&[], &[], &config);

        // Event 1: TextDelta with response text
        let e1 = poll_fn(|cx| Pin::new(&mut s).poll_next(cx)).await;
        match e1 {
            Some(StreamEvent::TextDelta { text }) => assert_eq!(text, "hi"),
            other => panic!("expected TextDelta, got {other:?}"),
        }

        // Event 2: Done
        let e2 = poll_fn(|cx| Pin::new(&mut s).poll_next(cx)).await;
        assert!(
            matches!(e2, Some(StreamEvent::Done)),
            "expected Done, got {e2:?}"
        );

        // Event 3: stream terminates
        let e3 = poll_fn(|cx| Pin::new(&mut s).poll_next(cx)).await;
        assert!(e3.is_none(), "stream should terminate after Done");
    }

    /// Re-polling after termination must be safe (return `None` again,
    /// no panic). Some downstream stream combinators poll past `None`.
    #[tokio::test]
    async fn stream_poll_after_termination_is_none_safe() {
        let m = StubLanguageModel::default();
        let config = ModelConfig::default();
        let mut s = m.stream(&[], &[], &config);
        // Drain
        for _ in 0..2 {
            let _ = poll_fn(|cx| Pin::new(&mut s).poll_next(cx)).await;
        }
        // Two extra polls — both must return None without panic
        for _ in 0..2 {
            let e = poll_fn(|cx| Pin::new(&mut s).poll_next(cx)).await;
            assert!(e.is_none());
        }
    }
}
