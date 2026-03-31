//! End-to-end tests for the full agent pipeline.
//!
//! Tests the complete chain: prompt -> LLM -> tool call -> tool execution ->
//! LLM response -> events, using MockLanguageModel (no real API calls).
//!
//! These tests exercise BaseAgent as a black box: build an agent, send a
//! prompt, collect events, and assert the full lifecycle occurred correctly.

use std::sync::Arc;

use alva_app_core::base_agent::{BaseAgent, PermissionMode};
use alva_app_core::AgentEvent;
use alva_test::fixtures::{make_assistant_message, make_tool_call_message};
use alva_test::mock_provider::MockLanguageModel;
use alva_test::mock_tool::MockTool;
use alva_types::{ContentBlock, Message, MessageRole, StreamEvent, ToolOutput, UsageMetadata};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal BaseAgent with a mock model, no browser, temp workspace.
async fn build_agent(model: Arc<dyn alva_types::LanguageModel>) -> (BaseAgent, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("You are a test assistant.")
        .without_browser()
        .build(model)
        .await
        .expect("build should succeed");
    (agent, tmp)
}

/// Drain events from an agent prompt, returning all collected events.
async fn collect_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        let is_end = matches!(event, AgentEvent::AgentEnd { .. });
        events.push(event);
        if is_end {
            break;
        }
    }
    events
}

// ---------------------------------------------------------------------------
// Test: Simple text prompt produces full event lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_simple_prompt_produces_streaming_events() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("Hello! I can help you with coding.")),
    );

    let (agent, _tmp) = build_agent(model).await;
    let rx = agent.prompt_text("Hi there!");
    let events = collect_events(rx).await;

    // Check lifecycle events are present
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentStart)),
        "should receive AgentStart"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::MessageStart { .. })),
        "should receive MessageStart"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::MessageEnd { .. })),
        "should receive MessageEnd"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "should receive AgentEnd with no error"
    );

    // Check that streaming text deltas were emitted
    let mut streamed_text = String::new();
    for event in &events {
        if let AgentEvent::MessageUpdate {
            delta: StreamEvent::TextDelta { text },
            ..
        } = event
        {
            streamed_text.push_str(text);
        }
    }
    assert!(
        streamed_text.contains("Hello"),
        "streamed text should contain model response, got: '{}'",
        streamed_text
    );
}

// ---------------------------------------------------------------------------
// Test: Tool call chain — LLM requests tool -> tool executes -> LLM responds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_tool_call_chain() {
    // First LLM response: request a tool call
    let tool_call_resp = make_tool_call_message("my_test_tool", serde_json::json!({"key": "value"}));

    // Second LLM response: final answer after receiving tool result
    let final_resp = make_assistant_message("Based on the tool output, here is the answer.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_call_resp)
            .with_response(final_resp),
    );

    let mock_tool = MockTool::new("my_test_tool").with_result(ToolOutput::text("tool result data"));
    let mock_tool_clone = mock_tool.clone();

    let tmp = tempfile::tempdir().expect("tempdir");
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test agent.")
        .without_browser()
        .tool(Box::new(mock_tool))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Use the tool please.");
    let events = collect_events(rx).await;

    // Verify tool execution events
    let got_tool_start = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolExecutionStart { tool_call } if tool_call.name == "my_test_tool"));
    assert!(got_tool_start, "should see ToolExecutionStart for my_test_tool");

    let got_tool_end = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, result }
            if tool_call.name == "my_test_tool" && !result.is_error && result.model_text() == "tool result data")
    });
    assert!(got_tool_end, "should see ToolExecutionEnd with correct result");

    // Verify two MessageEnd events (one for tool call response, one for final response)
    let message_end_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::MessageEnd { .. }))
        .count();
    assert_eq!(
        message_end_count, 2,
        "should have 2 MessageEnd events (tool call + final)"
    );

    // Verify final text contains expected content
    let mut final_text = String::new();
    for event in &events {
        if let AgentEvent::MessageUpdate {
            delta: StreamEvent::TextDelta { text },
            ..
        } = event
        {
            final_text.push_str(text);
        }
    }
    assert!(
        final_text.contains("answer"),
        "final streamed text should contain 'answer', got: '{}'",
        final_text
    );

    // Verify the mock tool was actually called
    let calls = mock_tool_clone.calls();
    assert_eq!(calls.len(), 1, "tool should have been called exactly once");
    assert_eq!(calls[0], serde_json::json!({"key": "value"}));

    // Verify AgentEnd has no error
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })));
}

// ---------------------------------------------------------------------------
// Test: Follow-up message continues after natural stop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_follow_up_continues() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("First answer."))
            .with_response(make_assistant_message("Follow-up answer.")),
    );

    let (agent, _tmp) = build_agent(model).await;

    // Queue follow-up BEFORE prompting — it will be consumed after the first
    // natural stop (no tool calls).
    agent.follow_up("Now do this additional thing.");

    let rx = agent.prompt_text("Do something.");
    let events = collect_events(rx).await;

    // Should have 2 MessageEnd events: first response + follow-up response
    let message_end_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::MessageEnd { .. }))
        .count();
    assert_eq!(
        message_end_count, 2,
        "should process both initial and follow-up, got {} MessageEnd events",
        message_end_count
    );

    // Verify session contains follow-up messages
    let messages = agent.messages().await;
    assert!(
        messages.len() >= 4,
        "should have >= 4 messages (user, assistant, follow-up user, follow-up assistant), got {}",
        messages.len()
    );

    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })));
}

// ---------------------------------------------------------------------------
// Test: Steering message mid-turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_steering_mid_turn() {
    // First response: model requests a tool call
    let tool_resp = make_tool_call_message("my_helper", serde_json::json!({}));
    // Second response: model acknowledges steering
    let steering_resp = make_assistant_message("OK, I changed my approach as requested.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_resp)
            .with_response(steering_resp),
    );

    let mock_tool = MockTool::new("my_helper").with_result(ToolOutput::text("done"));

    let tmp = tempfile::tempdir().expect("tempdir");
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test.")
        .without_browser()
        .tool(Box::new(mock_tool))
        .build(model)
        .await
        .expect("build");

    // Queue a steering message BEFORE prompting; it will be consumed after
    // tool execution completes, before the next LLM call.
    agent.steer("Actually, use a different approach.");

    let rx = agent.prompt_text("Do something.");
    let events = collect_events(rx).await;

    // Should have completed without error
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error"
    );

    // Session should contain: user msg, tool-call response, tool result,
    // steering msg (normalized to Standard), steering response.
    let messages = agent.messages().await;
    assert!(
        messages.len() >= 4,
        "should have multiple messages including steering, got {}",
        messages.len()
    );
}

// ---------------------------------------------------------------------------
// Test: Plan mode blocks write tools
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_mode_blocks_writes() {
    // Model requests a write tool
    let tool_resp = Message {
        id: "msg-plan".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_plan_1".to_string(),
            name: "create_file".to_string(),
            input: serde_json::json!({"path": "test.txt", "content": "hello"}),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("I see the file creation was blocked.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_resp)
            .with_response(final_resp),
    );

    let (agent, _tmp) = build_agent(model).await;
    agent.set_permission_mode(PermissionMode::Plan);

    let rx = agent.prompt_text("Create a file.");
    let events = collect_events(rx).await;

    // The tool execution should have been blocked
    let tool_was_blocked = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { result, .. }
            if result.is_error && result.model_text().contains("blocked"))
    });
    assert!(
        tool_was_blocked,
        "create_file should be blocked in plan mode"
    );

    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })));
}

// ---------------------------------------------------------------------------
// Test: Cancel stops the agent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_cancel_stops_agent() {
    // Model returns a response — but we will cancel before or during processing
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("This should not fully complete.")),
    );

    let (agent, _tmp) = build_agent(model).await;

    // Cancel immediately — even before the spawned task starts the agent loop,
    // the cancellation token will be checked on the first iteration.
    agent.cancel();

    let rx = agent.prompt_text("Run something.");
    let events = collect_events(rx).await;

    // Should receive AgentEnd (possibly with a cancellation error)
    let got_end = events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. }));
    assert!(got_end, "agent should end after cancel");
}

// ---------------------------------------------------------------------------
// Test: Usage metadata flows through events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_usage_metadata_in_events() {
    let response = Message {
        id: "msg-usage".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::Text {
            text: "Here is the answer.".to_string(),
        }],
        tool_call_id: None,
        usage: Some(UsageMetadata {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        }),
        timestamp: 0,
    };

    let model = Arc::new(MockLanguageModel::new().with_response(response));

    let (agent, _tmp) = build_agent(model).await;
    let rx = agent.prompt_text("What is the answer?");
    let events = collect_events(rx).await;

    // Check that usage metadata was emitted as a StreamEvent::Usage
    let got_usage = events.iter().any(|e| {
        matches!(e, AgentEvent::MessageUpdate { delta: StreamEvent::Usage(u), .. }
            if u.total_tokens == 150)
    });
    assert!(got_usage, "should receive usage metadata in stream events");
}

// ---------------------------------------------------------------------------
// Test: Multiple tool calls in single response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_multiple_tool_calls_in_single_response() {
    // LLM returns two tool calls in one message
    let multi_tool_resp = Message {
        id: "msg-multi".to_string(),
        role: MessageRole::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "Let me use both tools.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_a".to_string(),
                name: "tool_alpha".to_string(),
                input: serde_json::json!({"x": 1}),
            },
            ContentBlock::ToolUse {
                id: "call_b".to_string(),
                name: "tool_beta".to_string(),
                input: serde_json::json!({"y": 2}),
            },
        ],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("Both tools completed successfully.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(multi_tool_resp)
            .with_response(final_resp),
    );

    let tool_alpha = MockTool::new("tool_alpha").with_result(ToolOutput::text("alpha result"));
    let tool_beta = MockTool::new("tool_beta").with_result(ToolOutput::text("beta result"));
    let tool_alpha_clone = tool_alpha.clone();
    let tool_beta_clone = tool_beta.clone();

    let tmp = tempfile::tempdir().expect("tempdir");
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test.")
        .without_browser()
        .tool(Box::new(tool_alpha))
        .tool(Box::new(tool_beta))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Use both tools.");
    let events = collect_events(rx).await;

    // Both tools should have ToolExecutionStart and ToolExecutionEnd
    let tool_start_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }))
        .count();
    assert_eq!(tool_start_count, 2, "should have 2 ToolExecutionStart events");

    let tool_end_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }))
        .count();
    assert_eq!(tool_end_count, 2, "should have 2 ToolExecutionEnd events");

    // Both tools should have been called
    assert_eq!(tool_alpha_clone.calls().len(), 1, "tool_alpha called once");
    assert_eq!(tool_beta_clone.calls().len(), 1, "tool_beta called once");

    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })));
}

// ---------------------------------------------------------------------------
// Test: Session persistence across prompts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_session_persists_across_prompts() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("First response."))
            .with_response(make_assistant_message("Second response.")),
    );

    let (agent, _tmp) = build_agent(model).await;

    // First prompt
    let rx1 = agent.prompt_text("First message.");
    let _ = collect_events(rx1).await;

    let messages_after_first = agent.messages().await;
    assert_eq!(
        messages_after_first.len(),
        2,
        "should have user + assistant after first prompt"
    );

    // Second prompt — session should accumulate
    let rx2 = agent.prompt_text("Second message.");
    let _ = collect_events(rx2).await;

    let messages_after_second = agent.messages().await;
    assert_eq!(
        messages_after_second.len(),
        4,
        "should have 4 messages after both prompts (user + assistant + user + assistant)"
    );
}

// ---------------------------------------------------------------------------
// Test: new_session clears history
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_new_session_clears_history() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("Response."))
            .with_response(make_assistant_message("After clear.")),
    );

    let (agent, _tmp) = build_agent(model).await;

    // First prompt
    let rx = agent.prompt_text("Hello.");
    let _ = collect_events(rx).await;
    assert_eq!(agent.messages().await.len(), 2);

    // Clear session
    agent.new_session().await;
    assert_eq!(
        agent.messages().await.len(),
        0,
        "session should be empty after new_session"
    );

    // Second prompt — fresh session
    let rx2 = agent.prompt_text("Fresh start.");
    let _ = collect_events(rx2).await;
    assert_eq!(
        agent.messages().await.len(),
        2,
        "should have exactly 2 messages in fresh session"
    );
}

// ---------------------------------------------------------------------------
// Test: LLM error is propagated as AgentEnd error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_llm_error_propagated() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_error(alva_types::AgentError::LlmError("model exploded".into())),
    );

    let (agent, _tmp) = build_agent(model).await;
    let rx = agent.prompt_text("This will fail.");
    let events = collect_events(rx).await;

    let has_error = events.iter().any(|e| {
        matches!(e, AgentEvent::AgentEnd { error: Some(msg) } if msg.contains("model exploded"))
    });
    assert!(
        has_error,
        "AgentEnd should contain the LLM error message"
    );
}
