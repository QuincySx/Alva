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
        Self { response: response.into() }
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
