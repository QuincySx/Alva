use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use srow_core::domain::message::*;
use srow_core::error::EngineError;
use srow_core::ports::llm_provider::*;
use srow_core::ui_message_stream::FinishReason;

use srow_ai::generate::{Agent, Prompt};

// ---------------------------------------------------------------------------
// Mock LLM Provider (supports both complete and complete_stream)
// ---------------------------------------------------------------------------

struct MockAgentProvider {
    responses: Vec<LLMResponse>,
    call_count: AtomicUsize,
}

impl MockAgentProvider {
    fn new(responses: Vec<LLMResponse>) -> Self {
        Self {
            responses,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LLMProvider for MockAgentProvider {
    fn model_id(&self) -> &str {
        "mock-agent"
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
            ..Default::default()
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_generate_simple() {
    let provider = Arc::new(MockAgentProvider::new(vec![text_response(
        "Hello from agent",
        15,
        8,
    )]));

    let agent = Agent::new(provider)
        .with_instructions("You are a helpful assistant.")
        .with_workspace(std::path::PathBuf::from("/tmp"));

    let result = agent
        .generate(Prompt::Text("hi".to_string()))
        .await
        .unwrap();

    assert_eq!(result.text, "Hello from agent");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason, FinishReason::Stop);
    assert_eq!(result.usage.input_tokens, 15);
    assert_eq!(result.usage.output_tokens, 8);
}

#[tokio::test]
async fn test_agent_stream_simple() {
    let provider = Arc::new(MockAgentProvider::new(vec![text_response(
        "Streaming from agent",
        25,
        12,
    )]));

    let agent = Agent::new(provider)
        .with_instructions("You are a helpful assistant.")
        .with_workspace(std::path::PathBuf::from("/tmp"));

    let mut result = agent.stream(Prompt::Text("hi".to_string()));

    // Collect all chunks
    let mut chunks = Vec::new();
    while let Some(chunk) = result.chunk_rx.recv().await {
        chunks.push(chunk);
    }

    // Verify key chunks are present
    assert!(
        chunks.iter().any(|c| matches!(
            c,
            srow_core::ui_message_stream::UIMessageChunk::Start { .. }
        )),
        "Should contain a Start chunk"
    );
    assert!(
        chunks.iter().any(|c| matches!(
            c,
            srow_core::ui_message_stream::UIMessageChunk::TextDelta { .. }
        )),
        "Should contain a TextDelta chunk"
    );
    assert!(
        chunks.iter().any(|c| matches!(
            c,
            srow_core::ui_message_stream::UIMessageChunk::Finish { .. }
        )),
        "Should contain a Finish chunk"
    );

    // Verify the TextDelta contains the expected text
    let text_deltas: Vec<String> = chunks
        .iter()
        .filter_map(|c| match c {
            srow_core::ui_message_stream::UIMessageChunk::TextDelta { delta, .. } => {
                Some(delta.clone())
            }
            _ => None,
        })
        .collect();
    let full_text = text_deltas.join("");
    assert_eq!(full_text, "Streaming from agent");
}
