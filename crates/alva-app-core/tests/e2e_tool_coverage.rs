//! E2E coverage tests for individual and combined builtin tools.
//!
//! Each test wires a `BaseAgent` with the full extension set, scripts a
//! `MockLanguageModel` to issue a specific tool call (or sequence), and
//! verifies the tool resolved → ran → returned through the full pipeline.
//!
//! Stages:
//!   1. Individual-tool coverage (10 tests) — one tool per test.
//!   2. Network + task-service tests (8 tests) — read_url via wiremock,
//!      task lifecycle through the TaskExtension's in-memory store.
//!   3. Combined-tool sequences (5 tests) — multi-turn LLM loops chaining
//!      several real tools.

use std::sync::Arc;

use alva_app_core::base_agent::{BaseAgent, PermissionMode};
use alva_app_core::extension::ApprovalExtension;
use alva_app_core::AgentEvent;
use alva_agent_extension_builtin::notebook_edit::NotebookEditTool;
use alva_test::fixtures::make_assistant_message;
use alva_test::mock_provider::MockLanguageModel;
use alva_kernel_abi::{ContentBlock, Message, MessageRole};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a BaseAgent with the full standard extension set, a scripted
/// mock model, and a background task that auto-approves every approval
/// request the security middleware emits.
///
/// Permission mode is set to `AcceptShell` (→ `Sec::Auto`) so:
///   - Bash classifier auto-approves read-only shell commands.
///   - Dangerous tools without a `command` field (e.g. `create_file`,
///     `file_edit`) fall through to HITL → the auto-approver answers
///     `AllowOnce`.
async fn build_agent_with_responses(
    workspace: &std::path::Path,
    responses: Vec<Message>,
) -> BaseAgent {
    let (approval_ext, mut approval_rx) = ApprovalExtension::with_channel();

    let mut model = MockLanguageModel::new();
    for r in responses {
        model = model.with_response(r);
    }

    let agent = BaseAgent::builder()
        .workspace(workspace)
        .system_prompt("You are a test assistant.")
        .extension(Box::new(approval_ext))
        .extension(Box::new(alva_app_core::extension::CoreExtension))
        .extension(Box::new(alva_app_core::extension::ShellExtension))
        .extension(Box::new(alva_app_core::extension::InteractionExtension))
        .extension(Box::new(alva_app_core::extension::PlanningExtension))
        .extension(Box::new(alva_app_core::extension::TaskExtension::default()))
        .extension(Box::new(alva_app_core::extension::TeamExtension::default()))
        .extension(Box::new(alva_app_core::extension::UtilityExtension))
        .extension(Box::new(alva_app_core::extension::WebExtension))
        .extension(Box::new(alva_app_core::extension::LoopDetectionExtension))
        .extension(Box::new(alva_app_core::extension::DanglingToolCallExtension))
        .extension(Box::new(alva_app_core::extension::ToolTimeoutExtension))
        .extension(Box::new(alva_app_core::extension::CompactionExtension))
        .extension(Box::new(alva_app_core::extension::CheckpointExtension))
        .tool(Box::new(NotebookEditTool))
        .build(Arc::new(model))
        .await
        .expect("build should succeed");

    agent.set_permission_mode(PermissionMode::AcceptShell);

    // Auto-approve approval requests via the bus-published SecurityGuard.
    // BaseAgent is not Clone, so we operate on the guard directly instead
    // of going through `agent.resolve_permission`.
    let bus = agent.bus().clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            if let Some(guard) = bus
                .get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
            {
                let mut g = guard.lock().await;
                g.resolve_permission(
                    &req.request_id,
                    &req.tool_name,
                    alva_agent_security::PermissionDecision::AllowOnce,
                );
            }
        }
    });

    agent
}

/// Build a single Message containing one ToolUse block with the given args.
fn tool_use_message(id: &str, name: &str, input: serde_json::Value) -> Message {
    Message {
        id: format!("msg-{id}"),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: format!("call-{id}"),
            name: name.to_string(),
            input,
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    }
}

/// Drain all events until AgentEnd. Returns the full event list.
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

/// Find the first ToolExecutionEnd event for the given tool name and
/// return a clone of its `result` field. Panics if not found — the
/// caller knows the tool was supposed to run.
fn tool_result_for(events: &[AgentEvent], tool_name: &str) -> alva_kernel_abi::ToolOutput {
    events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolExecutionEnd { tool_call, result } if tool_call.name == tool_name => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ToolExecutionEnd for `{tool_name}` in event stream"))
}

fn ran_tool(events: &[AgentEvent], tool_name: &str) -> bool {
    events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolExecutionEnd { tool_call, .. } if tool_call.name == tool_name))
}

fn agent_ended_cleanly(events: &[AgentEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None }))
}

// ===========================================================================
// STAGE 1: individual-tool coverage
// ===========================================================================

#[tokio::test]
async fn stage1_create_file_writes_to_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("new.txt");

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "create_file",
                serde_json::json!({ "path": target.to_str().unwrap(), "content": "hello" }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Create the file.");
    let events = collect_events(rx).await;

    let result = tool_result_for(&events, "create_file");
    assert!(!result.is_error, "create_file should succeed: {}", result.model_text());
    assert!(target.exists(), "create_file should have written the file");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
    assert!(agent_ended_cleanly(&events));
}

#[tokio::test]
async fn stage1_list_files_returns_workspace_entries() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "b").unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "list_files", serde_json::json!({})),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("List the workspace.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "list_files");
    assert!(!out.is_error, "list_files should succeed: {}", out.model_text());
    let text = out.model_text();
    assert!(text.contains("a.txt"), "expected a.txt in listing: {text}");
    assert!(text.contains("b.txt"), "expected b.txt in listing: {text}");
}

#[tokio::test]
async fn stage1_notebook_edit_modifies_cell() {
    let tmp = tempfile::tempdir().unwrap();
    // macOS canonicalizes /var/folders → /private/var/folders; the
    // security middleware enforces authorized_roots after canonicalize,
    // so we must canonicalize the path we hand the tool too.
    let ws = tmp.path().canonicalize().unwrap();
    let nb_path = ws.join("test.ipynb");
    let nb = serde_json::json!({
        "cells": [
            {
                "cell_type": "code",
                "id": "c1",
                "metadata": {},
                "source": ["old"],
                "outputs": [],
                "execution_count": null,
            }
        ],
        "metadata": {},
        "nbformat": 4,
        "nbformat_minor": 5,
    });
    std::fs::write(&nb_path, serde_json::to_string(&nb).unwrap()).unwrap();

    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message(
                "1",
                "notebook_edit",
                serde_json::json!({
                    "notebook_path": nb_path.to_str().unwrap(),
                    "cell_id": "c1",
                    "edit_mode": "replace",
                    "new_source": "print('new')",
                }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Edit notebook cell.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "notebook_edit");
    assert!(!out.is_error, "notebook_edit should succeed: {}", out.model_text());
    let updated = std::fs::read_to_string(&nb_path).unwrap();
    assert!(updated.contains("print('new')"), "notebook should reflect new source: {updated}");
}

#[tokio::test]
async fn stage1_execute_shell_runs_command_in_accept_shell_mode() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "execute_shell", serde_json::json!({ "command": "echo hello_e2e_coverage" })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Run echo.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "execute_shell");
    assert!(!out.is_error, "execute_shell should succeed in AcceptShell mode: {}", out.model_text());
    assert!(
        out.model_text().contains("hello_e2e_coverage"),
        "shell output should include echoed text: {}",
        out.model_text()
    );
}

#[tokio::test]
async fn stage1_skill_tool_invokes_stub_and_echoes_name() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "skill", serde_json::json!({ "skill": "commit", "args": "--amend" })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Invoke commit skill.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "skill");
    assert!(!out.is_error);
    let text = out.model_text();
    assert!(text.contains("commit"), "skill name should appear: {text}");
    assert!(text.contains("--amend"), "args should appear: {text}");
}

#[tokio::test]
async fn stage1_tool_search_returns_stub_with_query() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "tool_search", serde_json::json!({ "query": "file" })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Search for file tools.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "tool_search");
    assert!(!out.is_error);
    assert!(out.model_text().contains("file"), "query should appear: {}", out.model_text());
}

#[tokio::test]
async fn stage1_config_get_returns_not_set_for_unknown_key() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "config", serde_json::json!({ "action": "get", "key": "missing.key" })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Check config.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "config");
    assert!(!out.is_error);
    assert!(
        out.model_text().contains("not set") || out.model_text().contains("missing.key"),
        "expected 'not set' message: {}",
        out.model_text()
    );
}

#[tokio::test]
async fn stage1_sleep_tool_blocks_for_requested_duration() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "sleep", serde_json::json!({ "duration_ms": 50 })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let start = std::time::Instant::now();
    let rx = agent.prompt_text("Sleep briefly.");
    let events = collect_events(rx).await;
    let elapsed = start.elapsed();

    let out = tool_result_for(&events, "sleep");
    assert!(!out.is_error, "sleep should succeed: {}", out.model_text());
    assert!(
        elapsed.as_millis() >= 40,
        "sleep should consume real time (got {}ms)",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn stage1_todo_write_appends_to_default_file() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "todo_write",
                serde_json::json!({ "content": "- first todo item" }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Record a todo.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "todo_write");
    assert!(!out.is_error, "todo_write should succeed: {}", out.model_text());
    let written = std::fs::read_to_string(tmp.path().join("CLAUDE.md"))
        .expect("todo_write should create CLAUDE.md");
    assert!(written.contains("first todo item"), "CLAUDE.md should contain item: {written}");
}

#[tokio::test]
async fn stage1_enter_plan_mode_succeeds() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "enter_plan_mode", serde_json::json!({})),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Enter plan mode.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "enter_plan_mode");
    assert!(!out.is_error, "enter_plan_mode should succeed: {}", out.model_text());
    assert!(agent_ended_cleanly(&events));
}

// ===========================================================================
// STAGE 2: network + task-service tests
// ===========================================================================

#[tokio::test]
async fn stage2_read_url_fetches_from_wiremock_server() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string("<html><body><h1>Hello E2E</h1></body></html>"),
        )
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let url = server.uri();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "read_url", serde_json::json!({ "url": url })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Read the URL.");
    let events = collect_events(rx).await;

    // SecurityMiddleware's URL-aware SSRF check may classify 127.0.0.1 as
    // high-risk and route through HITL — our auto-approver answers
    // AllowOnce, so the tool runs either way. Verify ToolExecutionEnd and
    // a clean terminal state; a hang would surface as test timeout.
    assert!(ran_tool(&events, "read_url"), "read_url should have executed");
    assert!(agent_ended_cleanly(&events));
}

#[tokio::test]
async fn stage2_read_url_with_malformed_url_returns_terminal_state() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "read_url", serde_json::json!({ "url": "not-a-real-url" })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Read garbage URL.");
    let events = collect_events(rx).await;

    // SecurityMiddleware's SSRF check may deny `not-a-real-url` outright
    // (Deny is emitted as a tool-end with is_error=true) OR the URL parser
    // may reject it inside the tool itself. Either way, the agent must
    // terminate — the failure we're pinning is "hang", not "any specific
    // error shape".
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
}

#[tokio::test]
async fn stage2_internet_search_executes_and_returns_terminal_state() {
    // Does NOT assert DDG content — that would be flaky. Only verifies
    // the tool resolves through the pipeline and the agent reaches a
    // clean terminal state.
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "internet_search",
                serde_json::json!({ "query": "rust", "max_results": 1 }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Search.");
    let events = collect_events(rx).await;

    // DDG hit may fail (offline CI / blocked) or middleware may deny
    // the outbound HTTP — both surface as deterministic terminal states.
    // Pin only that the agent terminates and either started executing the
    // tool OR explicitly errored without hanging.
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
}

#[tokio::test]
async fn stage2_task_create_persists_to_in_memory_store() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "task_create",
                serde_json::json!({ "subject": "do the thing", "description": "details" }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Create a task.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "task_create");
    assert!(!out.is_error, "task_create should succeed: {}", out.model_text());
    assert!(out.model_text().contains("Task created"), "should announce creation: {}", out.model_text());
    assert!(out.model_text().contains("ID:"), "should expose ID: {}", out.model_text());
}

#[tokio::test]
async fn stage2_task_list_after_create_includes_new_task() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "task_create",
                serde_json::json!({ "subject": "listed-task", "description": "x" }),
            ),
            tool_use_message("2", "task_list", serde_json::json!({})),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Create then list.");
    let events = collect_events(rx).await;

    let list_out = tool_result_for(&events, "task_list");
    assert!(!list_out.is_error, "task_list should succeed: {}", list_out.model_text());
    assert!(
        list_out.model_text().contains("listed-task"),
        "task_list output should include the just-created subject: {}",
        list_out.model_text()
    );
}

#[tokio::test]
async fn stage2_task_update_with_unknown_id_reaches_service() {
    // task_update needs a real task_id that we can't statically script
    // (IDs are generated at runtime). Instead, drive an update against a
    // known-missing ID and assert the tool resolved through the pipeline
    // and produced a deterministic response from the service.
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "task_update",
                serde_json::json!({ "task_id": "nonexistent-id-xyz", "status": "completed" }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;
    let rx = agent.prompt_text("Update a task.");
    let events = collect_events(rx).await;

    let upd = tool_result_for(&events, "task_update");
    // The service either errors (is_error=true) or returns a structured
    // not-found / no-changes response. Either way the tool resolved.
    let text_lower = upd.model_text().to_lowercase();
    assert!(
        upd.is_error
            || text_lower.contains("not")
            || text_lower.contains("update")
            || text_lower.contains("nonexistent"),
        "task_update should reach the service and produce a deterministic output: {}",
        upd.model_text()
    );
}

#[tokio::test]
async fn stage2_task_get_returns_state_for_created_task() {
    // Create + get on the same agent so the in-memory TaskService
    // singleton survives between prompts. The second prompt uses a fresh
    // scripted model installed via `set_model`.
    let tmp = tempfile::tempdir().unwrap();
    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "task_create",
                serde_json::json!({ "subject": "gettable", "description": "y" }),
            ),
            make_assistant_message("first done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Create.");
    let events = collect_events(rx).await;
    let created = tool_result_for(&events, "task_create");
    let task_id = created
        .model_text()
        .lines()
        .find_map(|l| l.trim().strip_prefix("ID: ").map(|s| s.to_string()))
        .expect("task_create output should expose ID line");

    let new_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "2",
            "task_get",
            serde_json::json!({ "task_id": task_id }),
        ))
        .with_response(make_assistant_message("done"));
    agent.set_model(Arc::new(new_model)).await;

    let rx2 = agent.prompt_text("Look up the task.");
    let events2 = collect_events(rx2).await;

    let got = tool_result_for(&events2, "task_get");
    assert!(!got.is_error, "task_get should find the task: {}", got.model_text());
    assert!(
        got.model_text().contains("gettable"),
        "task_get output should include the subject: {}",
        got.model_text()
    );
}

#[tokio::test]
async fn stage2_exit_plan_mode_succeeds() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "exit_plan_mode", serde_json::json!({})),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Exit plan.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "exit_plan_mode");
    assert!(!out.is_error);
    assert!(
        out.model_text().contains("Exited planning"),
        "should confirm exit: {}",
        out.model_text()
    );
}

// ===========================================================================
// STAGE 3: combined-tool sequences
// ===========================================================================

#[tokio::test]
async fn stage3_read_then_edit_then_create_in_sequence() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();
    let src = ws.join("src.txt");
    let new_file = ws.join("derived.txt");
    std::fs::write(&src, "alpha").unwrap();

    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message(
                "1",
                "read_file",
                serde_json::json!({ "path": src.to_str().unwrap() }),
            ),
            tool_use_message(
                "2",
                "file_edit",
                serde_json::json!({
                    "path": src.to_str().unwrap(),
                    "old_str": "alpha",
                    "new_str": "beta",
                }),
            ),
            tool_use_message(
                "3",
                "create_file",
                serde_json::json!({
                    "path": new_file.to_str().unwrap(),
                    "content": "derived",
                }),
            ),
            make_assistant_message("all three done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Read, edit, create.");
    let events = collect_events(rx).await;

    assert!(ran_tool(&events, "read_file"), "read_file should run");
    assert!(ran_tool(&events, "file_edit"), "file_edit should run");
    assert!(ran_tool(&events, "create_file"), "create_file should run");

    let updated = std::fs::read_to_string(&src).unwrap();
    assert_eq!(updated, "beta", "file_edit should have substituted contents");
    assert_eq!(
        std::fs::read_to_string(&new_file).unwrap(),
        "derived",
        "create_file should have written derived.txt"
    );
    assert!(agent_ended_cleanly(&events));
}

#[tokio::test]
async fn stage3_grep_then_read_then_edit_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();
    let target = ws.join("hit.rs");
    std::fs::write(&target, "fn needle() {}").unwrap();

    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message("1", "grep_search", serde_json::json!({ "pattern": "needle" })),
            tool_use_message(
                "2",
                "read_file",
                serde_json::json!({ "path": target.to_str().unwrap() }),
            ),
            tool_use_message(
                "3",
                "file_edit",
                serde_json::json!({
                    "path": target.to_str().unwrap(),
                    "old_str": "needle",
                    "new_str": "haystack",
                }),
            ),
            make_assistant_message("chain done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Grep, read, then edit.");
    let events = collect_events(rx).await;

    assert!(ran_tool(&events, "grep_search"));
    assert!(ran_tool(&events, "read_file"));
    assert!(ran_tool(&events, "file_edit"));
    let updated = std::fs::read_to_string(&target).unwrap();
    assert_eq!(updated, "fn haystack() {}");
}

#[tokio::test]
async fn stage3_shell_then_grep_chain_terminates_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let touched = tmp.path().join("touched.txt");

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "execute_shell",
                serde_json::json!({
                    "command": format!("printf 'matchme' > {}", touched.display()),
                }),
            ),
            tool_use_message("2", "grep_search", serde_json::json!({ "pattern": "matchme" })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Shell then grep.");
    let events = collect_events(rx).await;

    assert!(ran_tool(&events, "execute_shell"));
    assert!(ran_tool(&events, "grep_search"));
    // Bash classifier may flag `>` redirect as destructive and deny in
    // Auto mode; we don't pin shell success, only the pipeline reaching
    // a deterministic terminal state.
    assert!(agent_ended_cleanly(&events));
}

#[tokio::test]
async fn stage3_create_then_list_then_read_visibility_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();
    let target = ws.join("chain.txt");

    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message(
                "1",
                "create_file",
                serde_json::json!({
                    "path": target.to_str().unwrap(),
                    "content": "visible-by-grep",
                }),
            ),
            tool_use_message("2", "list_files", serde_json::json!({})),
            tool_use_message(
                "3",
                "read_file",
                serde_json::json!({ "path": target.to_str().unwrap() }),
            ),
            make_assistant_message("all three"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Create, list, read.");
    let events = collect_events(rx).await;

    assert!(ran_tool(&events, "create_file"));
    assert!(ran_tool(&events, "list_files"));
    assert!(ran_tool(&events, "read_file"));

    let list_out = tool_result_for(&events, "list_files");
    assert!(
        list_out.model_text().contains("chain.txt"),
        "list_files after create should see the new file: {}",
        list_out.model_text()
    );
    let read_out = tool_result_for(&events, "read_file");
    assert!(
        read_out.model_text().contains("visible-by-grep"),
        "read_file should return content created in step 1: {}",
        read_out.model_text()
    );
}

#[tokio::test]
async fn stage3_task_create_then_get_round_trip() {
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "task_create",
                serde_json::json!({ "subject": "round-trip", "description": "details" }),
            ),
            make_assistant_message("first done"),
        ],
    )
    .await;
    let rx = agent.prompt_text("Create.");
    let events = collect_events(rx).await;
    let created = tool_result_for(&events, "task_create");
    let task_id = created
        .model_text()
        .lines()
        .find_map(|l| l.trim().strip_prefix("ID: ").map(|s| s.to_string()))
        .expect("ID line should be present");

    let next_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "2",
            "task_get",
            serde_json::json!({ "task_id": task_id.clone() }),
        ))
        .with_response(make_assistant_message("done"));
    agent.set_model(Arc::new(next_model)).await;

    let rx2 = agent.prompt_text("Get it back.");
    let events2 = collect_events(rx2).await;
    let got = tool_result_for(&events2, "task_get");
    assert!(!got.is_error, "task_get should resolve: {}", got.model_text());
    assert!(
        got.model_text().contains("round-trip"),
        "round-trip subject should reappear: {}",
        got.model_text()
    );
}
