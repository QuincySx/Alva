// INPUT:  alva_types::{LanguageModel, Message, ModelConfig, StreamEvent, AgentError, Tool}, Arc, Mutex
// OUTPUT: MockLanguageModel — configurable mock for LanguageModel with preset responses, error queuing, stream events, and call recording
// POS:    alva-test crate — provides a test double for LanguageModel used in unit and integration tests

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;
use futures::Stream;

use alva_types::{AgentError, CompletionResponse, LanguageModel, Message, ModelConfig, StreamEvent};
use alva_types::tool::Tool;

/// A queued response entry: either a successful Message or an error.
enum QueuedResponse {
    Ok(Message),
    Err(AgentError),
}

struct MockState {
    /// Ordered queue of responses returned by `complete`.
    response_queue: Vec<QueuedResponse>,
    /// Index into the queue for the next `complete` call.
    call_index: usize,
    /// Record of all `complete` call argument slices (one Vec<Message> per call).
    calls: Vec<Vec<Message>>,
    /// Events emitted by `stream`.
    stream_events: Vec<StreamEvent>,
}

/// A mock implementation of [`LanguageModel`] for use in tests.
///
/// Supports:
/// - Queuing preset responses (in order) via [`with_response`] / [`with_error`].
/// - Queuing a stream event sequence via [`with_stream_events`].
/// - Recording every `complete` call for assertion via [`calls`].
///
/// Uses `Arc<Mutex<...>>` internally so the struct can be cloned and still share
/// state across handles, which is required because the trait takes `&self`.
#[derive(Clone)]
pub struct MockLanguageModel {
    state: Arc<Mutex<MockState>>,
}

impl MockLanguageModel {
    /// Create a new mock with an empty response queue.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                response_queue: Vec::new(),
                call_index: 0,
                calls: Vec::new(),
                stream_events: Vec::new(),
            })),
        }
    }

    /// Queue a successful response (builder pattern, consumes and returns `self`).
    pub fn with_response(self, response: Message) -> Self {
        self.state
            .lock()
            .unwrap()
            .response_queue
            .push(QueuedResponse::Ok(response));
        self
    }

    /// Queue an error response (builder pattern, consumes and returns `self`).
    pub fn with_error(self, err: AgentError) -> Self {
        self.state
            .lock()
            .unwrap()
            .response_queue
            .push(QueuedResponse::Err(err));
        self
    }

    /// Set the events emitted by [`stream`] (replaces any previous setting).
    pub fn with_stream_events(self, events: Vec<StreamEvent>) -> Self {
        self.state.lock().unwrap().stream_events = events;
        self
    }

    /// Return all recorded `complete` calls.
    ///
    /// Each element is the `messages` slice passed to one `complete` invocation.
    pub fn calls(&self) -> Vec<Vec<Message>> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl Default for MockLanguageModel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LanguageModel for MockLanguageModel {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        let mut state = self.state.lock().unwrap();

        // Record this call.
        state.calls.push(messages.to_vec());

        let index = state.call_index;
        if index >= state.response_queue.len() {
            return Err(AgentError::LlmError(format!(
                "MockLanguageModel: no response queued for call index {index}"
            )));
        }

        state.call_index += 1;

        match &state.response_queue[index] {
            QueuedResponse::Ok(msg) => Ok(CompletionResponse::from_message(msg.clone())),
            QueuedResponse::Err(err) => Err(AgentError::LlmError(err.to_string())),
        }
    }

    fn stream(
        &self,
        messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        let state_events = self.state.lock().unwrap().stream_events.clone();
        if !state_events.is_empty() {
            // Use explicitly configured stream events
            return Box::pin(stream::iter(state_events));
        }

        // Auto-convert queued complete() response into stream events
        // so tests using with_response() work with the streaming run loop.
        let mut state = self.state.lock().unwrap();
        state.calls.push(messages.to_vec());
        let index = state.call_index;
        if index >= state.response_queue.len() {
            return Box::pin(stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::Error(format!(
                    "MockLanguageModel: no response queued for call index {index}"
                )),
            ]));
        }
        state.call_index += 1;

        match &state.response_queue[index] {
            QueuedResponse::Err(err) => {
                Box::pin(stream::iter(vec![
                    StreamEvent::Start,
                    StreamEvent::Error(err.to_string()),
                ]))
            }
            QueuedResponse::Ok(msg) => {
                let mut events = vec![StreamEvent::Start];
                for block in &msg.content {
                    match block {
                        alva_types::ContentBlock::Text { text } => {
                            events.push(StreamEvent::TextDelta { text: text.clone() });
                        }
                        alva_types::ContentBlock::ToolUse { id, name, input } => {
                            events.push(StreamEvent::ToolCallDelta {
                                id: id.clone(),
                                name: Some(name.clone()),
                                arguments_delta: input.to_string(),
                            });
                        }
                        _ => {}
                    }
                }
                if let Some(usage) = &msg.usage {
                    events.push(StreamEvent::Usage(usage.clone()));
                }
                events.push(StreamEvent::Done);
                Box::pin(stream::iter(events))
            }
        }
    }

    fn model_id(&self) -> &str {
        "mock-language-model"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{ContentBlock, Message, MessageRole, ModelConfig};

    #[tokio::test]
    async fn test_mock_returns_preset_response() {
        let response = Message {
            id: "resp-1".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "Hello!".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let mock = MockLanguageModel::new().with_response(response.clone());

        let result = mock
            .complete(&[], &[], &ModelConfig::default())
            .await
            .unwrap()
            .message;
        assert_eq!(result.content.len(), response.content.len());
        match (&result.content[0], &response.content[0]) {
            (ContentBlock::Text { text: a }, ContentBlock::Text { text: b }) => {
                assert_eq!(a, b);
            }
            _ => panic!("expected text content"),
        }
    }

    #[tokio::test]
    async fn test_mock_records_calls() {
        let response = Message {
            id: "resp-1".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "ok".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let mock = MockLanguageModel::new().with_response(response);

        let input_msg = Message {
            id: "msg-1".into(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let _ = mock
            .complete(&[input_msg.clone()], &[], &ModelConfig::default())
            .await;
        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].len(), 1);
        assert_eq!(calls[0][0].id, "msg-1");
    }

    #[tokio::test]
    async fn test_mock_sequential_responses() {
        let r1 = Message {
            id: "r1".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "first".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let r2 = Message {
            id: "r2".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "second".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let mock = MockLanguageModel::new().with_response(r1).with_response(r2);

        let res1 = mock
            .complete(&[], &[], &ModelConfig::default())
            .await
            .unwrap()
            .message;
        let res2 = mock
            .complete(&[], &[], &ModelConfig::default())
            .await
            .unwrap()
            .message;

        match &res1.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "first"),
            _ => panic!("expected text"),
        }
        match &res2.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "second"),
            _ => panic!("expected text"),
        }
    }

    #[tokio::test]
    async fn test_mock_stream_emits_events() {
        use futures::StreamExt;

        let mock = MockLanguageModel::new().with_stream_events(vec![
            StreamEvent::Start,
            StreamEvent::TextDelta { text: "Hello".into() },
            StreamEvent::TextDelta { text: " world".into() },
            StreamEvent::Done,
        ]);

        let stream = mock.stream(&[], &[], &ModelConfig::default());
        let events: Vec<_> = stream.collect::<Vec<_>>().await;
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], StreamEvent::Start));
        assert!(matches!(events[3], StreamEvent::Done));
    }

    #[tokio::test]
    async fn test_mock_returns_error() {
        let mock = MockLanguageModel::new()
            .with_error(AgentError::LlmError("boom".into()));

        let result = mock.complete(&[], &[], &ModelConfig::default()).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("boom"));
    }

    #[tokio::test]
    async fn test_mock_no_response_returns_error() {
        let mock = MockLanguageModel::new();
        let result = mock.complete(&[], &[], &ModelConfig::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_model_id() {
        let mock = MockLanguageModel::new();
        assert_eq!(mock.model_id(), "mock-language-model");
    }
}
