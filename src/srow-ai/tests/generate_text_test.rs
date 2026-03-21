use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use srow_core::domain::message::*;
use srow_core::domain::tool::*;
use srow_core::error::EngineError;
use srow_core::ports::llm_provider::*;
use srow_core::ports::tool::ToolRegistry;
use srow_core::ui_message_stream::FinishReason;

use srow_ai::generate::*;

// ---------------------------------------------------------------------------
// Mock LLM Provider
// ---------------------------------------------------------------------------

struct MockLLMProvider {
    responses: Vec<LLMResponse>,
    call_count: AtomicUsize,
}

impl MockLLMProvider {
    fn new(responses: Vec<LLMResponse>) -> Self {
        Self {
            responses,
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LLMProvider for MockLLMProvider {
    fn model_id(&self) -> &str {
        "mock"
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
        _tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError> {
        unimplemented!("Not needed for generate_text tests")
    }
}

// ---------------------------------------------------------------------------
// Failing Mock LLM Provider (for retry test)
// ---------------------------------------------------------------------------

struct FailThenSucceedProvider {
    fail_count: AtomicUsize,
    failures_before_success: usize,
    success_response: LLMResponse,
}

impl FailThenSucceedProvider {
    fn new(failures_before_success: usize, success_response: LLMResponse) -> Self {
        Self {
            fail_count: AtomicUsize::new(0),
            failures_before_success,
            success_response,
        }
    }
}

#[async_trait]
impl LLMProvider for FailThenSucceedProvider {
    fn model_id(&self) -> &str {
        "mock-fail-then-succeed"
    }

    async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse, EngineError> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        if count < self.failures_before_success {
            Err(EngineError::LLMProvider("Transient error".to_string()))
        } else {
            Ok(self.success_response.clone())
        }
    }

    async fn complete_stream(
        &self,
        _request: LLMRequest,
        _tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError> {
        unimplemented!("Not needed for generate_text tests")
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn tool_use_response(
    tool_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    input_tokens: u32,
    output_tokens: u32,
) -> LLMResponse {
    LLMResponse {
        content: vec![LLMContent::ToolUse {
            id: tool_id.to_string(),
            name: tool_name.to_string(),
            input,
        }],
        stop_reason: StopReason::ToolUse,
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
async fn test_generate_text_simple() {
    let provider = Arc::new(MockLLMProvider::new(vec![text_response("Hello", 10, 5)]));
    let settings = make_settings(provider);

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
    // Step 1: model returns tool use
    // Step 2: model returns text "Done"
    let provider = Arc::new(MockLLMProvider::new(vec![
        tool_use_response(
            "tc_1",
            "search",
            serde_json::json!({"query": "test"}),
            20,
            10,
        ),
        text_response("Done", 30, 15),
    ]));

    // Register an empty tool registry (no tools registered, so execution returns error)
    let tools = Arc::new(ToolRegistry::new());

    let mut settings = make_settings(provider);
    settings.tools = Some(tools);

    let result = generate_text(settings, Prompt::Text("Do something".to_string()))
        .await
        .unwrap();

    // Should have 2 steps: tool call step + final text step
    assert_eq!(result.steps.len(), 2);
    assert_eq!(result.text, "Done");
    assert_eq!(result.finish_reason, FinishReason::Stop);

    // First step should have a tool call and an error tool result (tool not found)
    let step1 = &result.steps[0];
    assert_eq!(step1.tool_calls.len(), 1);
    assert_eq!(step1.tool_calls[0].name, "search");
    assert_eq!(step1.tool_results.len(), 1);
    assert!(step1.tool_results[0].is_error);
    assert!(step1.tool_results[0].output.contains("not found"));

    // Second step should have text
    let step2 = &result.steps[1];
    assert_eq!(step2.text, "Done");
}

#[tokio::test]
async fn test_generate_text_stop_condition() {
    // Model wants to call tools, but stop_when limits to 1 step
    let provider = Arc::new(MockLLMProvider::new(vec![
        tool_use_response(
            "tc_1",
            "search",
            serde_json::json!({"query": "test"}),
            20,
            10,
        ),
        // This response should never be reached
        text_response("Should not reach", 30, 15),
    ]));

    let tools = Arc::new(ToolRegistry::new());

    let mut settings = make_settings(provider);
    settings.tools = Some(tools);
    settings.stop_when = Some(step_count_is(1));

    let result = generate_text(settings, Prompt::Text("Do something".to_string()))
        .await
        .unwrap();

    // Should have only 1 step because stop_when kicked in
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason, FinishReason::ToolCalls);
}

#[tokio::test]
async fn test_generate_text_total_usage() {
    // Two steps with different usage
    let provider = Arc::new(MockLLMProvider::new(vec![
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

    let mut settings = make_settings(provider);
    settings.tools = Some(tools);

    let result = generate_text(settings, Prompt::Text("Read file".to_string()))
        .await
        .unwrap();

    assert_eq!(result.steps.len(), 2);

    // total_usage should be sum of all steps
    assert_eq!(result.total_usage.input_tokens, 300); // 100 + 200
    assert_eq!(result.total_usage.output_tokens, 125); // 50 + 75

    // usage (last step) should be that step's usage
    assert_eq!(result.usage.input_tokens, 200);
    assert_eq!(result.usage.output_tokens, 75);
}

#[tokio::test]
async fn test_generate_text_retry() {
    // Provider fails once then succeeds. With max_retries=1, it should work.
    let provider = Arc::new(FailThenSucceedProvider::new(
        1, // fail once
        text_response("Recovered", 10, 5),
    ));

    let mut settings = make_settings(provider);
    settings.max_retries = 1;

    let result = generate_text(settings, Prompt::Text("Test retry".to_string()))
        .await
        .unwrap();

    assert_eq!(result.text, "Recovered");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason, FinishReason::Stop);
}
