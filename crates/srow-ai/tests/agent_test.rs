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

use srow_ai::generate::{Agent, Prompt};

// Use a type alias to avoid confusion with ui_message_stream::FinishReason
use srow_core::ports::provider::language_model::FinishReason as FinishReasonV4;

// ---------------------------------------------------------------------------
// Mock Language Model (supports both do_generate and do_stream)
// ---------------------------------------------------------------------------

struct MockAgentModel {
    responses: Vec<MockResponse>,
    call_count: AtomicUsize,
}

struct MockResponse {
    text: String,
    input_tokens: u32,
    output_tokens: u32,
}

impl MockAgentModel {
    fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LanguageModel for MockAgentModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "mock-agent"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError> {
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
        Ok(LanguageModelGenerateResult {
            content: vec![LanguageModelContent::Text {
                text: resp.text.clone(),
                provider_metadata: None,
            }],
            finish_reason: FinishReasonV4 {
                unified: UnifiedFinishReason::Stop,
                raw: None,
            },
            usage: LanguageModelUsage {
                input_tokens: UsageInputTokens {
                    total: Some(resp.input_tokens),
                    ..Default::default()
                },
                output_tokens: UsageOutputTokens {
                    total: Some(resp.output_tokens),
                    ..Default::default()
                },
                raw: None,
            },
            provider_metadata: None,
            warnings: Vec::new(),
            response: None,
        })
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

        let parts = vec![
            LanguageModelStreamPart::TextStart {
                id: "t0".to_string(),
            },
            LanguageModelStreamPart::TextDelta {
                id: "t0".to_string(),
                delta: resp.text.clone(),
            },
            LanguageModelStreamPart::TextEnd {
                id: "t0".to_string(),
            },
            LanguageModelStreamPart::Finish {
                usage: LanguageModelUsage {
                    input_tokens: UsageInputTokens {
                        total: Some(resp.input_tokens),
                        ..Default::default()
                    },
                    output_tokens: UsageOutputTokens {
                        total: Some(resp.output_tokens),
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
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_generate_simple() {
    let model = Arc::new(MockAgentModel::new(vec![MockResponse {
        text: "Hello from agent".to_string(),
        input_tokens: 15,
        output_tokens: 8,
    }]));

    let agent = Agent::new(model)
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
    let model = Arc::new(MockAgentModel::new(vec![MockResponse {
        text: "Streaming from agent".to_string(),
        input_tokens: 25,
        output_tokens: 12,
    }]));

    let agent = Agent::new(model)
        .with_instructions("You are a helpful assistant.")
        .with_workspace(std::path::PathBuf::from("/tmp"));

    let mut result = agent.stream(Prompt::Text("hi".to_string()));

    let mut chunks = Vec::new();
    while let Some(chunk) = result.chunk_rx.recv().await {
        chunks.push(chunk);
    }

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
