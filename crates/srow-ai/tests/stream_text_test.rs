use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, stream};

use srow_core::domain::message::*;
use srow_core::ports::provider::language_model::*;
use srow_core::ports::provider::content::LanguageModelContent;
use srow_core::ports::provider::errors::ProviderError;
use srow_core::ui_message_stream::FinishReason;

use srow_ai::generate::*;

// Use a type alias to avoid confusion with ui_message_stream::FinishReason
use srow_core::ports::provider::language_model::FinishReason as FinishReasonV4;

// ---------------------------------------------------------------------------
// Mock Language Model with streaming support
// ---------------------------------------------------------------------------

struct MockStreamingModel {
    responses: Vec<MockStreamResponse>,
    call_count: AtomicUsize,
}

struct MockStreamResponse {
    text: String,
    input_tokens: u32,
    output_tokens: u32,
}

impl MockStreamingModel {
    fn new(responses: Vec<MockStreamResponse>) -> Self {
        Self {
            responses,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LanguageModel for MockStreamingModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "mock-streaming"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError> {
        unimplemented!("Not needed for stream tests — use do_stream")
    }

    async fn do_stream(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, ProviderError> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        if idx >= self.responses.len() {
            return Err(ProviderError::ApiCall {
                message: "No more mock responses".to_string(),
                url: String::new(),
                status_code: None,
                response_body: None,
                is_retryable: false,
            });
        }
        let resp = &self.responses[idx];

        let text = resp.text.clone();
        let input_tokens = resp.input_tokens;
        let output_tokens = resp.output_tokens;

        let parts = vec![
            LanguageModelStreamPart::TextStart {
                id: "t0".to_string(),
            },
            LanguageModelStreamPart::TextDelta {
                id: "t0".to_string(),
                delta: text,
            },
            LanguageModelStreamPart::TextEnd {
                id: "t0".to_string(),
            },
            LanguageModelStreamPart::Finish {
                usage: LanguageModelUsage {
                    input_tokens: UsageInputTokens {
                        total: Some(input_tokens),
                        ..Default::default()
                    },
                    output_tokens: UsageOutputTokens {
                        total: Some(output_tokens),
                        ..Default::default()
                    },
                    raw: None,
                },
                finish_reason: FinishReasonV4 {
                    unified: UnifiedFinishReason::Stop,
                    raw: None,
                },
                provider_metadata: None,
            },
        ];

        Ok(LanguageModelStreamResult {
            stream: Box::pin(stream::iter(parts)),
            response: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_settings(model: Arc<dyn LanguageModel>) -> CallSettings {
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
    let model = Arc::new(MockStreamingModel::new(vec![MockStreamResponse {
        text: "Hello world".to_string(),
        input_tokens: 10,
        output_tokens: 5,
    }]));
    let settings = make_settings(model);

    let mut result = stream_text(settings, Prompt::Text("Hi".to_string()));

    let mut chunks = Vec::new();
    while let Some(chunk) = result.chunk_rx.recv().await {
        chunks.push(chunk);
    }

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
    let model = Arc::new(MockStreamingModel::new(vec![MockStreamResponse {
        text: "Final answer".to_string(),
        input_tokens: 20,
        output_tokens: 10,
    }]));
    let settings = make_settings(model);

    let result = stream_text(settings, Prompt::Text("Question".to_string()));

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
