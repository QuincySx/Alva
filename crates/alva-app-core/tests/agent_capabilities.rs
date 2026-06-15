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
//! The agent assembly here goes through `alva_app_core::components::
//! apply_components` — the SAME assembly switchboard the CLI/Tauri use (one
//! source of truth, no hand-copied `.plugin()/.middleware()` mirror). The mock
//! suite and register assertion build with `default_toggles()` (catalog
//! `default_on`); the real suite can build with ANY component subset via the
//! `ALVA_TEST_COMPONENTS` env (default / `all` / `core,shell,…`) so it can
//! measure tool-set-size vs model accuracy. The component set used for a run is
//! recorded in the report JSON (`components` / `component_count`) for A/B.
//! The only substrate wired outside the catalog is the ApprovalPlugin (the REPL
//! needs its `approval_rx`). Approvals are auto-resolved (see `build_mini_agent`).

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

/// Build a BaseAgent via [`apply_components`] — the SAME assembly switchboard
/// the CLI/Tauri use (single source of truth), driven by `toggles`:
///   approval substrate (always, wired here for its `approval_rx`) + every
///   component in `alva_app_core::components::COMPONENTS` that `toggles` leaves
///   enabled. Passing `default_toggles()` (an empty map) selects exactly the
///   catalog `default_on` set — core/shell + 3 hygiene mw, permission +
///   compaction, skills + web, provider-registry/tool-lock infra, task/team/
///   sub-agents, hooks, checkpoint, interaction — which covers every tool the
///   register-assertion test pins. Any other `toggles` (subset / `all`) lets
///   callers A/B "tool-set size vs model accuracy".
///
/// `ComponentContext` here has `provider_registry = None` (tests have no real
/// provider; the `provider-registry` component gracefully skips and sub-agents
/// degrade to the main model — the `agent` tool is still registered) and
/// `skills = Some((<ws>/.alva/skills, None))` (empty tree, cleaned with the
/// tempdir).
///
/// Dangerous tools (create_file / file_edit / execute_shell) route through the
/// security middleware's HITL path; the background task below auto-resolves each
/// request as `AllowOnce` via the bus-published `SecurityGuard` (same mechanism
/// the e2e suite uses), so the suite runs unattended. If a tool ever hangs here,
/// it means an approval was not resolved — a real finding, not a harness bug.
async fn build_mini_agent(
    workspace: &Path,
    model: Arc<dyn LanguageModel>,
    toggles: &alva_app_core::components::ComponentToggles,
) -> BaseAgent {
    let (approval_ext, mut approval_rx) =
        alva_app_core::extension::ApprovalPlugin::with_channel();

    let ctx = alva_app_core::components::ComponentContext {
        workspace: workspace.to_path_buf(),
        // No real provider in tests: provider-registry skips, sub-agents degrade.
        provider_registry: None,
        // Empty skills tree under the tempdir (cleaned with it); bundled = None.
        skills: Some((workspace.join(".alva/skills"), None)),
        mcp_config_paths: vec![],
        subagent_depth: 3,
        hooks_settings: alva_app_core::settings::HooksSettings::default(),
        subprocess_ext_dirs: vec![],
    };

    let builder = alva_app_core::components::apply_components(
        BaseAgent::builder()
            .workspace(workspace)
            .system_prompt(
                "You are a helpful coding assistant. You have access to tools for \
                 running shell commands, reading/writing files, and searching code. \
                 Use tools when needed to accomplish the user's task. Be concise.",
            )
            .max_iterations(20)
            // substrate: ApprovalPlugin is not in the catalog (the REPL needs its
            // `approval_rx`), so it is wired manually before apply_components.
            .plugin(Box::new(approval_ext)),
        toggles,
        &ctx,
    );

    let agent = builder
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
// Component-toggle selection (which components the harness builds with).
// ---------------------------------------------------------------------------

/// The default component set: an empty toggle map, so every component falls back
/// to its catalog `default_on`. This is what the mock suite and the register
/// assertion build with (it covers every tool they pin).
fn default_toggles() -> alva_app_core::components::ComponentToggles {
    alva_app_core::components::ComponentToggles::new()
}

/// Resolve the component toggles for the REAL suite from `ALVA_TEST_COMPONENTS`:
///   - unset / empty → `default_toggles()` (catalog `default_on`).
///   - `all`         → every catalog id forced ON (maximal tool-set).
///   - `core,shell,…`→ EXACT subset: every catalog id forced OFF, then only the
///                     listed ids forced ON. Lets you measure "model accuracy
///                     when given only these tools".
fn toggles_from_env() -> alva_app_core::components::ComponentToggles {
    use alva_app_core::components::COMPONENTS;
    match std::env::var("ALVA_TEST_COMPONENTS") {
        Ok(s) if !s.trim().is_empty() => {
            let s = s.trim();
            if s.eq_ignore_ascii_case("all") {
                return COMPONENTS.iter().map(|m| (m.id.to_string(), true)).collect();
            }
            let wanted: std::collections::HashSet<&str> =
                s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
            COMPONENTS
                .iter()
                .map(|m| (m.id.to_string(), wanted.contains(m.id)))
                .collect()
        }
        _ => default_toggles(),
    }
}

/// The sorted list of component ids actually enabled under `toggles`. Recorded
/// in the run report so a reader can tell which configuration produced a run
/// (and A/B different `ALVA_TEST_COMPONENTS` configs).
fn enabled_component_ids(
    toggles: &alva_app_core::components::ComponentToggles,
) -> Vec<String> {
    let mut ids: Vec<String> = alva_app_core::components::COMPONENTS
        .iter()
        .filter(|m| alva_app_core::components::is_on(toggles, m))
        .map(|m| m.id.to_string())
        .collect();
    ids.sort();
    ids
}

// ---------------------------------------------------------------------------
// P3/P4 wiring assertion — Skills + Web + Task/Team/SubAgent registered.
// ---------------------------------------------------------------------------

/// Some plugins expose tools that can't be exercised deterministically in the
/// mock suite, so we pin the cheap no-network invariant instead: building the
/// mini agent REGISTERS each plugin's tools (proving they're wired exactly like
/// the CLI build_agent).
///
/// Not executed here, by category:
///   - `internet_search` / `read_url` (WebPlugin): hit live endpoints.
///     `read_url`'s real HTTP path is already covered deterministically via
///     wiremock in `e2e_tool_coverage.rs::stage2_read_url_fetches_from_wiremock_server`;
///     `internet_search` is only meaningful under the REAL suite (real model +
///     configured search backend).
///   - `search_skills` / `use_skill` (SkillsPlugin): need a populated skill tree.
///   - `agent` (SubAgentPlugin, registered late via `finalize`): actually
///     spawning a sub-agent is non-deterministic (needs ProviderRegistry +
///     real model); the spawn-execution test is left to the real suite / a
///     follow-up. Here we only assert the `agent` tool is registered. In this
///     harness ProviderRegistry is omitted, so `finalize` degrades to the main
///     model — the tool is still registered.
#[tokio::test]
async fn mini_agent_registers_p3_p4_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().canonicalize().unwrap();
    let model = MockLanguageModel::new().with_response(make_assistant_message("noop"));
    let agent = build_mini_agent(&ws, Arc::new(model), &default_toggles()).await;

    let names = agent.tool_names();
    // P3: Web + Skills.
    // P4: Task (task_*), Team (team_*/send_message), SubAgent (`agent`).
    for expected in [
        "internet_search",
        "read_url",
        "search_skills",
        "use_skill",
        "task_create",
        "task_list",
        "task_get",
        "task_update",
        "task_output",
        "task_stop",
        "team_create",
        "team_delete",
        "send_message",
        "agent",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "P3/P4 wiring: mini agent should register `{expected}`. Got: {names:?}"
        );
    }
    // Sanity: the P1/P2 local tools are still present alongside the new ones.
    for expected in ["create_file", "read_file", "execute_shell"] {
        assert!(
            names.iter().any(|n| n == expected),
            "regression: local tool `{expected}` missing after P3/P4. Got: {names:?}"
        );
    }
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
    /// Stable case name (and report row label). Unique across ALL cases.
    name: &'static str,
    /// Grouping key for the report — typically the tool/category under test
    /// (e.g. "create_file", "execute_shell", "search"). Many cases may share
    /// one group; the report aggregates + collapses by it.
    group: &'static str,
    /// Free-form filter tags (e.g. ["fs","write"], ["edge-case"]).
    tags: &'static [&'static str],
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
            group: "create_file",
            tags: &["fs", "write"],
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
            group: "read_file",
            tags: &["fs", "read"],
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
            group: "file_edit",
            tags: &["fs", "write", "edit"],
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
            group: "list_files",
            tags: &["fs", "read", "search"],
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
            group: "find_files",
            tags: &["fs", "search", "glob"],
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
            group: "grep_search",
            tags: &["fs", "search", "grep"],
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
            group: "execute_shell",
            tags: &["shell", "exec"],
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
    group: &'static str,
    tags: &'static [&'static str],
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

/// Build the runtime JSON for ONE case run (data layer). The trace is pulled
/// straight from the raw `AgentEvent` stream; `input` stays a real JSON value
/// (not a pre-stringified blob) so machine diffs across models are precise.
fn case_to_json(run: &CaseRun) -> serde_json::Value {
    let mut trace: Vec<serde_json::Value> = Vec::new();
    let mut final_text: Option<String> = None;
    let mut end_error: Option<String> = None;

    for ev in &run.events {
        match ev {
            AgentEvent::ToolExecutionEnd { tool_call, result } => {
                trace.push(serde_json::json!({
                    "kind": "tool",
                    "tool": tool_call.name,
                    "input": tool_call.arguments,
                    "output": result.model_text(),
                    "is_error": result.is_error,
                }));
            }
            AgentEvent::MessageEnd { message } => {
                // Last non-empty assistant text wins. Standard/Steering/FollowUp
                // each wrap a Message; Marker/Extension carry no role text.
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

    serde_json::json!({
        "name": run.name,
        "group": run.group,
        "tags": run.tags,
        "task": run.task,
        "assertion": run.assertion,
        "mode": run.mode,
        "verdict": if run.passed() { "pass" } else { "fail" },
        "detail": run.verdict.as_ref().err().cloned().unwrap_or_default(),
        "latency_ms": run.latency_ms,
        "trace": trace,
        "final_text": final_text,
        "end_error": end_error,
    })
}

/// Assemble the full runtime report JSON (data layer).
///
/// `components` is the sorted list of component ids actually enabled for this
/// run (and `component_count` its length) so the report records *which*
/// configuration produced these results — enabling tool-set-size vs accuracy
/// A/B across runs.
fn build_report_json(
    suite_label: &str,
    model_label: &str,
    components: &[String],
    runs: &[CaseRun],
) -> serde_json::Value {
    let total = runs.len();
    let passed = runs.iter().filter(|r| r.passed()).count();
    let duration_ms: u128 = runs.iter().map(|r| r.latency_ms).sum();
    let timestamp_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Per-group passed/total, in first-seen order (front-end can also recompute
    // from `cases`, but pre-aggregating keeps JSON diffs at the group level).
    let mut group_order: Vec<&str> = Vec::new();
    let mut group_stats: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();
    for r in runs {
        let e = group_stats.entry(r.group).or_insert_with(|| {
            group_order.push(r.group);
            (0, 0)
        });
        e.1 += 1;
        if r.passed() {
            e.0 += 1;
        }
    }
    let groups: Vec<serde_json::Value> = group_order
        .iter()
        .map(|g| {
            let (gp, gt) = group_stats[g];
            serde_json::json!({ "group": g, "passed": gp, "total": gt })
        })
        .collect();

    serde_json::json!({
        "suite": suite_label,
        "model": model_label,
        "timestamp_unix": timestamp_unix,
        "components": components,
        "component_count": components.len(),
        "summary": {
            "passed": passed,
            "total": total,
            "duration_ms": duration_ms,
            "groups": groups,
        },
        "cases": runs.iter().map(case_to_json).collect::<Vec<_>>(),
    })
}

/// Sanitize a label into a filename-safe token (replace `/`, spaces, etc.).
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' })
        .collect()
}

/// Resolve `<crate>/tests/reports` (stable regardless of test CWD).
fn reports_dir() -> std::path::PathBuf {
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("tests/reports");
    dir
}

/// Format a unix timestamp (UTC) as a `YYYYMMDD-HHMMSS` filename slug.
/// Self-contained calendar math (no chrono dep needed for filenames).
fn timestamp_slug(unix: u64) -> String {
    let secs = unix % 60;
    let mins = (unix / 60) % 60;
    let hours = (unix / 3600) % 24;
    let mut days = unix / 86_400; // days since 1970-01-01

    let mut year = 1970u64;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let ydays = if leap { 366 } else { 365 };
        if days >= ydays {
            days -= ydays;
            year += 1;
        } else {
            break;
        }
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while days >= mdays[month] {
        days -= mdays[month];
        month += 1;
    }
    let day = days + 1;
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year,
        month + 1,
        day,
        hours,
        mins,
        secs
    )
}

/// Write a timestamped, never-overwritten run JSON into `tests/reports/`, then
/// regenerate `index.json` (newest-first list of all runs). Returns the run
/// JSON path. The committed `viewer.html` reads these at view time.
fn write_run_report(
    suite_label: &str,
    model_label: &str,
    components: &[String],
    runs: &[CaseRun],
) -> std::path::PathBuf {
    let report = build_report_json(suite_label, model_label, components, runs);
    let json_str = serde_json::to_string_pretty(&report).expect("report JSON must serialize");

    let dir = reports_dir();
    std::fs::create_dir_all(&dir).expect("failed to create tests/reports dir");

    let unix = report
        .get("timestamp_unix")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let run_path = dir.join(format!(
        "run-{}-{}-{}.json",
        timestamp_slug(unix),
        slugify(suite_label),
        slugify(model_label),
    ));
    std::fs::write(&run_path, &json_str).expect("failed to write run JSON");

    regenerate_index(&dir);
    regenerate_data_js(&dir);
    run_path
}

/// Max number of recent runs embedded into `data.js` (older runs stay archived
/// as `run-*.json` and remain loadable via the viewer's file picker).
const DATA_JS_MAX_RUNS: usize = 50;

/// Regenerate `data.js` — a plain JS file the viewer loads via `<script src>`,
/// which (unlike `fetch`) is NOT blocked under `file://`. So double-clicking
/// `viewer.html` works with no local server. Embeds the newest
/// `DATA_JS_MAX_RUNS` run objects (full data), newest-first.
fn regenerate_data_js(dir: &std::path::Path) {
    // Collect (timestamp, filename, full parsed run) for every run-*.json.
    let mut runs: Vec<(u64, String, serde_json::Value)> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for ent in read.flatten() {
            let name = ent.file_name().to_string_lossy().to_string();
            if !(name.starts_with("run-") && name.ends_with(".json")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(ent.path()) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            let ts = v.get("timestamp_unix").and_then(|x| x.as_u64()).unwrap_or(0);
            runs.push((ts, name, v));
        }
    }
    // Newest first; tie-break by filename desc so same-second runs stay stable.
    runs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    runs.truncate(DATA_JS_MAX_RUNS);

    let array: Vec<serde_json::Value> = runs.into_iter().map(|(_, _, v)| v).collect();
    let json = serde_json::to_string(&serde_json::Value::Array(array))
        .unwrap_or_else(|_| "[]".into());
    let contents = format!(
        "// Auto-generated by `cargo test -p alva-app-core --test agent_capabilities`.\n\
         // Loaded by viewer.html via <script src> so file:// works without a server.\n\
         window.CAPABILITY_RUNS = {json};\n"
    );
    let _ = std::fs::write(dir.join("data.js"), contents);
}

/// Scan `tests/reports/` for `run-*.json`, read each header, and write
/// `index.json` as a newest-first array the viewer can list.
fn regenerate_index(dir: &std::path::Path) {
    let mut entries: Vec<serde_json::Value> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for ent in read.flatten() {
            let name = ent.file_name().to_string_lossy().to_string();
            if !(name.starts_with("run-") && name.ends_with(".json")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(ent.path()) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            entries.push(serde_json::json!({
                "file": name,
                "suite": v.get("suite").and_then(|x| x.as_str()).unwrap_or(""),
                "model": v.get("model").and_then(|x| x.as_str()).unwrap_or(""),
                "timestamp_unix": v.get("timestamp_unix").and_then(|x| x.as_u64()).unwrap_or(0),
                "passed": v.pointer("/summary/passed").and_then(|x| x.as_u64()).unwrap_or(0),
                "total": v.pointer("/summary/total").and_then(|x| x.as_u64()).unwrap_or(0),
            }));
        }
    }
    // Newest first; tie-break by filename desc so same-second runs stay stable.
    entries.sort_by(|a, b| {
        let ta = a.get("timestamp_unix").and_then(|x| x.as_u64()).unwrap_or(0);
        let tb = b.get("timestamp_unix").and_then(|x| x.as_u64()).unwrap_or(0);
        tb.cmp(&ta).then_with(|| {
            let fa = a.get("file").and_then(|x| x.as_str()).unwrap_or("");
            let fb = b.get("file").and_then(|x| x.as_str()).unwrap_or("");
            fb.cmp(fa)
        })
    });
    let index = serde_json::Value::Array(entries);
    let _ = std::fs::write(
        dir.join("index.json"),
        serde_json::to_string_pretty(&index).unwrap_or_else(|_| "[]".into()),
    );
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
    // Mock suite always runs the default (catalog default_on) component set.
    let toggles = default_toggles();
    let components = enabled_component_ids(&toggles);
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

        let agent = build_mini_agent(&ws, Arc::new(model), &toggles).await;
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
            group: case.group,
            tags: case.tags,
            task: case.task,
            assertion: case.assertion,
            mode: "mock",
            events,
            verdict,
            latency_ms: start.elapsed().as_millis(),
        });
    }

    let all_passed =
        print_stderr_summary("MOCK capability suite (MockLanguageModel)", &runs);
    let run_path = write_run_report("mock", "MockLanguageModel", &components, &runs);
    eprintln!(
        "report run: {}\nopen viewer: double-click crates/alva-app-core/tests/reports/viewer.html (reads data.js, no server needed)",
        run_path.display()
    );

    assert!(all_passed, "mock capability suite had failures — see report above / report run JSON");
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

    // Component subset for this run, from ALVA_TEST_COMPONENTS (default / `all` /
    // exact subset). Recorded in the report so runs are A/B-comparable.
    let toggles = toggles_from_env();
    let components = enabled_component_ids(&toggles);
    eprintln!(
        "real_capability_suite components ({}): {}",
        components.len(),
        components.join(", ")
    );

    let mut runs: Vec<CaseRun> = Vec::new();

    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().canonicalize().unwrap();
        (case.setup)(&ws);

        let agent = build_mini_agent(&ws, make_model(), &toggles).await;
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
            group: case.group,
            tags: case.tags,
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
    let run_path = write_run_report("real", &model_name, &components, &runs);
    eprintln!(
        "report run: {}\nopen viewer: double-click crates/alva-app-core/tests/reports/viewer.html (reads data.js, no server needed)",
        run_path.display()
    );

    assert!(
        all_passed,
        "real capability suite had failures against model `{model_name}` — see report above / report run JSON"
    );
}
