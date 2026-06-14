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
use alva_app_core::extension::{ApprovalExtension, PermissionExtension};
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
        // PermissionExtension publishes the PermissionModeService on the
        // bus. Without it, `agent.set_permission_mode(...)` is a silent
        // no-op (the lookup misses, the call returns), and Plan-mode
        // tests would falsely pass writes.
        .extension(Box::new(PermissionExtension::new().with_initial(PermissionMode::AcceptShell)))
        .plugin(Box::new(approval_ext))
        .plugin(Box::new(alva_app_core::extension::CoreExtension))
        .plugin(Box::new(alva_app_core::extension::ShellExtension))
        .plugin(Box::new(alva_app_core::extension::InteractionExtension))
        .extension(Box::new(alva_app_core::extension::PlanningExtension))
        .plugin(Box::new(alva_app_core::extension::TaskExtension::default()))
        .plugin(Box::new(alva_app_core::extension::TeamExtension::default()))
        .plugin(Box::new(alva_app_core::extension::UtilityExtension))
        .plugin(Box::new(alva_app_core::extension::WebExtension))
        .extension(Box::new(alva_app_core::extension::LoopDetectionExtension))
        .extension(Box::new(alva_app_core::extension::DanglingToolCallExtension))
        .extension(Box::new(alva_app_core::extension::ToolTimeoutExtension))
        .extension(Box::new(alva_app_core::extension::CompactionExtension))
        .extension(Box::new(alva_app_core::extension::CheckpointExtension))
        .tool(Box::new(NotebookEditTool))
        .build(Arc::new(model))
        .await
        .expect("build should succeed");

    // Mode is initialized to AcceptShell via the extension; reasserting
    // here is a no-op but keeps the intent visible at the call site.
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
    // STUB CONTRACT: SkillTool is wired into the registry but its impl is
    // a stub — see crate::skill_tool source (`Skill execution is not yet
    // wired to the skill registry`). This test pins what the stub
    // CURRENTLY guarantees end-to-end: tool resolves through registry,
    // input is echoed back, and the stub's "not yet wired" marker is in
    // the output. When the real SkillRegistry lands, drop the `not yet
    // wired` assert and add a real skill-invocation assert in lockstep.
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
    assert!(
        text.contains("not yet wired"),
        "skill is a stub — output should declare it (when this fails, the \
         real registry has landed and the test needs to be rewritten): {text}"
    );
}

#[tokio::test]
async fn stage1_tool_search_returns_stub_with_query() {
    // STUB CONTRACT: ToolSearchTool is registered but its impl is a stub
    // (`Tool registry search is not yet wired`). This test pins the stub
    // contract: query echo + max_results echo + "not yet wired" marker.
    // When ToolRegistry-aware search lands, replace these asserts with
    // ones that look up the actual registry contents.
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message("1", "tool_search", serde_json::json!({ "query": "file", "max_results": 7 })),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Search for file tools.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "tool_search");
    assert!(!out.is_error);
    let text = out.model_text();
    assert!(text.contains("file"), "query should appear: {text}");
    assert!(text.contains("7"), "max_results should appear: {text}");
    assert!(
        text.contains("not yet wired"),
        "tool_search is a stub — output should declare it: {text}"
    );
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
async fn stage1_enter_plan_mode_tool_stub_and_real_plan_mode_blocks_writes() {
    // Two contracts in one test:
    //
    // (a) STUB: the enter_plan_mode TOOL is currently a stub — see
    //     src/enter_plan_mode.rs: "In a full implementation, this would
    //     set a flag on the session/context". So calling the tool does
    //     NOT actually switch session mode. Pin the stub's text contract.
    //
    // (b) REAL: the production plan-mode behavior is driven by
    //     `set_permission_mode(PermissionMode::Plan)` from the UI layer,
    //     enforced by PlanModeMiddleware. Verify that PATH end-to-end
    //     by switching mode and asserting a create_file gets blocked.
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();

    // (a) Stub: tool returns the canned message, mode does NOT change.
    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message("1", "enter_plan_mode", serde_json::json!({})),
            make_assistant_message("done"),
        ],
    )
    .await;
    let rx = agent.prompt_text("Enter plan mode.");
    let events = collect_events(rx).await;
    let out = tool_result_for(&events, "enter_plan_mode");
    assert!(!out.is_error);
    assert!(
        out.model_text().contains("Entered planning mode"),
        "stub should return its canned message: {}",
        out.model_text()
    );

    // (b) Real plan mode (set via the UI-layer API) DOES block writes.
    agent.set_permission_mode(PermissionMode::Plan);
    let new_target = ws.join("blocked.txt");
    let new_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "2",
            "create_file",
            serde_json::json!({ "path": new_target.to_str().unwrap(), "content": "x" }),
        ))
        .with_response(make_assistant_message("attempted"));
    agent.set_model(Arc::new(new_model)).await;

    let rx2 = agent.prompt_text("Create a file in plan mode.");
    let events2 = collect_events(rx2).await;
    let blocked = tool_result_for(&events2, "create_file");
    assert!(
        blocked.is_error && blocked.model_text().to_lowercase().contains("blocked"),
        "create_file in Plan mode must be blocked: {}",
        blocked.model_text()
    );
    assert!(!new_target.exists(), "blocked create_file must not have written");
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
                .set_body_string(
                    "<html><body><h1>Hello E2E from wiremock</h1><p>marker-string-xyz</p></body></html>",
                ),
        )
        .expect(1)
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

    // SecurityMiddleware's URL-aware SSRF check on 127.0.0.1 routes through
    // HITL; our auto-approver answers AllowOnce, so the fetch proceeds.
    let out = tool_result_for(&events, "read_url");
    assert!(!out.is_error, "read_url should succeed: {}", out.model_text());
    assert!(
        out.model_text().contains("marker-string-xyz"),
        "read_url result should contain the wiremock body marker, got: {}",
        out.model_text()
    );
    // Drop the MockServer — its Drop verifies `.expect(1)` was satisfied,
    // panicking the test if the mock was never hit.
    drop(server);
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

    // Strengthened: malformed URL must NOT silently succeed. Three valid
    // outcomes (all "deterministic failure"):
    //   (i)  Tool runs, returns is_error=true (most common).
    //   (ii) Middleware blocks before execute — emits ToolExecutionEnd
    //        with is_error=true and a "blocked" message.
    //   (iii) Loop detection / dangling tool fires and the agent ends with
    //         an error WITHOUT a ToolExecutionEnd for read_url at all
    //         (acceptable — the call simply never completed).
    // Forbidden: a ToolExecutionEnd with is_error=false (a silent ok).
    let read_end = events.iter().find_map(|e| match e {
        AgentEvent::ToolExecutionEnd { tool_call, result } if tool_call.name == "read_url" => {
            Some(result.clone())
        }
        _ => None,
    });
    if let Some(out) = read_end {
        assert!(
            out.is_error,
            "malformed URL must NOT return a successful ToolExecutionEnd, got: {}",
            out.model_text()
        );
        let msg = out.model_text().to_lowercase();
        assert!(
            msg.contains("blocked")
                || msg.contains("invalid")
                || msg.contains("error")
                || msg.contains("scheme")
                || msg.contains("url")
                || msg.contains("failed")
                || msg.contains("relative"),
            "error message should explain the URL failure, got: {}",
            out.model_text()
        );
    }
    // Either way, no hang.
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
}

#[tokio::test]
async fn stage2_internet_search_executes_and_returns_terminal_state() {
    // internet_search hits api.duckduckgo.com directly — the URL is
    // hardcoded so wiremock can't substitute. We strengthen what we CAN
    // pin: tool must reach ToolExecutionEnd (not be silently dropped by
    // middleware), and its output must mention the query we sent OR
    // surface a network/HTTP error — never "ok with no signal".
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "internet_search",
                serde_json::json!({ "query": "unique-query-marker-zzz", "max_results": 1 }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Search.");
    let events = collect_events(rx).await;

    let out = tool_result_for(&events, "internet_search");
    let text = out.model_text();
    // Either:
    //   - success: DDG echoes the query in some form (search header / no
    //     results message references the term); OR
    //   - failure: tool flags is_error with a network/HTTP message.
    if out.is_error {
        let lower = text.to_lowercase();
        assert!(
            lower.contains("http")
                || lower.contains("network")
                || lower.contains("timeout")
                || lower.contains("dns")
                || lower.contains("failed")
                || lower.contains("error"),
            "if internet_search errored, message should explain why: {text}"
        );
    } else {
        assert!(
            text.contains("unique-query-marker-zzz")
                || text.to_lowercase().contains("no results")
                || text.to_lowercase().contains("search")
                || text.to_lowercase().contains("result"),
            "successful internet_search should reference the query or report no results: {text}"
        );
    }
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
async fn stage2_task_update_happy_path_marks_task_completed() {
    // True happy-path: create a task, parse its id, update its status,
    // then `task_get` to verify the new status is persisted. This is the
    // full create→update→read round trip — the prior version only tested
    // the unknown-id failure branch.
    let tmp = tempfile::tempdir().unwrap();

    let agent = build_agent_with_responses(
        tmp.path(),
        vec![
            tool_use_message(
                "1",
                "task_create",
                serde_json::json!({ "subject": "updatable", "description": "first" }),
            ),
            make_assistant_message("created"),
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
        .expect("task_create should expose ID line");

    // Step 2: update the task we just made.
    let upd_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "2",
            "task_update",
            serde_json::json!({ "task_id": task_id.clone(), "status": "completed" }),
        ))
        .with_response(make_assistant_message("updated"));
    agent.set_model(Arc::new(upd_model)).await;
    let rx2 = agent.prompt_text("Update.");
    let events2 = collect_events(rx2).await;

    let upd_out = tool_result_for(&events2, "task_update");
    assert!(!upd_out.is_error, "happy-path update should succeed: {}", upd_out.model_text());

    // Step 3: re-fetch and assert the status changed.
    let get_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "3",
            "task_get",
            serde_json::json!({ "task_id": task_id }),
        ))
        .with_response(make_assistant_message("got"));
    agent.set_model(Arc::new(get_model)).await;
    let rx3 = agent.prompt_text("Get.");
    let events3 = collect_events(rx3).await;

    let got_out = tool_result_for(&events3, "task_get");
    assert!(!got_out.is_error, "task_get should succeed: {}", got_out.model_text());
    let got_text_lower = got_out.model_text().to_lowercase();
    assert!(
        got_text_lower.contains("completed"),
        "task_get after update should report 'completed' status: {}",
        got_out.model_text()
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
async fn stage2_exit_plan_mode_tool_stub_and_real_mode_switch_unblocks_writes() {
    // Mirror of enter_plan_mode test:
    //   (a) STUB: exit_plan_mode tool just returns its canned message.
    //   (b) REAL: leaving Plan mode via set_permission_mode lets
    //       create_file succeed again.
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();

    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message("1", "exit_plan_mode", serde_json::json!({})),
            make_assistant_message("done"),
        ],
    )
    .await;

    // (a) Stub text contract.
    let rx = agent.prompt_text("Exit plan.");
    let events = collect_events(rx).await;
    let out = tool_result_for(&events, "exit_plan_mode");
    assert!(!out.is_error);
    assert!(
        out.model_text().contains("Exited planning"),
        "exit_plan_mode stub should return its canned message: {}",
        out.model_text()
    );

    // (b) Real plan-mode round trip: Plan → blocks; back to AcceptShell → allows.
    agent.set_permission_mode(PermissionMode::Plan);
    let blocked_path = ws.join("plan-blocked.txt");
    let blocked_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "2",
            "create_file",
            serde_json::json!({ "path": blocked_path.to_str().unwrap(), "content": "x" }),
        ))
        .with_response(make_assistant_message("attempted"));
    agent.set_model(Arc::new(blocked_model)).await;
    let rx2 = agent.prompt_text("Create in plan mode.");
    let events2 = collect_events(rx2).await;
    let blocked = tool_result_for(&events2, "create_file");
    assert!(
        blocked.is_error && blocked.model_text().to_lowercase().contains("blocked"),
        "create_file in Plan mode should be blocked: {}",
        blocked.model_text()
    );

    // Now exit plan mode (production API) and verify write goes through.
    agent.set_permission_mode(PermissionMode::AcceptShell);
    let allowed_path = ws.join("post-exit.txt");
    let allow_model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "3",
            "create_file",
            serde_json::json!({ "path": allowed_path.to_str().unwrap(), "content": "y" }),
        ))
        .with_response(make_assistant_message("done"));
    agent.set_model(Arc::new(allow_model)).await;
    let rx3 = agent.prompt_text("Create after exit.");
    let events3 = collect_events(rx3).await;
    let allowed = tool_result_for(&events3, "create_file");
    assert!(
        !allowed.is_error,
        "after leaving Plan mode, create_file should succeed: {}",
        allowed.model_text()
    );
    assert!(allowed_path.exists(), "post-exit create_file should have written");
    assert_eq!(std::fs::read_to_string(&allowed_path).unwrap(), "y");
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
async fn stage3_shell_output_feeds_into_grep() {
    // Strengthened: assert the shell's filesystem side-effect happens
    // AND grep finds the content the shell produced. This proves the
    // workspace state mutated by tool N is visible to tool N+1 — the
    // whole point of chaining tools.
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();
    let touched = ws.join("touched.txt");

    let agent = build_agent_with_responses(
        &ws,
        vec![
            tool_use_message(
                "1",
                "execute_shell",
                serde_json::json!({
                    "command": format!("printf 'matchme-shell-output' > {}", touched.display()),
                }),
            ),
            tool_use_message(
                "2",
                "grep_search",
                serde_json::json!({ "pattern": "matchme-shell-output" }),
            ),
            make_assistant_message("done"),
        ],
    )
    .await;

    let rx = agent.prompt_text("Shell then grep.");
    let events = collect_events(rx).await;

    // Shell side-effect must be real, not just "no panic".
    let shell_out = tool_result_for(&events, "execute_shell");
    assert!(
        !shell_out.is_error,
        "shell printf-to-file should succeed (classifier treats `printf` as Unknown → auto-approved in AcceptShell mode): {}",
        shell_out.model_text()
    );
    assert!(touched.exists(), "shell should have created touched.txt");
    assert_eq!(
        std::fs::read_to_string(&touched).unwrap(),
        "matchme-shell-output",
        "file content should match what printf wrote"
    );

    // Grep must find what shell wrote.
    let grep_out = tool_result_for(&events, "grep_search");
    assert!(!grep_out.is_error, "grep_search should succeed: {}", grep_out.model_text());
    let grep_text = grep_out.model_text();
    assert!(
        grep_text.contains("touched.txt") || grep_text.contains("matchme-shell-output"),
        "grep should report the file/match created by shell: {grep_text}"
    );
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
