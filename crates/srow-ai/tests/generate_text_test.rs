use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use srow_core::domain::message::*;
use srow_core::domain::tool::*;
use srow_core::ports::provider::language_model::*;
use srow_core::ports::provider::content::LanguageModelContent;
use srow_core::ports::provider::errors::ProviderError;
use srow_core::ports::tool::ToolRegistry;
use srow_core::ui_message_stream::FinishReason;

use srow_ai::generate::*;

// ---------------------------------------------------------------------------
// Mock Language Model
// ---------------------------------------------------------------------------

struct MockLanguageModel {
    responses: Vec<LanguageModelGenerateResult>,
    call_count: AtomicUsize,
}

impl MockLanguageModel {
    fn new(responses: Vec<LanguageModelGenerateResult>) -> Self {
        Self {
            responses,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LanguageModel for MockLanguageModel {
    fn provider(&self) -> &str {
        "mock"
    }

    fn model_id(&self) -> &str {
        "mock"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        if idx < self.responses.len() {
            // Clone the response manually since LanguageModelGenerateResult doesn't derive Clone
            let resp = &self.responses[idx];
            Ok(LanguageModelGenerateResult {
                content: resp.content.clone(),
                finish_reason: resp.finish_reason.clone(),
                usage: resp.usage.clone(),
                provider_metadata: None,
                warnings: Vec::new(),
                response: None,
            })
        } else {
            Err(ProviderError::ApiCall {
                message: "No more mock responses".to_string(),
                url: String::new(),
                status_code: None,
                response_body: None,
                is_retryable: false,
            })
        }
    }

    async fn do_stream(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, ProviderError> {
        unimplemented!("Not needed for generate_text tests")
    }
}

// ---------------------------------------------------------------------------
// Failing Mock (for retry test)
// ---------------------------------------------------------------------------

struct FailThenSucceedModel {
    fail_count: AtomicUsize,
    failures_before_success: usize,
    success_response: LanguageModelGenerateResult,
}

impl FailThenSucceedModel {
    fn new(failures_before_success: usize, success_response: LanguageModelGenerateResult) -> Self {
        Self {
            fail_count: AtomicUsize::new(0),
            failures_before_success,
            success_response,
        }
    }
}

#[async_trait]
impl LanguageModel for FailThenSucceedModel {
    fn provider(&self) -> &str {
        "mock-fail"
    }

    fn model_id(&self) -> &str {
        "mock-fail-then-succeed"
    }

    async fn do_generate(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        if count < self.failures_before_success {
            Err(ProviderError::ApiCall {
                message: "Transient error".to_string(),
                url: String::new(),
                status_code: None,
                response_body: None,
                is_retryable: true,
            })
        } else {
            let resp = &self.success_response;
            Ok(LanguageModelGenerateResult {
                content: resp.content.clone(),
                finish_reason: resp.finish_reason.clone(),
                usage: resp.usage.clone(),
                provider_metadata: None,
                warnings: Vec::new(),
                response: None,
            })
        }
    }

    async fn do_stream(
        &self,
        _options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, ProviderError> {
        unimplemented!("Not needed for generate_text tests")
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

fn text_response(text: &str, input_tokens: u32, output_tokens: u32) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult {
        content: vec![LanguageModelContent::Text {
            text: text.to_string(),
            provider_metadata: None,
        }],
        finish_reason: FinishReasonV4 {
            unified: UnifiedFinishReason::Stop,
            raw: None,
        },
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
        provider_metadata: None,
        warnings: Vec::new(),
        response: None,
    }
}

fn tool_use_response(
    tool_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    input_tokens: u32,
    output_tokens: u32,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult {
        content: vec![LanguageModelContent::ToolCall {
            tool_call_id: tool_id.to_string(),
            tool_name: tool_name.to_string(),
            input: serde_json::to_string(&input).unwrap_or_default(),
            provider_metadata: None,
        }],
        finish_reason: FinishReasonV4 {
            unified: UnifiedFinishReason::ToolCalls,
            raw: None,
        },
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
        provider_metadata: None,
        warnings: Vec::new(),
        response: None,
    }
}

// Use a type alias to avoid confusion with ui_message_stream::FinishReason
use srow_core::ports::provider::language_model::FinishReason as FinishReasonV4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_generate_text_simple() {
    let model = Arc::new(MockLanguageModel::new(vec![text_response("Hello", 10, 5)]));
    let settings = make_settings(model);

    let result = generate_text(settings, Prompt::Text("Hi".to_string()))
        .await
        .unwrap();

    assert_eq!(result.text, "Hello");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason, FinishReason::Stop);
    assert_eq!(result.usage.input_tokens, 10);
    assert_eq!(result.usage.output_tokens, 5);
}

#[tokio::test]
async fn test_generate_text_with_tool_call() {
    let model = Arc::new(MockLanguageModel::new(vec![
        tool_use_response(
            "tc_1",
            "search",
            serde_json::json!({"query": "test"}),
            20,
            10,
        ),
        text_response("Done", 30, 15),
    ]));

    let tools = Arc::new(ToolRegistry::new());

    let mut settings = make_settings(model);
    settings.tools = Some(tools);

    let result = generate_text(settings, Prompt::Text("Do something".to_string()))
        .await
        .unwrap();

    assert_eq!(result.steps.len(), 2);
    assert_eq!(result.text, "Done");
    assert_eq!(result.finish_reason, FinishReason::Stop);

    let step1 = &result.steps[0];
    assert_eq!(step1.tool_calls.len(), 1);
    assert_eq!(step1.tool_calls[0].name, "search");
    assert_eq!(step1.tool_results.len(), 1);
    assert!(step1.tool_results[0].is_error);
    assert!(step1.tool_results[0].output.contains("not found"));

    let step2 = &result.steps[1];
    assert_eq!(step2.text, "Done");
}

#[tokio::test]
async fn test_generate_text_stop_condition() {
    let model = Arc::new(MockLanguageModel::new(vec![
        tool_use_response(
            "tc_1",
            "search",
            serde_json::json!({"query": "test"}),
            20,
            10,
        ),
        text_response("Should not reach", 30, 15),
    ]));

    let tools = Arc::new(ToolRegistry::new());

    let mut settings = make_settings(model);
    settings.tools = Some(tools);
    settings.stop_when = Some(step_count_is(1));

    let result = generate_text(settings, Prompt::Text("Do something".to_string()))
        .await
        .unwrap();

    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason, FinishReason::ToolCalls);
}

#[tokio::test]
async fn test_generate_text_total_usage() {
    let model = Arc::new(MockLanguageModel::new(vec![
        tool_use_response(
            "tc_1",
            "read_file",
            serde_json::json!({"path": "/tmp/test"}),
            100,
            50,
        ),
        text_response("Final answer", 200, 75),
    ]));

    let tools = Arc::new(ToolRegistry::new());

    let mut settings = make_settings(model);
    settings.tools = Some(tools);

    let result = generate_text(settings, Prompt::Text("Read file".to_string()))
        .await
        .unwrap();

    assert_eq!(result.steps.len(), 2);
    assert_eq!(result.total_usage.input_tokens, 300);
    assert_eq!(result.total_usage.output_tokens, 125);
    assert_eq!(result.usage.input_tokens, 200);
    assert_eq!(result.usage.output_tokens, 75);
}

#[tokio::test]
async fn test_generate_text_retry() {
    let model = Arc::new(FailThenSucceedModel::new(
        1,
        text_response("Recovered", 10, 5),
    ));

    let mut settings = make_settings(model);
    settings.max_retries = 1;

    let result = generate_text(settings, Prompt::Text("Test retry".to_string()))
        .await
        .unwrap();

    assert_eq!(result.text, "Recovered");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason, FinishReason::Stop);
}
