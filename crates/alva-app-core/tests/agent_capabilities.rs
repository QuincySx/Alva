//! Dual-path agent capability regression harness.
//!
//! A single batch of "capability cases" (one per built-in tool the CLI mini
//! agent exposes) is run two ways from the SAME case definitions:
//!
//!   1. `mock_capability_suite` — deterministic, no API key. Each case scripts
//!      a `MockLanguageModel` to emit the exact tool call, then the shared
//!      `check` closure asserts the real tool ran and produced the right
//!      filesystem / output effect.
//!
//!   2. `real_capability_suite` — env-gated. When `ALVA_TEST_API_KEY` is set,
//!      builds a real provider from `ALVA_TEST_*` env vars and runs the SAME
//!      cases by handing the natural-language `task` to a real LLM, then runs
//!      the SAME `check`. This is the "test a different model / upgrade
//!      regression" path. Without the key it skips (test passes).
//!
//! Both suites print a `✅/❌ case — detail` report table (use
//! `cargo test -- --nocapture` to see it) and fail if any case fails.
//!
//! The agent assembly here MIRRORS the CLI mini agent
//! (`alva-app-cli/src/agent_setup.rs::build_agent`): bare BaseAgentBuilder
//! (auto memory/security/system_context) + ApprovalPlugin + CorePlugin +
//! ShellPlugin + 3 hygiene middlewares. NOTE: mini has NO PermissionPlugin —
//! see `build_mini_agent` for how approvals are still resolved.

use std::path::Path;
use std::sync::Arc;

use alva_app_core::base_agent::BaseAgent;
use alva_app_core::AgentEvent;
use alva_kernel_abi::{ContentBlock, LanguageModel, Message, MessageRole, ToolOutput};
use alva_test::fixtures::make_assistant_message;
use alva_test::mock_provider::MockLanguageModel;

// ---------------------------------------------------------------------------
// Agent assembly — exact mirror of the CLI mini agent.
// ---------------------------------------------------------------------------

/// Build a BaseAgent matching the CLI mini assembly:
///   approval (substrate) + CorePlugin + ShellPlugin + 3 hygiene middlewares.
///
/// IMPORTANT: mini does NOT register PermissionPlugin. Without it,
/// `set_permission_mode` is a no-op and there is no PermissionModeService on
/// the bus. Dangerous tools (create_file / file_edit / execute_shell) instead
/// route through the security middleware's HITL path, and the background task
/// below auto-resolves each request as `AllowOnce` via the bus-published
/// `SecurityGuard` (same mechanism the e2e suite uses). If a tool ever hangs
/// here, it means HITL is NOT the gate and mini genuinely needs PermissionPlugin
/// in its baseline — that would be a real finding, not a harness bug.
async fn build_mini_agent(workspace: &Path, model: Arc<dyn LanguageModel>) -> BaseAgent {
    let (approval_ext, mut approval_rx) =
        alva_app_core::extension::ApprovalPlugin::with_channel();

    let agent = BaseAgent::builder()
        .workspace(workspace)
        .system_prompt(
            "You are a helpful coding assistant. You have access to tools for \
             running shell commands, reading/writing files, and searching code. \
             Use tools when needed to accomplish the user's task. Be concise.",
        )
        .max_iterations(20)
        .plugin(Box::new(approval_ext))
        .plugin(Box::new(alva_app_core::extension::CorePlugin))
        .plugin(Box::new(alva_app_core::extension::ShellPlugin))
        .middleware(Arc::new(
            alva_kernel_core::builtins::LoopDetectionMiddleware::new(),
        ))
        .middleware(Arc::new(
            alva_kernel_core::builtins::DanglingToolCallMiddleware::new(),
        ))
        .middleware(Arc::new(
            alva_kernel_core::builtins::ToolTimeoutMiddleware::default(),
        ))
        .build(model)
        .await
        .expect("failed to build mini agent");

    // Auto-approve every approval request via the bus-published SecurityGuard.
    let bus = agent.bus().clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            if let Some(guard) =
                bus.get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
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

// ---------------------------------------------------------------------------
// Event helpers (shared by both suites' `check` closures).
// ---------------------------------------------------------------------------

/// Build a single Message containing one ToolUse block.
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

/// Drain events until AgentEnd.
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

fn ran_tool(events: &[AgentEvent], tool_name: &str) -> bool {
    events.iter().any(|e| {
        matches!(e, AgentEvent::ToolExecutionEnd { tool_call, .. } if tool_call.name == tool_name)
    })
}

/// First successful (or any, if all errored) ToolOutput for `tool_name`.
fn result_for(events: &[AgentEvent], tool_name: &str) -> Option<ToolOutput> {
    events.iter().find_map(|e| match e {
        AgentEvent::ToolExecutionEnd { tool_call, result } if tool_call.name == tool_name => {
            Some(result.clone())
        }
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Capability case definition.
// ---------------------------------------------------------------------------

struct Cap {
    /// Stable case name (and report row label).
    name: &'static str,
    /// Natural-language instruction for the REAL LLM path.
    task: &'static str,
    /// Prepare the workspace before the run (e.g. pre-write files).
    setup: Box<dyn Fn(&Path)>,
    /// Script the MockLanguageModel's tool call(s) for the MOCK path.
    /// (A trailing assistant message is appended by the runner.)
    mock_script: Box<dyn Fn(&Path) -> Vec<Message>>,
    /// Shared assertion: inspect workspace + events, Ok(()) on pass.
    check: Box<dyn Fn(&Path, &[AgentEvent]) -> Result<(), String>>,
}

fn cases() -> Vec<Cap> {
    vec![
        // ── create_file ────────────────────────────────────────────────
        Cap {
            name: "create_file",
            task: "Create a file named hello.txt in the workspace with exactly the content: hello",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|ws| {
                let target = ws.join("hello.txt");
                vec![tool_use_message(
                    "1",
                    "create_file",
                    serde_json::json!({ "path": target.to_str().unwrap(), "content": "hello" }),
                )]
            }),
            check: Box::new(|ws, events| {
                if !ran_tool(events, "create_file") {
                    return Err("create_file did not run".into());
                }
                let target = ws.join("hello.txt");
                if !target.exists() {
                    return Err("hello.txt was not created".into());
                }
                let content = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
                if content.trim() != "hello" {
                    return Err(format!("content mismatch: {content:?}"));
                }
                Ok(())
            }),
        },
        // ── read_file ──────────────────────────────────────────────────
        Cap {
            name: "read_file",
            task: "Read the file data.txt in the workspace and report its contents.",
            setup: Box::new(|ws| {
                std::fs::write(ws.join("data.txt"), "secret-content-123").unwrap();
            }),
            mock_script: Box::new(|ws| {
                let target = ws.join("data.txt");
                vec![tool_use_message(
                    "1",
                    "read_file",
                    serde_json::json!({ "path": target.to_str().unwrap() }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "read_file")
                    .ok_or("read_file did not run")?;
                if out.is_error {
                    return Err(format!("read_file errored: {}", out.model_text()));
                }
                if !out.model_text().contains("secret-content-123") {
                    return Err(format!(
                        "read_file output missing file content: {}",
                        out.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── file_edit ──────────────────────────────────────────────────
        Cap {
            name: "file_edit",
            task: "In the file edit_me.txt, replace the word alpha with beta.",
            setup: Box::new(|ws| {
                std::fs::write(ws.join("edit_me.txt"), "alpha").unwrap();
            }),
            mock_script: Box::new(|ws| {
                let target = ws.join("edit_me.txt");
                vec![tool_use_message(
                    "1",
                    "file_edit",
                    serde_json::json!({
                        "path": target.to_str().unwrap(),
                        "old_str": "alpha",
                        "new_str": "beta",
                    }),
                )]
            }),
            check: Box::new(|ws, events| {
                if !ran_tool(events, "file_edit") {
                    return Err("file_edit did not run".into());
                }
                let content =
                    std::fs::read_to_string(ws.join("edit_me.txt")).map_err(|e| e.to_string())?;
                if content.trim() != "beta" {
                    return Err(format!("edit not applied, content: {content:?}"));
                }
                Ok(())
            }),
        },
        // ── list_files ─────────────────────────────────────────────────
        Cap {
            name: "list_files",
            task: "List the files in the workspace.",
            setup: Box::new(|ws| {
                std::fs::write(ws.join("alpha.txt"), "a").unwrap();
                std::fs::write(ws.join("beta.txt"), "b").unwrap();
            }),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message("1", "list_files", serde_json::json!({}))]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "list_files").ok_or("list_files did not run")?;
                if out.is_error {
                    return Err(format!("list_files errored: {}", out.model_text()));
                }
                let text = out.model_text();
                if !text.contains("alpha.txt") || !text.contains("beta.txt") {
                    return Err(format!("listing missing expected files: {text}"));
                }
                Ok(())
            }),
        },
        // ── find_files ─────────────────────────────────────────────────
        Cap {
            name: "find_files",
            task: "Find files matching the glob pattern *.rs in the workspace.",
            setup: Box::new(|ws| {
                std::fs::write(ws.join("needle.rs"), "fn main() {}").unwrap();
                std::fs::write(ws.join("other.txt"), "x").unwrap();
            }),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "find_files",
                    serde_json::json!({ "pattern": "*.rs" }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "find_files").ok_or("find_files did not run")?;
                if out.is_error {
                    return Err(format!("find_files errored: {}", out.model_text()));
                }
                if !out.model_text().contains("needle.rs") {
                    return Err(format!(
                        "find_files did not return needle.rs: {}",
                        out.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── grep_search ────────────────────────────────────────────────
        Cap {
            name: "grep_search",
            task: "Search the workspace for the text FINDME_MARKER.",
            setup: Box::new(|ws| {
                std::fs::write(ws.join("haystack.txt"), "line one\nFINDME_MARKER here\nline three")
                    .unwrap();
            }),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "grep_search",
                    serde_json::json!({ "pattern": "FINDME_MARKER" }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "grep_search").ok_or("grep_search did not run")?;
                if out.is_error {
                    return Err(format!("grep_search errored: {}", out.model_text()));
                }
                let text = out.model_text();
                if !text.contains("FINDME_MARKER") && !text.contains("haystack.txt") {
                    return Err(format!("grep_search found no match: {text}"));
                }
                Ok(())
            }),
        },
        // ── execute_shell ──────────────────────────────────────────────
        Cap {
            name: "execute_shell",
            task: "Run the shell command: echo hello_capability_marker",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "execute_shell",
                    serde_json::json!({ "command": "echo hello_capability_marker" }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "execute_shell").ok_or("execute_shell did not run")?;
                if out.is_error {
                    return Err(format!("execute_shell errored: {}", out.model_text()));
                }
                if !out.model_text().contains("hello_capability_marker") {
                    return Err(format!(
                        "shell output missing echoed text: {}",
                        out.model_text()
                    ));
                }
                Ok(())
            }),
        },
    ]
}

/// Print a report table from (name, Result) rows and return whether all passed.
fn print_report(header: &str, rows: &[(&str, Result<(), String>)]) -> bool {
    eprintln!("\n===== {header} =====");
    let mut all_passed = true;
    for (name, res) in rows {
        match res {
            Ok(()) => eprintln!("✅ {name}"),
            Err(detail) => {
                all_passed = false;
                eprintln!("❌ {name} — {detail}");
            }
        }
    }
    eprintln!(
        "----- {} / {} passed -----\n",
        rows.iter().filter(|(_, r)| r.is_ok()).count(),
        rows.len()
    );
    all_passed
}

// ===========================================================================
// MOCK SUITE — deterministic, no API key.
// ===========================================================================

#[tokio::test]
async fn mock_capability_suite() {
    let mut rows: Vec<(&str, Result<(), String>)> = Vec::new();

    for case in cases() {
        // Independent canonicalized tempdir per case (security middleware
        // canonicalizes paths; macOS maps /var → /private/var).
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        (case.setup)(&ws);

        let mut script = (case.mock_script)(&ws);
        script.push(make_assistant_message("done"));

        let mut model = MockLanguageModel::new();
        for m in script {
            model = model.with_response(m);
        }

        let agent = build_mini_agent(&ws, Arc::new(model)).await;
        let rx = agent.prompt_text(case.task);

        // Guard against a hang (e.g. an approval that never resolves) so the
        // suite reports ❌ instead of blocking forever.
        let events = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            collect_events(rx),
        )
        .await
        {
            Ok(ev) => ev,
            Err(_) => {
                rows.push((
                    case.name,
                    Err("TIMEOUT — agent never reached AgentEnd (tool stuck at HITL?)".into()),
                ));
                continue;
            }
        };

        rows.push((case.name, (case.check)(&ws, &events)));
    }

    let all_passed = print_report("MOCK capability suite (MockLanguageModel)", &rows);
    assert!(all_passed, "mock capability suite had failures — see report above");
}

// ===========================================================================
// REAL SUITE — env-gated regression gate against a live model.
// ===========================================================================

#[tokio::test]
async fn real_capability_suite() {
    let api_key = match std::env::var("ALVA_TEST_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!(
                "real_capability_suite skipped: set ALVA_TEST_API_KEY \
                 (+ optional ALVA_TEST_MODEL / ALVA_TEST_KIND / ALVA_TEST_BASE_URL) to run"
            );
            return;
        }
    };

    let model_name = std::env::var("ALVA_TEST_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
    let kind = std::env::var("ALVA_TEST_KIND").ok().filter(|s| !s.is_empty());
    let base_url = std::env::var("ALVA_TEST_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    let config = alva_llm_provider::ProviderConfig {
        api_key,
        model: model_name.clone(),
        base_url,
        max_tokens: 4096,
        custom_headers: std::collections::HashMap::new(),
        kind: kind.clone(),
    };

    // Same match the CLI uses (agent_setup.rs).
    let make_model = || -> Arc<dyn LanguageModel> {
        match config.kind.as_deref() {
            Some("anthropic") => Arc::new(alva_llm_provider::AnthropicProvider::new(config.clone())),
            Some("openai-responses") => {
                Arc::new(alva_llm_provider::OpenAIResponsesProvider::new(config.clone()))
            }
            Some("gemini") => Arc::new(alva_llm_provider::GeminiProvider::new(config.clone())),
            _ => Arc::new(alva_llm_provider::OpenAIChatProvider::new(config.clone())),
        }
    };

    let mut rows: Vec<(&str, Result<(), String>)> = Vec::new();

    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        (case.setup)(&ws);

        let agent = build_mini_agent(&ws, make_model()).await;
        let rx = agent.prompt_text(case.task);

        let events = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            collect_events(rx),
        )
        .await
        {
            Ok(ev) => ev,
            Err(_) => {
                rows.push((case.name, Err("TIMEOUT (120s) waiting on live model".into())));
                continue;
            }
        };

        rows.push((case.name, (case.check)(&ws, &events)));
    }

    let header = format!("REAL capability suite (model: {})", model_name);
    let all_passed = print_report(&header, &rows);
    assert!(
        all_passed,
        "real capability suite had failures against model `{model_name}` — see report above"
    );
}
