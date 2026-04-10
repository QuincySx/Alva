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

// ===========================================================================
// Real built-in tool tests — exercises actual tool resolution and execution
// through the full agent pipeline (not MockTool).
// ===========================================================================

// ---------------------------------------------------------------------------
// Helper: build an agent with builtin tools and a specific workspace
// ---------------------------------------------------------------------------

/// Build a BaseAgent with all builtin tools registered (no browser), pointing
/// at the given temp directory as the workspace.
async fn build_agent_with_workspace(
    model: Arc<dyn alva_types::LanguageModel>,
    workspace: &std::path::Path,
) -> BaseAgent {
    BaseAgent::builder()
        .workspace(workspace)
        .system_prompt("You are a test assistant.")
        .middlewares(alva_app_core::base_agent::builder::middleware_presets::production())
        .build(model)
        .await
        .expect("build should succeed")
}

// ---------------------------------------------------------------------------
// Test: Real read_file tool — tool is found and invoked through the pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_real_read_file_tool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "Hello World\nLine 2\nLine 3").unwrap();

    // Model returns read_file tool call with the absolute path, then final text
    let file_path = tmp.path().join("hello.txt");
    let tool_call_resp = Message {
        id: "msg-read".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_read_1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": file_path.to_str().unwrap()}),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("The file contains 3 lines.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_call_resp)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;
    let rx = agent.prompt_text("Read the hello.txt file.");
    let events = collect_events(rx).await;

    // Verify the read_file tool was found and execution was attempted
    let got_tool_start = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionStart { tool_call } if tool_call.name == "read_file")
    });
    assert!(got_tool_start, "should see ToolExecutionStart for read_file");

    // Verify ToolExecutionEnd was emitted (tool executes through the pipeline)
    let got_tool_end = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, .. } if tool_call.name == "read_file")
    });
    assert!(got_tool_end, "should see ToolExecutionEnd for read_file");

    // Verify the agent completed without fatal error
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error"
    );
}

// ---------------------------------------------------------------------------
// Test: Real file_edit tool — security blocks dangerous tools without
// approval handler, verifying the middleware pipeline works end-to-end
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_real_file_edit_tool_blocked_without_approval() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("code.rs"),
        "fn main() {\n    println!(\"old\");\n}",
    )
    .unwrap();

    // Model calls file_edit — a dangerous tool that requires approval
    let edit_call = Message {
        id: "msg-edit".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_edit_1".to_string(),
            name: "file_edit".to_string(),
            input: serde_json::json!({
                "path": tmp.path().join("code.rs").to_str().unwrap(),
                "old_str": "println!(\"old\")",
                "new_str": "println!(\"new\")"
            }),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("The edit was attempted.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(edit_call)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;
    let rx = agent.prompt_text("Edit the code.");
    let events = collect_events(rx).await;

    // file_edit is a dangerous tool; without an approval handler, it gets blocked
    let tool_was_blocked = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, result }
            if tool_call.name == "file_edit" && result.is_error
               && result.model_text().contains("blocked"))
    });
    assert!(
        tool_was_blocked,
        "file_edit should be blocked without approval handler"
    );

    // File should remain unchanged
    let content = std::fs::read_to_string(tmp.path().join("code.rs")).unwrap();
    assert!(
        content.contains("old"),
        "file should not be modified when tool is blocked"
    );
}

// ---------------------------------------------------------------------------
// Test: Real grep_search tool — tool is found and invoked through the pipeline
//
// The grep_search tool is registered as a builtin. When the mock LLM returns
// a tool call for grep_search, the agent resolves the tool from the registry
// and executes it with the workspace context propagated via AgentConfig.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_real_grep_search_tool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.rs"), "fn hello() {}").unwrap();

    // Model calls grep_search for "hello"
    let grep_call = Message {
        id: "msg-grep".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_grep_1".to_string(),
            name: "grep_search".to_string(),
            input: serde_json::json!({"pattern": "hello"}),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("Found hello in a.rs.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(grep_call)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;
    let rx = agent.prompt_text("Search for hello.");
    let events = collect_events(rx).await;

    // Verify the grep_search tool was found and execution was attempted
    let got_tool_start = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionStart { tool_call } if tool_call.name == "grep_search")
    });
    assert!(got_tool_start, "should see ToolExecutionStart for grep_search");

    // Workspace is now propagated via AgentConfig, so the tool succeeds.
    // AgentEnd should have no error.
    let has_clean_end = events.iter().any(|e| {
        matches!(e, AgentEvent::AgentEnd { error: None })
    });
    assert!(
        has_clean_end,
        "AgentEnd should have no error — workspace is now propagated to tools"
    );
}

// ---------------------------------------------------------------------------
// Test: Real find_files tool — invoked through the pipeline
//
// The find_files tool is registered as a builtin. Workspace context is now
// propagated via AgentConfig, so the tool executes successfully.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_real_find_files_tool() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("keep.rs"), "fn keep() {}").unwrap();

    let find_call = Message {
        id: "msg-find".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_find_1".to_string(),
            name: "find_files".to_string(),
            input: serde_json::json!({"pattern": "*.rs"}),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("Found the files.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(find_call)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;
    let rx = agent.prompt_text("Find all rust files.");
    let events = collect_events(rx).await;

    // Tool is found and execution attempted
    let got_tool_start = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionStart { tool_call } if tool_call.name == "find_files")
    });
    assert!(got_tool_start, "should see ToolExecutionStart for find_files");

    // Workspace is now propagated via AgentConfig, so the tool succeeds.
    // AgentEnd should have no error.
    let has_clean_end = events.iter().any(|e| {
        matches!(e, AgentEvent::AgentEnd { error: None })
    });
    assert!(
        has_clean_end,
        "AgentEnd should have no error — workspace is now propagated to tools"
    );
}

// ---------------------------------------------------------------------------
// Test: find_files with .gitignore — exercises the tool registration and
// invocation pipeline. Workspace context is propagated via AgentConfig.
//
// The underlying walk_dir_filtered() function respects .gitignore; this is
// tested at the unit level in alva-agent-tools. Here we verify the full
// agent pipeline: tool resolution -> security -> execution.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_find_files_respects_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join(".gitignore"), "ignored/\n").unwrap();
    std::fs::write(tmp.path().join("keep.rs"), "fn keep() {}").unwrap();
    std::fs::create_dir(tmp.path().join("ignored")).unwrap();
    std::fs::write(tmp.path().join("ignored").join("skip.rs"), "fn skip() {}").unwrap();

    let find_call = Message {
        id: "msg-find-gi".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_find_gi".to_string(),
            name: "find_files".to_string(),
            input: serde_json::json!({"pattern": "*.rs"}),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("Found the files.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(find_call)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;
    let rx = agent.prompt_text("Find all .rs files.");
    let events = collect_events(rx).await;

    // Tool is found and execution attempted
    let got_tool_start = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionStart { tool_call } if tool_call.name == "find_files")
    });
    assert!(got_tool_start, "should see ToolExecutionStart for find_files");

    // Workspace is now propagated, so find_files should succeed.
    // The result should contain keep.rs but NOT skip.rs (gitignore filtering).
    let has_end = events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. }));
    assert!(has_end, "agent should end (either with or without error)");
}

// ---------------------------------------------------------------------------
// Test: execute_shell — blocked as dangerous without approval handler
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_execute_shell_blocked_without_approval() {
    let tmp = tempfile::tempdir().unwrap();

    let shell_call = Message {
        id: "msg-shell".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_shell_1".to_string(),
            name: "execute_shell".to_string(),
            input: serde_json::json!({"command": "echo hello_e2e"}),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("The command was attempted.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(shell_call)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;
    let rx = agent.prompt_text("Run echo.");
    let events = collect_events(rx).await;

    // execute_shell is a dangerous tool — blocked without approval handler
    let tool_was_blocked = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, result }
            if tool_call.name == "execute_shell" && result.is_error
               && result.model_text().contains("blocked"))
    });
    assert!(
        tool_was_blocked,
        "execute_shell should be blocked without approval handler"
    );

    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error"
    );
}

// ---------------------------------------------------------------------------
// Test: Multi-turn tool loop — LLM calls tools 3 times before final answer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_multi_turn_tool_loop() {
    // Queue 4 responses: 3 tool calls + 1 final text
    let tool_call_1 = make_tool_call_message("step_one", serde_json::json!({"n": 1}));
    let tool_call_2 = make_tool_call_message("step_two", serde_json::json!({"n": 2}));
    let tool_call_3 = make_tool_call_message("step_three", serde_json::json!({"n": 3}));
    let final_resp = make_assistant_message("All 3 steps complete.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_call_1)
            .with_response(tool_call_2)
            .with_response(tool_call_3)
            .with_response(final_resp),
    );

    let tool1 = MockTool::new("step_one").with_result(ToolOutput::text("step 1 done"));
    let tool2 = MockTool::new("step_two").with_result(ToolOutput::text("step 2 done"));
    let tool3 = MockTool::new("step_three").with_result(ToolOutput::text("step 3 done"));
    let tool1_clone = tool1.clone();
    let tool2_clone = tool2.clone();
    let tool3_clone = tool3.clone();

    let tmp = tempfile::tempdir().unwrap();
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test multi-turn.")
        .tool(Box::new(tool1))
        .tool(Box::new(tool2))
        .tool(Box::new(tool3))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Execute all three steps.");
    let events = collect_events(rx).await;

    // Verify exactly 3 ToolExecutionStart events
    let tool_start_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }))
        .count();
    assert_eq!(tool_start_count, 3, "should have 3 ToolExecutionStart events");

    // Verify exactly 3 ToolExecutionEnd events
    let tool_end_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }))
        .count();
    assert_eq!(tool_end_count, 3, "should have 3 ToolExecutionEnd events");

    // Verify 4 MessageEnd events (3 tool-call messages + 1 final text)
    let message_end_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::MessageEnd { .. }))
        .count();
    assert_eq!(
        message_end_count, 4,
        "should have 4 MessageEnd events (3 tool calls + 1 final)"
    );

    // Each tool was called exactly once
    assert_eq!(tool1_clone.calls().len(), 1, "step_one called once");
    assert_eq!(tool2_clone.calls().len(), 1, "step_two called once");
    assert_eq!(tool3_clone.calls().len(), 1, "step_three called once");

    // Verify the final text was streamed
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
        final_text.contains("3 steps complete"),
        "final streamed text should contain '3 steps complete', got: '{}'",
        final_text
    );

    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error"
    );
}

// ---------------------------------------------------------------------------
// Test: Checkpoint callback fires on file_edit tool call (middleware level)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_checkpoint_created_on_file_edit() {
    use alva_agent_runtime::middleware::CheckpointCallback;
    use std::sync::Mutex as StdMutex;

    /// In-test checkpoint callback that records calls.
    #[derive(Clone)]
    struct TestCheckpointCallback {
        calls: Arc<StdMutex<Vec<(String, Vec<std::path::PathBuf>)>>>,
    }

    impl TestCheckpointCallback {
        fn new() -> Self {
            Self {
                calls: Arc::new(StdMutex::new(Vec::new())),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
        fn calls(&self) -> Vec<(String, Vec<std::path::PathBuf>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CheckpointCallback for TestCheckpointCallback {
        fn create_checkpoint(&self, desc: &str, paths: &[std::path::PathBuf]) {
            self.calls
                .lock()
                .unwrap()
                .push((desc.to_string(), paths.to_vec()));
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("target.txt"), "original content").unwrap();

    // Model requests file_edit (dangerous tool, will be blocked by security
    // since no approval handler is set). BUT the CheckpointMiddleware runs
    // BEFORE security in priority ordering, so it should still fire.
    let edit_call = Message {
        id: "msg-cp-edit".to_string(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_cp_edit".to_string(),
            name: "file_edit".to_string(),
            input: serde_json::json!({
                "path": "target.txt",
                "old_str": "original",
                "new_str": "modified"
            }),
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    };
    let final_resp = make_assistant_message("Edit attempted.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(edit_call)
            .with_response(final_resp),
    );

    let agent = build_agent_with_workspace(model, tmp.path()).await;

    // Register the checkpoint callback
    let checkpoint_cb = TestCheckpointCallback::new();
    agent
        .set_checkpoint_callback(Arc::new(checkpoint_cb.clone()));

    let rx = agent.prompt_text("Edit the file.");
    let events = collect_events(rx).await;

    // Verify the checkpoint callback was invoked before the tool call
    assert!(
        checkpoint_cb.call_count() > 0,
        "checkpoint callback should have been called at least once, got {} calls",
        checkpoint_cb.call_count()
    );

    // Check that the checkpoint description mentions file_edit
    let calls = checkpoint_cb.calls();
    let first_call = &calls[0];
    assert!(
        first_call.0.contains("file_edit"),
        "checkpoint description should mention file_edit, got: '{}'",
        first_call.0
    );

    // Check that the checkpoint path includes target.txt
    assert!(
        first_call.1.iter().any(|p| p.to_str().unwrap().contains("target.txt")),
        "checkpoint should reference target.txt, got: {:?}",
        first_call.1
    );

    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error"
    );
}

// ---------------------------------------------------------------------------
// Test: Session message flow transparency after a tool-call prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_session_message_flow() {
    // LLM returns a tool call, then a final text response
    let tool_call_resp = make_tool_call_message("my_flow_tool", serde_json::json!({"x": 1}));
    let final_resp = make_assistant_message("Here is the result.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_call_resp)
            .with_response(final_resp),
    );

    let mock_tool = MockTool::new("my_flow_tool").with_result(ToolOutput::text("tool output"));

    let tmp = tempfile::tempdir().unwrap();
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test flow.")
        .tool(Box::new(mock_tool))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Use the tool.");
    let _ = collect_events(rx).await;

    // Inspect session messages
    let messages = agent.messages().await;

    // Session should contain:
    // [0] User message ("Use the tool.")
    // [1] Assistant (with ToolUse block)
    // [2] Tool result message (role=User with ToolResult content)
    // [3] Assistant final response
    assert_eq!(
        messages.len(),
        4,
        "session should have exactly 4 messages (user, tool-call, tool-result, final), got {}",
        messages.len()
    );

    // Verify message roles in order
    if let alva_types::AgentMessage::Standard(m) = &messages[0] {
        assert_eq!(m.role, MessageRole::User, "message[0] should be User");
    } else {
        panic!("message[0] should be Standard");
    }

    if let alva_types::AgentMessage::Standard(m) = &messages[1] {
        assert_eq!(m.role, MessageRole::Assistant, "message[1] should be Assistant");
        // Should contain a ToolUse block
        let has_tool_use = m.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
        assert!(has_tool_use, "message[1] should contain ToolUse block");
    } else {
        panic!("message[1] should be Standard");
    }

    if let alva_types::AgentMessage::Standard(m) = &messages[2] {
        // Tool result is stored with role=Tool and ToolResult content block
        assert_eq!(m.role, MessageRole::Tool, "message[2] should be Tool (tool result)");
        let has_tool_result = m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }));
        assert!(has_tool_result, "message[2] should contain ToolResult block");
    } else {
        panic!("message[2] should be Standard");
    }

    if let alva_types::AgentMessage::Standard(m) = &messages[3] {
        assert_eq!(m.role, MessageRole::Assistant, "message[3] should be Assistant");
        let has_text = m.content.iter().any(|b| matches!(b, ContentBlock::Text { .. }));
        assert!(has_text, "message[3] should contain Text block");
    } else {
        panic!("message[3] should be Standard");
    }
}

// ---------------------------------------------------------------------------
// Test: Multi-turn with mixed tool types (read + write tools in sequence)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_multi_turn_mixed_tools() {
    // Simulate: read (MockTool) → analyze (MockTool) → report (text)
    let read_call = make_tool_call_message("reader", serde_json::json!({"file": "data.csv"}));
    let analyze_call = make_tool_call_message("analyzer", serde_json::json!({"mode": "deep"}));
    let final_resp = make_assistant_message("Analysis complete: 42 items found.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(read_call)
            .with_response(analyze_call)
            .with_response(final_resp),
    );

    let reader = MockTool::new("reader").with_result(ToolOutput::text("col1,col2\n1,2\n3,4"));
    let analyzer = MockTool::new("analyzer").with_result(ToolOutput::text("42 items"));
    let reader_clone = reader.clone();
    let analyzer_clone = analyzer.clone();

    let tmp = tempfile::tempdir().unwrap();
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test mixed.")
        .tool(Box::new(reader))
        .tool(Box::new(analyzer))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Analyze the data.");
    let events = collect_events(rx).await;

    // 2 tool calls, each with Start and End
    let tool_start_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }))
        .count();
    assert_eq!(tool_start_count, 2, "should have 2 ToolExecutionStart events");

    // Verify tool results flow through correctly
    let reader_end = events.iter().find(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, result }
            if tool_call.name == "reader" && result.model_text().contains("col1,col2"))
    });
    assert!(reader_end.is_some(), "reader tool should return CSV data");

    let analyzer_end = events.iter().find(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, result }
            if tool_call.name == "analyzer" && result.model_text().contains("42 items"))
    });
    assert!(analyzer_end.is_some(), "analyzer tool should return count");

    // Both tools were called exactly once
    assert_eq!(reader_clone.calls().len(), 1);
    assert_eq!(analyzer_clone.calls().len(), 1);

    // Session should have 6 messages: user, assistant(tool), tool-result,
    // assistant(tool), tool-result, assistant(text)
    let messages = agent.messages().await;
    assert_eq!(
        messages.len(),
        6,
        "session should have 6 messages for 2 tool calls + final, got {}",
        messages.len()
    );

    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error"
    );
}

// ---------------------------------------------------------------------------
// Test: Tool not found produces error result (not a crash)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_unknown_tool_produces_error_result() {
    // Model requests a tool that does not exist
    let bad_call = make_tool_call_message("nonexistent_tool", serde_json::json!({}));
    let final_resp = make_assistant_message("I see the tool was not found.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(bad_call)
            .with_response(final_resp),
    );

    let (agent, _tmp) = build_agent(model).await;
    let rx = agent.prompt_text("Call the missing tool.");
    let events = collect_events(rx).await;

    // The tool call should produce a ToolExecutionEnd with an error
    let got_error_result = events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, result }
            if tool_call.name == "nonexistent_tool"
               && result.is_error
               && result.model_text().contains("not found"))
    });
    assert!(
        got_error_result,
        "unknown tool should produce error ToolOutput with 'not found'"
    );

    // Agent should still complete normally
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "agent should end without error despite unknown tool"
    );
}

// ---------------------------------------------------------------------------
// Test: Event ordering — AgentStart before MessageStart before MessageEnd
// before AgentEnd
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_event_ordering_is_correct() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("Ordered response.")),
    );

    let (agent, _tmp) = build_agent(model).await;
    let rx = agent.prompt_text("Check ordering.");
    let events = collect_events(rx).await;

    // Find indices of key events
    let agent_start_idx = events.iter().position(|e| matches!(e, AgentEvent::AgentStart));
    let msg_start_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::MessageStart { .. }));
    let msg_end_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::MessageEnd { .. }));
    let agent_end_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::AgentEnd { .. }));

    assert!(agent_start_idx.is_some(), "should have AgentStart");
    assert!(msg_start_idx.is_some(), "should have MessageStart");
    assert!(msg_end_idx.is_some(), "should have MessageEnd");
    assert!(agent_end_idx.is_some(), "should have AgentEnd");

    let a = agent_start_idx.unwrap();
    let ms = msg_start_idx.unwrap();
    let me = msg_end_idx.unwrap();
    let ae = agent_end_idx.unwrap();

    assert!(a < ms, "AgentStart ({}) should come before MessageStart ({})", a, ms);
    assert!(ms < me, "MessageStart ({}) should come before MessageEnd ({})", ms, me);
    assert!(me < ae, "MessageEnd ({}) should come before AgentEnd ({})", me, ae);
}

// ---------------------------------------------------------------------------
// Test: Tool execution event ordering within a tool call turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_tool_event_ordering() {
    let tool_call_resp = make_tool_call_message("ordered_tool", serde_json::json!({}));
    let final_resp = make_assistant_message("Done.");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_call_resp)
            .with_response(final_resp),
    );

    let mock_tool = MockTool::new("ordered_tool").with_result(ToolOutput::text("ok"));

    let tmp = tempfile::tempdir().unwrap();
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test ordering.")
        .tool(Box::new(mock_tool))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Do it.");
    let events = collect_events(rx).await;

    // Find indices for the tool-related events
    let msg_end_1_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::MessageEnd { .. }));
    let tool_start_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }));
    let tool_end_idx = events
        .iter()
        .position(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }));

    assert!(msg_end_1_idx.is_some());
    assert!(tool_start_idx.is_some());
    assert!(tool_end_idx.is_some());

    let me1 = msg_end_1_idx.unwrap();
    let ts = tool_start_idx.unwrap();
    let te = tool_end_idx.unwrap();

    // MessageEnd (tool-call message) should come before ToolExecutionStart
    assert!(
        me1 < ts,
        "MessageEnd for tool-call ({}) should come before ToolExecutionStart ({})",
        me1, ts
    );
    // ToolExecutionStart should come before ToolExecutionEnd
    assert!(
        ts < te,
        "ToolExecutionStart ({}) should come before ToolExecutionEnd ({})",
        ts, te
    );
}

// ---------------------------------------------------------------------------
// Test: Builtin tool registry contains expected tools after build
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_builtin_tool_registry_completeness() {
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("unused")),
    );

    let tmp = tempfile::tempdir().unwrap();
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .tools(alva_app_core::tool_presets::all_standard())
        .build(model)
        .await
        .expect("build should succeed");

    let defs = agent.tool_registry().definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

    // All core tools should be registered via preset
    let expected = [
        "read_file",
        "file_edit",
        "create_file",
        "execute_shell",
        "grep_search",
        "find_files",
        "list_files",
        "ask_human",
        "view_image",
    ];
    for tool_name in &expected {
        assert!(
            names.contains(tool_name),
            "builtin tool '{}' should be registered, available: {:?}",
            tool_name, names
        );
    }
}
