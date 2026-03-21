use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use srow_core::domain::message::*;
use srow_core::error::EngineError;
use srow_core::ports::llm_provider::*;
use srow_core::ui_message_stream::FinishReason;

use srow_ai::generate::*;

// ---------------------------------------------------------------------------
// Mock LLM Provider with streaming support
// ---------------------------------------------------------------------------

struct MockStreamingProvider {
    responses: Vec<LLMResponse>,
    call_count: AtomicUsize,
}

impl MockStreamingProvider {
    fn new(responses: Vec<LLMResponse>) -> Self {
        Self {
            responses,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LLMProvider for MockStreamingProvider {
    fn model_id(&self) -> &str {
        "mock-streaming"
    }

    async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse, EngineError> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        if idx < self.responses.len() {
            Ok(self.responses[idx].clone())
        } else {
            Err(EngineError::LLMProvider(
                "No more mock responses".to_string(),
            ))
        }
    }

    async fn complete_stream(
        &self,
        _request: LLMRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        if idx >= self.responses.len() {
            return Err(EngineError::LLMProvider(
                "No more mock responses".to_string(),
            ));
        }
        let response = &self.responses[idx];

        // Send text content as TextDelta chunks
        for content in &response.content {
            if let LLMContent::Text { text } = content {
                let _ = tx.send(StreamChunk::TextDelta(text.clone())).await;
            }
        }

        // Send Done with the full response
        let _ = tx.send(StreamChunk::Done(response.clone())).await;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn text_response(text: &str, input_tokens: u32, output_tokens: u32) -> LLMResponse {
    LLMResponse {
        content: vec![LLMContent::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::EndTurn,
        usage: TokenUsage {
            input_tokens,
            output_tokens,
        },
    }
}

fn make_settings(model: Arc<dyn LLMProvider>) -> CallSettings {
    CallSettings {
        model,
        system: None,
        tools: None,
        max_output_tokens: None,
        temperature: None,
        stop_when: None,
        max_retries: 0,
        workspace: std::path::PathBuf::from("/tmp"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stream_text_emits_chunks() {
    let provider = Arc::new(MockStreamingProvider::new(vec![text_response(
        "Hello world",
        10,
        5,
    )]));
    let settings = make_settings(provider);

    let mut result = stream_text(settings, Prompt::Text("Hi".to_string()));

    // Collect all chunks from the channel
    let mut chunks = Vec::new();
    while let Some(chunk) = result.chunk_rx.recv().await {
        chunks.push(chunk);
    }

    // Verify we have Start, TextDelta, TokenUsage, and Finish chunks
    assert!(
        chunks.iter().any(|c| matches!(c, srow_core::ui_message_stream::UIMessageChunk::Start { .. })),
        "Should contain a Start chunk"
    );
    assert!(
        chunks.iter().any(|c| matches!(c, srow_core::ui_message_stream::UIMessageChunk::TextDelta { .. })),
        "Should contain a TextDelta chunk"
    );
    assert!(
        chunks.iter().any(|c| matches!(c, srow_core::ui_message_stream::UIMessageChunk::Finish { .. })),
        "Should contain a Finish chunk"
    );
}

#[tokio::test]
async fn test_stream_text_final_values() {
    let provider = Arc::new(MockStreamingProvider::new(vec![text_response(
        "Final answer",
        20,
        10,
    )]));
    let settings = make_settings(provider);

    let result = stream_text(settings, Prompt::Text("Question".to_string()));

    // Await the oneshot receivers for final values
    let text = result.text.await.unwrap();
    let steps = result.steps.await.unwrap();
    let total_usage = result.total_usage.await.unwrap();
    let finish_reason = result.finish_reason.await.unwrap();

    assert_eq!(text, "Final answer");
    assert_eq!(steps.len(), 1);
    assert_eq!(total_usage.input_tokens, 20);
    assert_eq!(total_usage.output_tokens, 10);
    assert_eq!(finish_reason, FinishReason::Stop);
}
