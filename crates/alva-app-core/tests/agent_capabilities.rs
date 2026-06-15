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
        // P2(安全/长会话):Permission + Compaction —— 与 CLI build_agent 镜像
        .plugin(Box::new(alva_app_core::extension::PermissionPlugin::new()))
        .middleware(Arc::new(
            alva_host_native::middleware::CompactionMiddleware::default(),
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
    /// Human-readable description of what this case asserts (shown in report).
    assertion: &'static str,
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
            assertion: "Asserts the `create_file` tool ran AND `hello.txt` now exists in the \
                        workspace with content exactly equal to \"hello\".",
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
            assertion: "Asserts the `read_file` tool ran successfully (is_error=false) AND its \
                        output text contains the pre-seeded marker \"secret-content-123\".",
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
            assertion: "Asserts the `file_edit` tool ran AND the on-disk content of edit_me.txt \
                        changed from \"alpha\" to \"beta\".",
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
            assertion: "Asserts the `list_files` tool ran successfully AND its output lists both \
                        pre-seeded files: alpha.txt and beta.txt.",
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
            assertion: "Asserts the `find_files` tool ran successfully AND its output includes \
                        needle.rs (the only *.rs file seeded), proving the glob matched.",
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
            assertion: "Asserts the `grep_search` tool ran successfully AND its output references \
                        the match (the FINDME_MARKER text or the file haystack.txt that contains it).",
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
            assertion: "Asserts the `execute_shell` tool ran successfully (is_error=false) AND its \
                        captured stdout contains the echoed text \"hello_capability_marker\".",
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

// ---------------------------------------------------------------------------
// Per-case captured run + reporting.
// ---------------------------------------------------------------------------

/// Everything captured for ONE case run, enough to render a full audit row.
struct CaseRun {
    name: &'static str,
    task: &'static str,
    assertion: &'static str,
    /// "mock" path injects the tool call via the scripted model; "real" path
    /// hands the task to a live LLM. Recorded so the report can label it.
    mode: &'static str,
    events: Vec<AgentEvent>,
    verdict: Result<(), String>,
    latency_ms: u128,
}

impl CaseRun {
    fn passed(&self) -> bool {
        self.verdict.is_ok()
    }
}

/// One tool execution distilled from the event stream for display.
struct ToolTrace {
    name: String,
    input_pretty: String,
    output_text: String,
    output_len: usize,
    is_error: bool,
}

/// Pull the human-auditable trace out of a raw event stream:
/// (ordered tool executions, final assistant text, AgentEnd error).
fn extract_trace(events: &[AgentEvent]) -> (Vec<ToolTrace>, Option<String>, Option<String>) {
    let mut tools = Vec::new();
    let mut final_text: Option<String> = None;
    let mut end_error: Option<String> = None;

    for ev in events {
        match ev {
            AgentEvent::ToolExecutionEnd { tool_call, result } => {
                let input_pretty = serde_json::to_string_pretty(&tool_call.arguments)
                    .unwrap_or_else(|_| tool_call.arguments.to_string());
                let output_text = result.model_text();
                tools.push(ToolTrace {
                    name: tool_call.name.clone(),
                    input_pretty,
                    output_len: output_text.chars().count(),
                    output_text,
                    is_error: result.is_error,
                });
            }
            AgentEvent::MessageEnd { message } => {
                // Capture the LAST non-empty assistant text as the model's
                // final answer. Standard/Steering/FollowUp wrap a Message.
                let inner = match message {
                    alva_kernel_abi::AgentMessage::Standard(m)
                    | alva_kernel_abi::AgentMessage::Steering(m)
                    | alva_kernel_abi::AgentMessage::FollowUp(m) => Some(m),
                    _ => None,
                };
                if let Some(m) = inner {
                    if m.role == MessageRole::Assistant {
                        let t = m.text_content();
                        if !t.trim().is_empty() {
                            final_text = Some(t);
                        }
                    }
                }
            }
            AgentEvent::AgentEnd { error } => {
                if let Some(e) = error {
                    end_error = Some(e.clone());
                }
            }
            _ => {}
        }
    }

    (tools, final_text, end_error)
}

// ---------------------------------------------------------------------------
// HTML report.
// ---------------------------------------------------------------------------

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Truncate long output for display, annotating the original length. The full
/// text is large enough to audit; we cap to keep the file a sane size.
fn truncate_for_display(s: &str, max: usize) -> String {
    let len = s.chars().count();
    if len <= max {
        return html_escape(s);
    }
    let head: String = s.chars().take(max).collect();
    format!(
        "{}\n\n… [truncated for display — original length {} chars]",
        html_escape(&head),
        len
    )
}

/// Sanitize a label into a filename-safe token (replace `/`, spaces, etc.).
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' })
        .collect()
}

fn now_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string()
}

/// Render the self-contained HTML report, write it under `target/`, and return
/// the absolute path written.
fn write_html_report(
    suite_label: &str,
    model_label: &str,
    runs: &[CaseRun],
) -> std::path::PathBuf {
    let total = runs.len();
    let passed = runs.iter().filter(|r| r.passed()).count();
    let total_ms: u128 = runs.iter().map(|r| r.latency_ms).sum();
    let all_pass = passed == total;

    let mut body = String::new();

    // Header.
    body.push_str(&format!(
        r#"<header class="report-header {hdr_class}">
  <h1>Agent Capability Report</h1>
  <div class="meta">
    <span class="pill">suite: <b>{suite}</b></span>
    <span class="pill">model: <b>{model}</b></span>
    <span class="pill">generated: <b>{ts}</b></span>
    <span class="pill {hdr_class}">{passed} / {total} passed</span>
    <span class="pill">total: <b>{total_ms} ms</b></span>
  </div>
</header>
"#,
        hdr_class = if all_pass { "ok" } else { "fail" },
        suite = html_escape(suite_label),
        model = html_escape(model_label),
        ts = html_escape(&now_timestamp()),
        passed = passed,
        total = total,
        total_ms = total_ms,
    ));

    // Sort: failures first so they're impossible to miss.
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by_key(|&i| runs[i].passed());

    for &i in &order {
        let run = &runs[i];
        let (tools, final_text, end_error) = extract_trace(&run.events);
        let card_class = if run.passed() { "card ok" } else { "card fail" };
        let badge = if run.passed() { "PASS" } else { "FAIL" };
        let badge_class = if run.passed() { "badge-pass" } else { "badge-fail" };

        body.push_str(&format!(
            r#"<section class="{card_class}">
  <div class="card-head">
    <span class="badge {badge_class}">{badge}</span>
    <h2>{name}</h2>
    <span class="latency">{latency} ms</span>
    <span class="modetag">{mode} path</span>
  </div>
"#,
            card_class = card_class,
            badge_class = badge_class,
            badge = badge,
            name = html_escape(run.name),
            latency = run.latency_ms,
            mode = html_escape(run.mode),
        ));

        // Task.
        let mock_note = if run.mode == "mock" {
            r#" <em class="note">(mock path: the scripted MockLanguageModel injected the tool call directly; this prompt is the intent only)</em>"#
        } else {
            ""
        };
        body.push_str(&format!(
            r#"  <div class="block"><div class="label">Task prompt{mock_note}</div><pre class="prompt">{task}</pre></div>
"#,
            mock_note = mock_note,
            task = html_escape(run.task),
        ));

        // Assertion.
        body.push_str(&format!(
            r#"  <div class="block"><div class="label">Assertion (what we check)</div><div class="assertion">{assertion}</div></div>
"#,
            assertion = html_escape(run.assertion),
        ));

        // Trace.
        body.push_str(r#"  <div class="block"><div class="label">Execution trace</div>"#);
        if tools.is_empty() {
            body.push_str(r#"<div class="empty">No tool executions captured.</div>"#);
        } else {
            for (n, t) in tools.iter().enumerate() {
                let err_tag = if t.is_error {
                    r#"<span class="err">is_error=true</span>"#
                } else {
                    r#"<span class="okk">is_error=false</span>"#
                };
                body.push_str(&format!(
                    r#"<div class="tool">
    <div class="tool-head">#{n} <code>{name}</code> {err_tag}</div>
    <div class="sub">input</div><pre>{input}</pre>
    <div class="sub">output ({olen} chars)</div><pre>{output}</pre>
  </div>"#,
                    n = n + 1,
                    name = html_escape(&t.name),
                    err_tag = err_tag,
                    input = truncate_for_display(&t.input_pretty, 4000),
                    olen = t.output_len,
                    output = truncate_for_display(&t.output_text, 4000),
                ));
            }
        }
        if let Some(text) = &final_text {
            body.push_str(&format!(
                r#"<div class="sub">final assistant message</div><pre class="finaltext">{}</pre>"#,
                truncate_for_display(text, 4000)
            ));
        }
        if let Some(err) = &end_error {
            body.push_str(&format!(
                r#"<div class="sub err">AgentEnd error</div><pre class="err">{}</pre>"#,
                truncate_for_display(err, 4000)
            ));
        }
        body.push_str("</div>\n");

        // Verdict.
        let verdict_html = match &run.verdict {
            Ok(()) => r#"<span class="okk">PASS — all assertions held.</span>"#.to_string(),
            Err(detail) => format!(r#"<span class="err">FAIL — {}</span>"#, html_escape(detail)),
        };
        body.push_str(&format!(
            r#"  <div class="block"><div class="label">Verdict</div><div class="verdict">{verdict_html}</div></div>
</section>
"#,
            verdict_html = verdict_html,
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Agent Capability Report — {suite} / {model}</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
         margin: 0; padding: 0 0 48px; background: #f5f6f8; color: #1c1e21; }}
  .report-header {{ padding: 24px 32px; color: #fff; }}
  .report-header.ok {{ background: linear-gradient(135deg,#1a7f37,#2da44e); }}
  .report-header.fail {{ background: linear-gradient(135deg,#a40e26,#cf222e); }}
  .report-header h1 {{ margin: 0 0 12px; font-size: 22px; }}
  .meta {{ display: flex; flex-wrap: wrap; gap: 8px; }}
  .pill {{ background: rgba(255,255,255,.18); border-radius: 999px; padding: 4px 12px; font-size: 13px; }}
  .pill.ok {{ background: rgba(255,255,255,.30); }}
  .pill.fail {{ background: rgba(0,0,0,.25); }}
  .card {{ background: #fff; margin: 18px 32px; border-radius: 10px; padding: 18px 20px;
          box-shadow: 0 1px 3px rgba(0,0,0,.12); border-left: 6px solid #2da44e; }}
  .card.fail {{ border-left-color: #cf222e; box-shadow: 0 0 0 2px #cf222e33, 0 1px 3px rgba(0,0,0,.2); }}
  .card-head {{ display: flex; align-items: center; gap: 12px; }}
  .card-head h2 {{ margin: 0; font-size: 18px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }}
  .badge {{ font-weight: 700; font-size: 12px; padding: 3px 10px; border-radius: 6px; color: #fff; }}
  .badge-pass {{ background: #2da44e; }}
  .badge-fail {{ background: #cf222e; }}
  .latency {{ margin-left: auto; color: #57606a; font-size: 13px; }}
  .modetag {{ color: #57606a; font-size: 12px; font-style: italic; }}
  .block {{ margin-top: 14px; }}
  .label {{ font-size: 12px; text-transform: uppercase; letter-spacing: .04em; color: #57606a; margin-bottom: 4px; font-weight: 600; }}
  .note {{ color: #8250df; font-style: italic; font-size: 12px; text-transform: none; letter-spacing: 0; }}
  .assertion {{ background: #fff8e6; border: 1px solid #f0e2b6; padding: 8px 12px; border-radius: 6px; font-size: 14px; }}
  pre {{ background: #0d1117; color: #e6edf3; padding: 12px 14px; border-radius: 6px; overflow-x: auto;
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12.5px; line-height: 1.45; white-space: pre-wrap; word-break: break-word; }}
  pre.prompt {{ background: #eef2ff; color: #1c1e21; border: 1px solid #d0d7de; }}
  pre.finaltext {{ background: #f0fff4; color: #1c1e21; border: 1px solid #b6e3c0; }}
  .tool {{ margin: 10px 0; border: 1px solid #d0d7de; border-radius: 8px; padding: 10px 12px; }}
  .tool-head {{ font-size: 14px; margin-bottom: 6px; }}
  .tool-head code {{ background: #eaeef2; padding: 2px 6px; border-radius: 4px; }}
  .sub {{ font-size: 11px; text-transform: uppercase; color: #57606a; margin: 8px 0 2px; font-weight: 600; }}
  .err {{ color: #cf222e; font-weight: 600; }}
  .okk {{ color: #1a7f37; font-weight: 600; }}
  .empty {{ color: #8c959f; font-style: italic; }}
  .verdict {{ font-size: 14px; }}
</style>
</head>
<body>
{body}
</body>
</html>
"#,
        suite = html_escape(suite_label),
        model = html_escape(model_label),
        body = body,
    );

    // target/ is the workspace target dir; tests run with CWD at the crate
    // root, so ../../target resolves to the workspace target. Fall back to
    // the crate-local path if that doesn't exist.
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("../../target");
    let dir = dir.canonicalize().unwrap_or_else(|_| {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("target");
        let _ = std::fs::create_dir_all(&p);
        p
    });
    let file = dir.join(format!(
        "capability-report-{}-{}.html",
        slugify(suite_label),
        slugify(model_label),
    ));
    std::fs::write(&file, html).expect("failed to write HTML report");
    file
}

/// Print the concise stderr ✅/❌ summary and return whether all passed.
fn print_stderr_summary(header: &str, runs: &[CaseRun]) -> bool {
    eprintln!("\n===== {header} =====");
    let mut all_passed = true;
    for run in runs {
        match &run.verdict {
            Ok(()) => eprintln!("✅ {} ({} ms)", run.name, run.latency_ms),
            Err(detail) => {
                all_passed = false;
                eprintln!("❌ {} — {} ({} ms)", run.name, detail, run.latency_ms);
            }
        }
    }
    eprintln!(
        "----- {} / {} passed -----",
        runs.iter().filter(|r| r.passed()).count(),
        runs.len()
    );
    all_passed
}

// ===========================================================================
// MOCK SUITE — deterministic, no API key.
// ===========================================================================

#[tokio::test]
async fn mock_capability_suite() {
    let mut runs: Vec<CaseRun> = Vec::new();

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
        let start = std::time::Instant::now();
        let rx = agent.prompt_text(case.task);

        // Guard against a hang (e.g. an approval that never resolves) so the
        // suite reports ❌ instead of blocking forever.
        let (events, verdict) = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            collect_events(rx),
        )
        .await
        {
            Ok(ev) => {
                let v = (case.check)(&ws, &ev);
                (ev, v)
            }
            Err(_) => (
                Vec::new(),
                Err("TIMEOUT — agent never reached AgentEnd (tool stuck at HITL?)".to_string()),
            ),
        };

        runs.push(CaseRun {
            name: case.name,
            task: case.task,
            assertion: case.assertion,
            mode: "mock",
            events,
            verdict,
            latency_ms: start.elapsed().as_millis(),
        });
    }

    let suite_label = "mock";
    let all_passed =
        print_stderr_summary("MOCK capability suite (MockLanguageModel)", &runs);
    let report = write_html_report(suite_label, "mock", &runs);
    eprintln!("HTML report: {}", report.display());

    assert!(all_passed, "mock capability suite had failures — see report above / HTML report");
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

    let mut runs: Vec<CaseRun> = Vec::new();

    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        (case.setup)(&ws);

        let agent = build_mini_agent(&ws, make_model()).await;
        let start = std::time::Instant::now();
        let rx = agent.prompt_text(case.task);

        let (events, verdict) = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            collect_events(rx),
        )
        .await
        {
            Ok(ev) => {
                let v = (case.check)(&ws, &ev);
                (ev, v)
            }
            Err(_) => (
                Vec::new(),
                Err("TIMEOUT (120s) waiting on live model".to_string()),
            ),
        };

        runs.push(CaseRun {
            name: case.name,
            task: case.task,
            assertion: case.assertion,
            mode: "real",
            events,
            verdict,
            latency_ms: start.elapsed().as_millis(),
        });
    }

    let header = format!("REAL capability suite (model: {})", model_name);
    let all_passed = print_stderr_summary(&header, &runs);
    let report = write_html_report("real", &model_name, &runs);
    eprintln!("HTML report: {}", report.display());

    assert!(
        all_passed,
        "real capability suite had failures against model `{model_name}` — see report above / HTML report"
    );
}
