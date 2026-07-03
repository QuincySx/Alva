// INPUT:  BaseAgent, apply_components, MockLanguageModel, real providers, AgentEvent
// OUTPUT: mock_capability_suite, real_capability_suite, report JSON/data.js helpers
// POS:    Dual-path built-in capability regression harness with opt-in report generation.
//! Dual-path agent capability regression harness.
//!
//! A single batch of "capability cases" (single-tool coverage plus cross-tool
//! boundary workflows) is run two ways from the SAME case definitions:
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
//! `cargo test -- --nocapture` to see it) and fail if any case fails. With
//! `ALVA_WRITE_CAPABILITY_REPORT=1` (or `ALVA_CAPABILITY_REPORT_DIR=<dir>`) they
//! write a full viewer report plus a compact `latest-agent-summary.json`.
//! `real_capability_suite` also supports `ALVA_TEST_REPEAT=N` so one command
//! can sample model stability instead of relying on a single stochastic run.
//!
//! Coverage spans every tool the mini agent registers, then adds boundary cases
//! for multi-step planning, error recovery, search→edit verification, and path
//! handling. Cases split two ways:
//!   - Deterministic cases run in BOTH suites (file ops, shell, task_create/
//!     task_list, team_create/team_delete, send_message, search_skills/use_skill).
//!   - `real_only` cases run ONLY in the real suite, because they can't be
//!     scripted up front: tools keyed by a runtime-minted id (task_get/update/
//!     output/stop need the id task_create returns), live-network tools
//!     (internet_search / read_url), and sub-agent spawn (`agent`). A live model
//!     chains these naturally (create → read id back → use it); the mock suite
//!     skips them. `ask_human` is the lone tool with no execution case at all —
//!     it blocks on a human, so only its registration is pinned (see
//!     `mini_agent_registers_p3_p4_tools`).
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
use alva_kernel_abi::{ContentBlock, LanguageModel, Message, MessageRole, ToolCall, ToolOutput};
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
    build_mini_agent_with_timeout(
        workspace,
        model,
        toggles,
        alva_app_core::components::DEFAULT_SUBAGENT_TIMEOUT,
    )
    .await
}

/// Same as [`build_mini_agent`] but with a caller-chosen sub-agent wall-clock
/// budget — the timeout regression uses a short fuse.
async fn build_mini_agent_with_timeout(
    workspace: &Path,
    model: Arc<dyn LanguageModel>,
    toggles: &alva_app_core::components::ComponentToggles,
    subagent_timeout: std::time::Duration,
) -> BaseAgent {
    let (agent, mut approval_rx) =
        build_mini_agent_inner(workspace, model, toggles, subagent_timeout).await;

    // Auto-approve every approval request via the bus-published SecurityGuard.
    let bus = agent.bus().clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            if let Some(guard) = bus.get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
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

/// Innermost harness: builds the agent and hands the approval stream back to
/// the caller UNCONSUMED, so tests can observe (and decide on) each HITL
/// request instead of blanket auto-approving.
async fn build_mini_agent_inner(
    workspace: &Path,
    model: Arc<dyn LanguageModel>,
    toggles: &alva_app_core::components::ComponentToggles,
    subagent_timeout: std::time::Duration,
) -> (
    BaseAgent,
    tokio::sync::mpsc::UnboundedReceiver<alva_host_native::middleware::ApprovalRequest>,
) {
    let (approval_ext, approval_rx) = alva_app_core::extension::ApprovalPlugin::with_channel();

    let ctx = alva_app_core::components::ComponentContext {
        workspace: workspace.to_path_buf(),
        // No real provider in tests: provider-registry skips, sub-agents degrade.
        provider_registry: None,
        // Empty skills tree under the tempdir (cleaned with it); bundled = None.
        skills: Some((workspace.join(".alva/skills"), None)),
        mcp_config_paths: vec![],
        subagent_depth: 3,
        subagent_timeout,
        agent_templates: vec![],
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

    (agent, approval_rx)
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

/// End-to-end wiring proof (no API key): assembling the agent with the
/// built-in `video` template publishes (a) the spawn tool, (b) the template
/// registry on the bus listing `video`, and (c) the AgentTemplateService
/// (phase 2 — skill injection). This is the full chain CLI/Tauri use.
#[tokio::test]
async fn video_template_wires_into_spawn_and_skill_service() {
    use alva_app_core::extension::agent_spawn::AgentTemplateRegistry;
    use alva_app_core::extension::skills::agent_template_service::AgentTemplateService;

    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().to_path_buf();
    let model = MockLanguageModel::new().with_response(make_assistant_message("noop"));
    let (approval_ext, _rx) = alva_app_core::extension::ApprovalPlugin::with_channel();

    let ctx = alva_app_core::components::ComponentContext {
        workspace: ws.clone(),
        provider_registry: None,
        skills: Some((ws.join(".alva/skills"), None)),
        mcp_config_paths: vec![],
        subagent_depth: 3,
        subagent_timeout: alva_app_core::components::DEFAULT_SUBAGENT_TIMEOUT,
        agent_templates: alva_app_core::extension::agent_templates::builtin_agent_templates(),
        hooks_settings: alva_app_core::settings::HooksSettings::default(),
        subprocess_ext_dirs: vec![],
    };
    let builder = alva_app_core::components::apply_components(
        BaseAgent::builder()
            .workspace(&ws)
            .system_prompt("x")
            .max_iterations(5)
            .plugin(Box::new(approval_ext)),
        &default_toggles(),
        &ctx,
    );
    let agent = builder.build(Arc::new(model)).await.expect("agent builds");

    // (a) spawn tool present
    assert!(
        agent.tool_names().iter().any(|n| n == "agent"),
        "spawn `agent` tool should be registered"
    );
    // (b) template registry on the bus lists the built-in `video`
    let reg = agent
        .bus()
        .get::<dyn AgentTemplateRegistry>()
        .expect("AgentTemplateRegistry published on the bus");
    assert!(
        reg.list().iter().any(|t| t.name == "video"),
        "registry should list the built-in video template"
    );
    // (c) phase 2: skills component published the AgentTemplateService
    assert!(
        agent.bus().get::<AgentTemplateService>().is_some(),
        "AgentTemplateService should be on the bus (skill injection wired)"
    );
}

/// KILL-1 regression: a sub-agent that runs a `serial-global` tool
/// (`execute_shell`) must not deadlock the whole session.
///
/// The parent's `agent` spawn tool defaults to `Parallel` execution, so the
/// scheduler holds the global READ lock for the entire inline child run. The
/// child inherits the same bus → the same `ToolLockRegistry`; its
/// `execute_shell` is `serial-global` and requests the global WRITE lock on
/// the same task that still holds the read guard → deadlock. This is the
/// default component set (`tool-lock` + `sub-agents` are both `default_on`),
/// so it is the default path, not an edge case.
///
/// The whole run is wrapped in a timeout so a regression surfaces as a failed
/// assertion instead of a hung test process.
#[tokio::test]
async fn subagent_running_shell_does_not_deadlock() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();

    // Parent delegates a shell command to a sub-agent, granting it
    // `execute_shell`. Parent and child share this one mock model's response
    // queue (with no ProviderRegistry the child inherits the parent model),
    // drawn in strict inline order: parent spawns → child runs shell → child
    // finishes → parent finishes.
    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "spawn",
            "agent",
            serde_json::json!({
                "task": "run a shell command and report its output",
                "role": "worker",
                "tools": ["execute_shell"],
            }),
        ))
        .with_response(tool_use_message(
            "sh",
            "execute_shell",
            serde_json::json!({ "command": "echo child_ran" }),
        ))
        .with_response(make_assistant_message("child done"))
        .with_response(make_assistant_message("parent done"));

    // Retain a handle so we can inspect how far the scripted conversation got.
    // The mock shares its state across clones (Arc<Mutex>), so calls made
    // through the agent's copy are visible here.
    let model_handle = model.clone();

    let agent = build_mini_agent(ws, Arc::new(model), &default_toggles()).await;

    let rx = agent.prompt_text("Delegate a shell command to a sub-agent.");
    let events = tokio::time::timeout(std::time::Duration::from_secs(20), collect_events(rx))
        .await
        .expect("KILL-1: sub-agent running a serial-global tool deadlocked the session");

    // The full scripted chain must have executed: parent spawns (call 1) →
    // child runs execute_shell (call 2) → child finishes (call 3) → parent
    // finishes (call 4). If the child had deadlocked or bailed before the
    // shell, the queue could not have advanced past call 2.
    assert_eq!(
        model_handle.calls().len(),
        4,
        "expected the full parent→child→shell→done chain (4 model turns)"
    );
    assert!(
        matches!(events.last(), Some(AgentEvent::AgentEnd { error: None })),
        "the parent agent run should end cleanly, got: {:?}",
        events.last()
    );
}

/// KILL-1b regression: cancelling the parent must propagate to a running
/// sub-agent. The child is spawned with a fresh, disconnected
/// `CancellationToken`, so the parent's cancel never reaches it — a child
/// blocked in a cooperative-cancel tool (`sleep`) hangs until its own
/// never-fired token or the 5-minute scope cap, long past any caller's
/// patience.
///
/// We grant the child `sleep`, kick off a 5-minute sleep, then cancel the
/// parent the instant its `agent` tool starts (the child is now running
/// inline). With the token connected the child aborts and the run unwinds;
/// without it the run hangs and the outer timeout fires this assertion.
#[tokio::test]
async fn parent_cancel_propagates_to_subagent() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();

    // `sleep` lives in the (default-off) `utility` component — enable it on
    // top of the defaults so the parent can hand it down to the child.
    let mut toggles = default_toggles();
    toggles.insert("utility".to_string(), true);

    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "spawn",
            "agent",
            serde_json::json!({
                "task": "sleep for a long time",
                "role": "sleeper",
                "tools": ["sleep"],
            }),
        ))
        .with_response(tool_use_message(
            "sleep",
            "sleep",
            serde_json::json!({ "duration_ms": 300000 }),
        ))
        .with_response(make_assistant_message("child done"))
        .with_response(make_assistant_message("parent done"));

    let agent = build_mini_agent(ws, Arc::new(model), &toggles).await;

    let mut rx = agent.prompt_text("Delegate a long sleep to a sub-agent.");

    let drive = async {
        let mut cancelled = false;
        while let Some(ev) = rx.recv().await {
            if !cancelled {
                if let AgentEvent::ToolExecutionStart { tool_call } = &ev {
                    if tool_call.name == "agent" {
                        // Parent is now inside the spawn tool; the child is
                        // running. Cancelling the parent must reach the child.
                        cancelled = true;
                        agent.cancel();
                    }
                }
            }
            if matches!(ev, AgentEvent::AgentEnd { .. }) {
                break;
            }
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(15), drive)
        .await
        .expect("KILL-1b: parent cancel did not propagate to the sub-agent (it hung)");
}

/// KILL-1b(2) regression: the sub-agent wall-clock budget must actually fire.
///
/// The scope timeout is only enforced when a real `Sleeper` reaches
/// `run_child_agent`; with `sleeper: None` the child falls back to
/// `NoopSleeper` and the budget silently never fires — a runaway child (here:
/// a 60s sleep) keeps running with nobody to stop it in headless runs.
///
/// We build the agent with a 200ms sub-agent budget, script the child into a
/// 60-second sleep, and require the run to unwind promptly with the spawn
/// tool reporting the timeout back to the parent: the parent's final model
/// call must carry a "timed out" tool result.
#[tokio::test]
async fn subagent_timeout_fires() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();

    // `sleep` lives in the (default-off) `utility` component.
    let mut toggles = default_toggles();
    toggles.insert("utility".to_string(), true);

    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "spawn",
            "agent",
            serde_json::json!({
                "task": "sleep for a long time",
                "role": "sleeper",
                "tools": ["sleep"],
            }),
        ))
        .with_response(tool_use_message(
            "sleep",
            "sleep",
            serde_json::json!({ "duration_ms": 60000 }),
        ))
        .with_response(make_assistant_message("parent done"));

    let model_handle = model.clone();
    let agent = build_mini_agent_with_timeout(
        ws,
        Arc::new(model),
        &toggles,
        std::time::Duration::from_millis(200),
    )
    .await;

    let rx = agent.prompt_text("Delegate a long sleep to a sub-agent.");
    let events = tokio::time::timeout(std::time::Duration::from_secs(10), collect_events(rx))
        .await
        .expect("KILL-1b(2): sub-agent wall-clock timeout did not fire (child ran unbounded)");

    assert!(
        matches!(events.last(), Some(AgentEvent::AgentEnd { .. })),
        "run should end after the child times out, got: {:?}",
        events.last()
    );
    // parent spawn → child sleep → parent final; the killed child makes no
    // further model calls.
    let calls = model_handle.calls();
    assert_eq!(calls.len(), 3, "parent → child(sleep) → parent-final");
    let final_call = format!("{:?}", calls[2]);
    assert!(
        final_call.contains("timed out"),
        "parent's final call should carry the child's timeout tool result: {final_call}"
    );
}

/// Sub-agents must pass dangerous tools through the same HITL gate as the
/// parent. run_child previously built the child loop with an EMPTY middleware
/// stack, so a child running `execute_shell` (on the default HITL review
/// list) executed without any approval — silently bypassing the security
/// gate the parent enforces for the very same tool.
#[tokio::test]
async fn subagent_dangerous_tool_goes_through_hitl() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();

    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "spawn",
            "agent",
            serde_json::json!({
                "task": "run a shell command and report its output",
                "role": "worker",
                "tools": ["execute_shell"],
            }),
        ))
        .with_response(tool_use_message(
            "sh",
            "execute_shell",
            serde_json::json!({ "command": "echo child_ran" }),
        ))
        .with_response(make_assistant_message("child done"))
        .with_response(make_assistant_message("parent done"));

    let model_handle = model.clone();
    let (agent, mut approval_rx) = build_mini_agent_inner(
        ws,
        Arc::new(model),
        &default_toggles(),
        alva_app_core::components::DEFAULT_SUBAGENT_TIMEOUT,
    )
    .await;

    // Approve every request, recording which tools asked.
    let approved = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let approved_writer = approved.clone();
    let bus = agent.bus().clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            approved_writer.lock().unwrap().push(req.tool_name.clone());
            if let Some(guard) = bus.get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
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

    let rx = agent.prompt_text("Delegate a shell command to a sub-agent.");
    let events = tokio::time::timeout(std::time::Duration::from_secs(20), collect_events(rx))
        .await
        .expect("run hung (an approval was never resolved?)");

    assert!(
        matches!(events.last(), Some(AgentEvent::AgentEnd { error: None })),
        "run should end cleanly, got: {:?}",
        events.last()
    );
    assert_eq!(model_handle.calls().len(), 4, "full chain must execute");
    let approvals = approved.lock().unwrap().clone();
    assert!(
        approvals.iter().any(|t| t == "execute_shell"),
        "the sub-agent's execute_shell must raise an HITL approval request \
         (child middleware gates dangerous tools); approvals seen: {approvals:?}"
    );
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
                return COMPONENTS
                    .iter()
                    .map(|m| (m.id.to_string(), true))
                    .collect();
            }
            let wanted: std::collections::HashSet<&str> = s
                .split(',')
                .map(|x| x.trim())
                .filter(|x| !x.is_empty())
                .collect();
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
fn enabled_component_ids(toggles: &alva_app_core::components::ComponentToggles) -> Vec<String> {
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

/// Cheap wiring invariant: building the mini agent REGISTERS each plugin's
/// tools (proving they're wired exactly like the CLI `build_agent`). This is
/// complementary to `cases()` — every tool below ALSO has an execution case in
/// `cases()` (deterministic in the mock suite for task/team/skills, `real_only`
/// for the live-network / runtime-id / spawn tools). The one exception is
/// `ask_human`: it blocks on a real human response, so it can't be exercised
/// unattended in either suite, and registration is the only invariant we pin.
///
/// (`read_url`'s deterministic HTTP path is additionally covered via wiremock in
/// `e2e_tool_coverage.rs::stage2_read_url_fetches_from_wiremock_server`.)
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
        // InteractionPlugin's `ask_human` blocks on a real human response, so it
        // can't be exercised unattended in EITHER suite — registration is the
        // strongest invariant we can pin for it here.
        "ask_human",
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

fn results_for(events: &[AgentEvent], tool_name: &str) -> Vec<ToolOutput> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionEnd { tool_call, result } if tool_call.name == tool_name => {
                Some(result.clone())
            }
            _ => None,
        })
        .collect()
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
    /// When true, this case runs ONLY in the real suite — never the mock suite.
    /// Used for tools that can't be scripted deterministically: those whose
    /// arguments are a runtime-generated value the mock can't know up front
    /// (task_get/update/output/stop need the `task_id` minted by task_create),
    /// network tools (internet_search / read_url hit live endpoints), and spawn
    /// (`agent` needs a live model to drive the sub-agent). A real model chains
    /// these naturally in one conversation (create → read the id back → use it),
    /// so they're exercised end-to-end there; the mock suite skips them.
    real_only: bool,
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
            real_only: false,
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
            real_only: false,
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
            real_only: false,
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
            real_only: false,
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
            real_only: false,
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
            real_only: false,
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
            real_only: false,
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
        // ── task_create ────────────────────────────────────────────────
        Cap {
            name: "task_create",
            group: "task",
            tags: &["task", "collab"],
            real_only: false,
            task: "Create a task with the subject \"RegressTask\" and the description \
                   \"verify task_create works\".",
            assertion: "Asserts the `task_create` tool ran successfully (is_error=false) AND its \
                        output confirms creation (\"Task created successfully\").",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "task_create",
                    serde_json::json!({
                        "subject": "RegressTask",
                        "description": "verify task_create works",
                    }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "task_create").ok_or("task_create did not run")?;
                if out.is_error {
                    return Err(format!("task_create errored: {}", out.model_text()));
                }
                if !out.model_text().contains("Task created successfully") {
                    return Err(format!("unexpected task_create output: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── task_list (create → list round-trip) ───────────────────────
        Cap {
            name: "task_list",
            group: "task",
            tags: &["task", "collab"],
            real_only: false,
            task: "Create a task titled \"ListMarkerTask\", then list all tracked tasks.",
            assertion: "Asserts the `task_list` tool ran successfully AND its output includes the \
                        task created earlier in the same run (\"ListMarkerTask\"), proving the \
                        service persists and lists tasks.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "task_create",
                        serde_json::json!({
                            "subject": "ListMarkerTask",
                            "description": "seed a task so task_list has something to show",
                        }),
                    ),
                    tool_use_message("2", "task_list", serde_json::json!({})),
                ]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "task_list").ok_or("task_list did not run")?;
                if out.is_error {
                    return Err(format!("task_list errored: {}", out.model_text()));
                }
                if !out.model_text().contains("ListMarkerTask") {
                    return Err(format!(
                        "task_list output missing the created task: {}",
                        out.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── team_create ────────────────────────────────────────────────
        Cap {
            name: "team_create",
            group: "team",
            tags: &["team", "collab"],
            real_only: false,
            task: "Create a multi-agent team named \"RegressTeam\" described as \"team for coverage\".",
            assertion: "Asserts the `team_create` tool ran successfully (is_error=false) AND its \
                        output confirms creation (\"Team created successfully\").",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "team_create",
                    serde_json::json!({
                        "team_name": "RegressTeam",
                        "description": "team for coverage",
                        "agent_type": "code",
                    }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "team_create").ok_or("team_create did not run")?;
                if out.is_error {
                    return Err(format!("team_create errored: {}", out.model_text()));
                }
                if !out.model_text().contains("Team created successfully") {
                    return Err(format!("unexpected team_create output: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── team_delete (create → delete) ──────────────────────────────
        Cap {
            name: "team_delete",
            group: "team",
            tags: &["team", "collab"],
            real_only: false,
            task: "Create a team named \"DeleteTeam\", then delete that same team.",
            assertion: "Asserts the `team_delete` tool ran successfully AND its output confirms the \
                        team was deleted (\"deleted successfully\").",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "team_create",
                        serde_json::json!({
                            "team_name": "DeleteTeam",
                            "description": "a team that will be deleted",
                        }),
                    ),
                    tool_use_message(
                        "2",
                        "team_delete",
                        serde_json::json!({ "team_name": "DeleteTeam" }),
                    ),
                ]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "team_delete").ok_or("team_delete did not run")?;
                if out.is_error {
                    return Err(format!("team_delete errored: {}", out.model_text()));
                }
                if !out.model_text().contains("deleted successfully") {
                    return Err(format!("unexpected team_delete output: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── send_message (create teammate → send) ──────────────────────
        Cap {
            name: "send_message",
            group: "team",
            tags: &["team", "collab", "message"],
            real_only: false,
            task: "Create a teammate named \"worker\", then send it the message \"hello teammate\".",
            assertion: "Asserts the `send_message` tool ran successfully AND its output confirms \
                        delivery (\"Message sent to 'worker'\").",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "team_create",
                        serde_json::json!({
                            "team_name": "worker",
                            "description": "the message recipient",
                        }),
                    ),
                    tool_use_message(
                        "2",
                        "send_message",
                        serde_json::json!({
                            "to": "worker",
                            "message": "hello teammate",
                            "summary": "greeting",
                        }),
                    ),
                ]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "send_message").ok_or("send_message did not run")?;
                if out.is_error {
                    return Err(format!("send_message errored: {}", out.model_text()));
                }
                if !out.model_text().contains("Message sent to 'worker'") {
                    return Err(format!("unexpected send_message output: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── search_skills (seed a skill tree → search) ─────────────────
        Cap {
            name: "search_skills",
            group: "skills",
            tags: &["skills", "search"],
            real_only: false,
            task: "Search the available skills for one matching the keyword \"regress\".",
            assertion: "Asserts the `search_skills` tool ran successfully AND its output includes \
                        the seeded skill id \"regress-skill\".",
            setup: Box::new(|ws| seed_skill(ws)),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "search_skills",
                    serde_json::json!({ "query": "regress" }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "search_skills").ok_or("search_skills did not run")?;
                if out.is_error {
                    return Err(format!("search_skills errored: {}", out.model_text()));
                }
                if !out.model_text().contains("regress-skill") {
                    return Err(format!(
                        "search_skills did not surface the seeded skill: {}",
                        out.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── use_skill (seed an enabled skill → activate) ───────────────
        Cap {
            name: "use_skill",
            group: "skills",
            tags: &["skills", "activate"],
            real_only: false,
            task: "Activate the skill named \"regress-skill\" and report its instructions.",
            assertion: "Asserts the `use_skill` tool ran successfully AND its output contains the \
                        seeded skill's body marker (\"REGRESS_SKILL_BODY\").",
            setup: Box::new(|ws| seed_skill(ws)),
            mock_script: Box::new(|_ws| {
                vec![tool_use_message(
                    "1",
                    "use_skill",
                    serde_json::json!({ "skill_name": "regress-skill" }),
                )]
            }),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "use_skill").ok_or("use_skill did not run")?;
                if out.is_error {
                    return Err(format!("use_skill errored: {}", out.model_text()));
                }
                if !out.model_text().contains("REGRESS_SKILL_BODY") {
                    return Err(format!(
                        "use_skill did not return the skill body: {}",
                        out.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── multi-step file pipeline ───────────────────────────────────
        Cap {
            name: "multi_step_file_pipeline",
            group: "boundary",
            tags: &["boundary", "multi-step", "fs", "shell", "edit"],
            real_only: false,
            task: "Perform this exact tool workflow in order: create notes.txt with the text \
                   alpha, read notes.txt, replace alpha with omega using file_edit, then run \
                   the shell command `cat notes.txt` to verify the final content.",
            assertion: "Asserts the model can drive a continuous create → read → edit → shell \
                        verification workflow, and that the tools preserve the final on-disk \
                        content `omega`.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "create_file",
                        serde_json::json!({ "path": "notes.txt", "content": "alpha" }),
                    ),
                    tool_use_message(
                        "2",
                        "read_file",
                        serde_json::json!({ "path": "notes.txt" }),
                    ),
                    tool_use_message(
                        "3",
                        "file_edit",
                        serde_json::json!({
                            "path": "notes.txt",
                            "old_str": "alpha",
                            "new_str": "omega",
                        }),
                    ),
                    tool_use_message(
                        "4",
                        "execute_shell",
                        serde_json::json!({ "command": "cat notes.txt" }),
                    ),
                ]
            }),
            check: Box::new(|ws, events| {
                for tool in ["create_file", "read_file", "file_edit", "execute_shell"] {
                    if !ran_tool(events, tool) {
                        return Err(format!("{tool} did not run in the multi-step pipeline"));
                    }
                }
                let content =
                    std::fs::read_to_string(ws.join("notes.txt")).map_err(|e| e.to_string())?;
                if content.trim() != "omega" {
                    return Err(format!("final notes.txt content mismatch: {content:?}"));
                }
                let shell = result_for(events, "execute_shell")
                    .ok_or("execute_shell did not produce a result")?;
                if shell.is_error || !shell.model_text().contains("omega") {
                    return Err(format!(
                        "shell verification failed or missed omega: {}",
                        shell.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── search → edit → verify pipeline ────────────────────────────
        Cap {
            name: "grep_edit_verify_pipeline",
            group: "boundary",
            tags: &["boundary", "multi-step", "search", "edit"],
            real_only: false,
            task: "Use grep_search to find the file containing TODO_CAP_MARKER, then use \
                   file_edit to replace TODO_CAP_MARKER with DONE_CAP_MARKER. After editing, \
                   use grep_search again to verify DONE_CAP_MARKER exists.",
            assertion: "Asserts the model can use search output to choose a file, edit that file, \
                        and verify the edit with a second search.",
            setup: Box::new(|ws| {
                std::fs::create_dir_all(ws.join("src")).unwrap();
                std::fs::write(
                    ws.join("src/pipeline.txt"),
                    "before\nTODO_CAP_MARKER\n after\n",
                )
                .unwrap();
            }),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "grep_search",
                        serde_json::json!({ "pattern": "TODO_CAP_MARKER" }),
                    ),
                    tool_use_message(
                        "2",
                        "file_edit",
                        serde_json::json!({
                            "path": "src/pipeline.txt",
                            "old_str": "TODO_CAP_MARKER",
                            "new_str": "DONE_CAP_MARKER",
                        }),
                    ),
                    tool_use_message(
                        "3",
                        "grep_search",
                        serde_json::json!({ "pattern": "DONE_CAP_MARKER" }),
                    ),
                ]
            }),
            check: Box::new(|ws, events| {
                if results_for(events, "grep_search").len() < 2 {
                    return Err("expected grep_search to run before and after edit".into());
                }
                if !ran_tool(events, "file_edit") {
                    return Err("file_edit did not run after grep_search".into());
                }
                let content = std::fs::read_to_string(ws.join("src/pipeline.txt"))
                    .map_err(|e| e.to_string())?;
                if !content.contains("DONE_CAP_MARKER") || content.contains("TODO_CAP_MARKER") {
                    return Err(format!("marker edit not applied correctly: {content:?}"));
                }
                let final_grep = results_for(events, "grep_search")
                    .last()
                    .cloned()
                    .ok_or("missing final grep result")?;
                let final_text = final_grep.model_text();
                if final_grep.is_error
                    || (!final_text.contains("DONE_CAP_MARKER")
                        && !final_text.contains("src/pipeline.txt"))
                {
                    return Err(format!(
                        "final grep did not verify DONE_CAP_MARKER or its file: {final_text}"
                    ));
                }
                Ok(())
            }),
        },
        // ── error recovery: missing file → create → read ───────────────
        Cap {
            name: "missing_file_recovery",
            group: "boundary",
            tags: &["boundary", "error-recovery", "fs"],
            real_only: false,
            task: "First try to read missing_then_create.txt. If it is missing, recover by \
                   creating missing_then_create.txt with exactly recovered-content, then read \
                   missing_then_create.txt again to confirm the content.",
            assertion: "Asserts the model can recover from an expected tool error by taking the \
                        next corrective action, and that read_file/create_file handle the sequence.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "read_file",
                        serde_json::json!({ "path": "missing_then_create.txt" }),
                    ),
                    tool_use_message(
                        "2",
                        "create_file",
                        serde_json::json!({
                            "path": "missing_then_create.txt",
                            "content": "recovered-content",
                        }),
                    ),
                    tool_use_message(
                        "3",
                        "read_file",
                        serde_json::json!({ "path": "missing_then_create.txt" }),
                    ),
                ]
            }),
            check: Box::new(|ws, events| {
                let reads = results_for(events, "read_file");
                if reads.len() < 2 {
                    return Err(format!(
                        "expected read_file to run before and after recovery, got {}",
                        reads.len()
                    ));
                }
                if !reads.iter().any(|out| out.is_error) {
                    return Err("expected the first missing-file read to produce an error".into());
                }
                if !ran_tool(events, "create_file") {
                    return Err("create_file did not run after missing-file error".into());
                }
                let content = std::fs::read_to_string(ws.join("missing_then_create.txt"))
                    .map_err(|e| e.to_string())?;
                if content.trim() != "recovered-content" {
                    return Err(format!("recovered file content mismatch: {content:?}"));
                }
                let final_read = reads.last().expect("checked len");
                if final_read.is_error || !final_read.model_text().contains("recovered-content") {
                    return Err(format!(
                        "final read did not confirm recovered content: {}",
                        final_read.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── path handling: spaces + nested directory ───────────────────
        Cap {
            name: "path_with_spaces",
            group: "boundary",
            tags: &["boundary", "fs", "path"],
            real_only: false,
            task: "Inside the existing directory named `notes with spaces`, create a file named \
                   `final report.txt` with exactly the text spaced-path-ok, then read that same \
                   file back.",
            assertion: "Asserts create_file/read_file handle paths containing spaces and that the \
                        model preserves the exact nested path.",
            setup: Box::new(|ws| {
                std::fs::create_dir_all(ws.join("notes with spaces")).unwrap();
            }),
            mock_script: Box::new(|_ws| {
                vec![
                    tool_use_message(
                        "1",
                        "create_file",
                        serde_json::json!({
                            "path": "notes with spaces/final report.txt",
                            "content": "spaced-path-ok",
                        }),
                    ),
                    tool_use_message(
                        "2",
                        "read_file",
                        serde_json::json!({ "path": "notes with spaces/final report.txt" }),
                    ),
                ]
            }),
            check: Box::new(|ws, events| {
                if !ran_tool(events, "create_file") || !ran_tool(events, "read_file") {
                    return Err("expected create_file and read_file for spaced path".into());
                }
                let target = ws.join("notes with spaces/final report.txt");
                let content = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
                if content.trim() != "spaced-path-ok" {
                    return Err(format!("spaced path file content mismatch: {content:?}"));
                }
                let read = results_for(events, "read_file")
                    .last()
                    .cloned()
                    .ok_or("read_file did not produce a result")?;
                if read.is_error || !read.model_text().contains("spaced-path-ok") {
                    return Err(format!(
                        "read_file did not return spaced path content: {}",
                        read.model_text()
                    ));
                }
                Ok(())
            }),
        },
        // ── task_get (REAL ONLY — needs runtime task_id) ───────────────
        Cap {
            name: "task_get",
            group: "task",
            tags: &["task", "collab", "real-only"],
            real_only: true,
            task: "Create a task with the subject \"Demo\" and a short description, then look up \
                   that task's full details by its ID.",
            assertion: "Asserts the `task_get` tool ran successfully (is_error=false) AND its \
                        output renders task details (contains \"Status:\"). REAL ONLY: the id is \
                        minted at create time, so a live model must chain create → get.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "task_get").ok_or("task_get did not run")?;
                if out.is_error {
                    return Err(format!("task_get errored: {}", out.model_text()));
                }
                if !out.model_text().contains("Status:") {
                    return Err(format!("task_get output missing task details: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── task_update (REAL ONLY — needs runtime task_id) ────────────
        Cap {
            name: "task_update",
            group: "task",
            tags: &["task", "collab", "real-only"],
            real_only: true,
            task: "Create a task, then update that task's status to in_progress.",
            assertion: "Asserts the `task_update` tool ran successfully (is_error=false). REAL \
                        ONLY: requires the runtime-minted task id.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "task_update").ok_or("task_update did not run")?;
                if out.is_error {
                    return Err(format!("task_update errored: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── task_output (REAL ONLY — needs runtime task_id) ────────────
        Cap {
            name: "task_output",
            group: "task",
            tags: &["task", "collab", "real-only"],
            real_only: true,
            task: "Create a task, then retrieve that task's output.",
            assertion: "Asserts the `task_output` tool ran successfully (is_error=false; a fresh \
                        task legitimately has no output yet). REAL ONLY: needs the runtime task id.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "task_output").ok_or("task_output did not run")?;
                if out.is_error {
                    return Err(format!("task_output errored: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── task_stop (REAL ONLY — needs runtime task_id) ──────────────
        Cap {
            name: "task_stop",
            group: "task",
            tags: &["task", "collab", "real-only"],
            real_only: true,
            task: "Create a task, then stop (cancel) that task.",
            assertion: "Asserts the `task_stop` tool ran successfully (is_error=false). REAL ONLY: \
                        needs the runtime-minted task id.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "task_stop").ok_or("task_stop did not run")?;
                if out.is_error {
                    return Err(format!("task_stop errored: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── internet_search (REAL ONLY — live network) ─────────────────
        Cap {
            name: "internet_search",
            group: "web",
            tags: &["web", "network", "real-only"],
            real_only: true,
            task: "Search the internet for information about the Rust programming language.",
            assertion: "Asserts the `internet_search` tool ran successfully (is_error=false). REAL \
                        ONLY: hits a live search endpoint; meaningful only with network access.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "internet_search").ok_or("internet_search did not run")?;
                if out.is_error {
                    return Err(format!("internet_search errored: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── read_url (REAL ONLY — live HTTP) ───────────────────────────
        Cap {
            name: "read_url",
            group: "web",
            tags: &["web", "network", "real-only"],
            real_only: true,
            task: "Fetch the contents of the URL https://example.com and summarize what it says.",
            assertion: "Asserts the `read_url` tool ran successfully (is_error=false). REAL ONLY: \
                        live HTTP fetch. (The deterministic HTTP path is covered separately by \
                        e2e_tool_coverage's wiremock test.)",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "read_url").ok_or("read_url did not run")?;
                if out.is_error {
                    return Err(format!("read_url errored: {}", out.model_text()));
                }
                Ok(())
            }),
        },
        // ── agent / sub-agent spawn (REAL ONLY — needs a live model) ───
        Cap {
            name: "agent",
            group: "sub-agents",
            tags: &["sub-agent", "spawn", "real-only"],
            real_only: true,
            task: "Use a sub-agent to write a one-sentence summary of what a binary search \
                   algorithm does.",
            assertion: "Asserts the `agent` (sub-agent spawn) tool ran successfully (is_error=false). \
                        REAL ONLY: spawning a sub-agent needs a live model to drive it.",
            setup: Box::new(|_ws| {}),
            mock_script: Box::new(|_ws| vec![]),
            check: Box::new(|_ws, events| {
                let out = result_for(events, "agent").ok_or("agent (sub-agent) did not run")?;
                if out.is_error {
                    return Err(format!("agent tool errored: {}", out.model_text()));
                }
                Ok(())
            }),
        },
    ]
}

/// Seed an *enabled* skill under `<ws>/.alva/skills` so `search_skills` /
/// `use_skill` have something real to find. Layout mirrors
/// `FsSkillRepository`: a `user/<name>/SKILL.md` (YAML frontmatter + body) plus
/// a `state.json` listing the skill as enabled (use_skill only activates
/// enabled skills). Runs in `setup`, i.e. BEFORE `build_mini_agent` (whose
/// SkillsPlugin scans this tree at build), so the skill is present at scan time.
fn seed_skill(ws: &Path) {
    let skills_root = ws.join(".alva/skills");
    let skill_dir = skills_root.join("user/regress-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: regress-skill\ndescription: A regression test skill for capability coverage.\n---\n\nREGRESS_SKILL_BODY: this skill exists solely to exercise search_skills/use_skill.\n",
    )
    .unwrap();
    std::fs::write(
        skills_root.join("state.json"),
        "{\"enabled\": [\"regress-skill\"]}",
    )
    .unwrap();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FailureClass {
    kind: &'static str,
    owner: &'static str,
}

fn classify_case(run: &CaseRun) -> FailureClass {
    if run.passed() {
        return FailureClass {
            kind: "pass",
            owner: "none",
        };
    }

    let detail = run.verdict.as_ref().err().map(String::as_str).unwrap_or("");
    let lower = detail.to_ascii_lowercase();

    if lower.contains("timeout") {
        return FailureClass {
            kind: "timeout",
            owner: "runtime",
        };
    }

    if run.events.iter().any(|ev| {
        matches!(
            ev,
            AgentEvent::AgentEnd {
                error: Some(error)
            } if !error.trim().is_empty()
        )
    }) {
        return FailureClass {
            kind: "runtime_error",
            owner: "runtime",
        };
    }

    if run.events.iter().any(|ev| {
        matches!(
            ev,
            AgentEvent::ToolExecutionEnd { result, .. } if result.is_error
        )
    }) {
        return FailureClass {
            kind: "tool_execution_error",
            owner: "tool",
        };
    }

    if lower.contains("did not run") || lower.contains("did not call") {
        return FailureClass {
            kind: "model_no_tool_call",
            owner: "model",
        };
    }

    FailureClass {
        kind: "assertion_failed",
        owner: "assertion",
    }
}

fn agent_end_error(run: &CaseRun) -> Option<String> {
    run.events.iter().find_map(|ev| match ev {
        AgentEvent::AgentEnd { error: Some(error) } => Some(error.clone()),
        _ => None,
    })
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

    let failure = classify_case(run);
    serde_json::json!({
        "name": run.name,
        "group": run.group,
        "tags": run.tags,
        "task": run.task,
        "assertion": run.assertion,
        "mode": run.mode,
        "verdict": if run.passed() { "pass" } else { "fail" },
        "detail": run.verdict.as_ref().err().cloned().unwrap_or_default(),
        "failure_kind": failure.kind,
        "failure_owner": failure.owner,
        "latency_ms": run.latency_ms,
        "trace": trace,
        "final_text": final_text,
        "end_error": end_error,
    })
}

fn current_timestamp_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    let timestamp_unix = current_timestamp_unix();

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

/// Build the compact, agent-readable report. This is intentionally smaller
/// than the viewer payload: it keeps classification and next-action hints, not
/// every model token or full trace row.
fn build_agent_summary_json(
    suite_label: &str,
    model_label: &str,
    components: &[String],
    runs: &[CaseRun],
) -> serde_json::Value {
    let total = runs.len();
    let passed = runs.iter().filter(|r| r.passed()).count();
    let failed = total.saturating_sub(passed);
    let duration_ms: u128 = runs.iter().map(|r| r.latency_ms).sum();

    let mut failure_counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut owner_counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut stability: std::collections::BTreeMap<
        &'static str,
        (&'static str, &'static [&'static str], usize, usize),
    > = std::collections::BTreeMap::new();
    let mut failures = Vec::new();

    for run in runs {
        let entry = stability
            .entry(run.name)
            .or_insert((run.group, run.tags, 0, 0));
        entry.3 += 1;
        if run.passed() {
            entry.2 += 1;
        }
    }

    for run in runs.iter().filter(|r| !r.passed()) {
        let class = classify_case(run);
        *failure_counts.entry(class.kind).or_insert(0) += 1;
        *owner_counts.entry(class.owner).or_insert(0) += 1;
        let tool_events = run
            .events
            .iter()
            .filter(|ev| matches!(ev, AgentEvent::ToolExecutionEnd { .. }))
            .count();
        failures.push(serde_json::json!({
            "case": run.name,
            "group": run.group,
            "tags": run.tags,
            "kind": class.kind,
            "owner": class.owner,
            "detail": run.verdict.as_ref().err().cloned().unwrap_or_default(),
            "latency_ms": run.latency_ms,
            "tool_event_count": tool_events,
            "end_error": agent_end_error(run),
        }));
    }

    let mut case_stability: Vec<serde_json::Value> = stability
        .into_iter()
        .map(|(case, (group, tags, passed, total))| {
            serde_json::json!({
                "case": case,
                "group": group,
                "tags": tags,
                "passed": passed,
                "total": total,
                "pass_rate": if total == 0 { 1.0 } else { passed as f64 / total as f64 },
            })
        })
        .collect();
    case_stability.sort_by(|a, b| {
        let ar = a.get("pass_rate").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let br = b.get("pass_rate").and_then(|v| v.as_f64()).unwrap_or(1.0);
        ar.partial_cmp(&br)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let ac = a.get("case").and_then(|v| v.as_str()).unwrap_or("");
                let bc = b.get("case").and_then(|v| v.as_str()).unwrap_or("");
                ac.cmp(bc)
            })
    });

    let mut next_actions = Vec::new();
    for (owner, count) in owner_counts {
        let action = match owner {
            "model" => "tighten task prompt/tool descriptions, then rerun with repeats before changing runtime code",
            "tool" => "inspect tool input/output contract and execution error details",
            "runtime" => "inspect agent loop, middleware, timeout, or provider transport before prompt tuning",
            "assertion" => "compare trace against test invariant; decide whether expectation or tool behavior is wrong",
            _ => "inspect failed case manually",
        };
        next_actions.push(serde_json::json!({
            "owner": owner,
            "count": count,
            "action": action,
        }));
    }

    serde_json::json!({
        "schema_version": 1,
        "report_kind": "agent_capability_summary",
        "suite": suite_label,
        "model": model_label,
        "timestamp_unix": current_timestamp_unix(),
        "components": components,
        "component_count": components.len(),
        "summary": {
            "passed": passed,
            "failed": failed,
            "total": total,
            "pass_rate": if total == 0 { 1.0 } else { passed as f64 / total as f64 },
            "duration_ms": duration_ms,
        },
        "failure_counts": failure_counts,
        "case_stability": case_stability,
        "failures": failures,
        "next_actions": next_actions,
    })
}

/// Sanitize a label into a filename-safe token (replace `/`, spaces, etc.).
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Resolve `<crate>/tests/reports` (stable regardless of test CWD).
fn reports_dir() -> std::path::PathBuf {
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("tests/reports");
    dir
}

const WRITE_REPORT_ENV: &str = "ALVA_WRITE_CAPABILITY_REPORT";
const REPORT_DIR_ENV: &str = "ALVA_CAPABILITY_REPORT_DIR";
const REPEAT_ENV: &str = "ALVA_TEST_REPEAT";

fn capability_report_output_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var(REPORT_DIR_ENV) {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Some(std::path::PathBuf::from(dir));
        }
    }

    match std::env::var(WRITE_REPORT_ENV) {
        Ok(v) if matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES") => Some(reports_dir()),
        _ => None,
    }
}

fn parse_repeat_count(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1)
}

fn repeat_count_from_env() -> usize {
    parse_repeat_count(std::env::var(REPEAT_ENV).ok().as_deref())
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
    let mdays = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
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
fn maybe_write_run_report(
    output_dir: Option<std::path::PathBuf>,
    suite_label: &str,
    model_label: &str,
    components: &[String],
    runs: &[CaseRun],
) -> Option<std::path::PathBuf> {
    output_dir.map(|dir| write_run_report_to_dir(&dir, suite_label, model_label, components, runs))
}

fn write_run_report_to_dir(
    dir: &std::path::Path,
    suite_label: &str,
    model_label: &str,
    components: &[String],
    runs: &[CaseRun],
) -> std::path::PathBuf {
    let report = build_report_json(suite_label, model_label, components, runs);
    let json_str = serde_json::to_string_pretty(&report).expect("report JSON must serialize");

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

    let mut agent_summary = build_agent_summary_json(suite_label, model_label, components, runs);
    if let Some(obj) = agent_summary.as_object_mut() {
        obj.insert(
            "run_file".to_string(),
            serde_json::Value::String(
                run_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        );
    }
    let summary_str =
        serde_json::to_string_pretty(&agent_summary).expect("agent summary JSON must serialize");
    let summary_path = dir.join(format!(
        "agent-summary-{}-{}-{}.json",
        timestamp_slug(unix),
        slugify(suite_label),
        slugify(model_label),
    ));
    std::fs::write(&summary_path, &summary_str).expect("failed to write agent summary JSON");
    std::fs::write(dir.join("latest-agent-summary.json"), &summary_str)
        .expect("failed to write latest agent summary JSON");

    regenerate_index(&dir);
    regenerate_data_js(&dir);
    run_path
}

#[cfg(test)]
mod report_write_tests {
    use super::*;

    #[test]
    fn failed_case_json_classifies_missing_tool_call_as_model_failure() {
        let run = CaseRun {
            name: "read_file",
            group: "core",
            tags: &["tool"],
            task: "Read the fixture",
            assertion: "read_file runs",
            mode: "real",
            events: vec![],
            verdict: Err("read_file did not run".to_string()),
            latency_ms: 42,
        };

        let json = case_to_json(&run);

        assert_eq!(json["failure_kind"], "model_no_tool_call");
        assert_eq!(json["failure_owner"], "model");
    }

    #[test]
    fn failed_case_json_classifies_tool_execution_error_as_tool_failure() {
        let run = CaseRun {
            name: "read_file",
            group: "core",
            tags: &["tool"],
            task: "Read a missing file",
            assertion: "read_file reports failure",
            mode: "real",
            events: vec![AgentEvent::ToolExecutionEnd {
                tool_call: ToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path": "missing.txt"}),
                },
                result: ToolOutput::error("file not found"),
            }],
            verdict: Err("read_file returned an error".to_string()),
            latency_ms: 10,
        };

        let json = case_to_json(&run);

        assert_eq!(json["failure_kind"], "tool_execution_error");
        assert_eq!(json["failure_owner"], "tool");
    }

    #[test]
    fn agent_summary_keeps_compact_failure_counts_and_next_actions() {
        let runs = vec![
            CaseRun {
                name: "create_file",
                group: "core",
                tags: &["tool"],
                task: "Create a fixture",
                assertion: "create_file runs",
                mode: "real",
                events: vec![],
                verdict: Ok(()),
                latency_ms: 5,
            },
            CaseRun {
                name: "agent",
                group: "sub-agents",
                tags: &["tool", "real-only"],
                task: "Ask a sub-agent",
                assertion: "agent runs",
                mode: "real",
                events: vec![],
                verdict: Err("agent did not run".to_string()),
                latency_ms: 12,
            },
        ];

        let summary = build_agent_summary_json("real", "deepseek-v4-flash", &[], &runs);

        assert_eq!(summary["summary"]["passed"], 1);
        assert_eq!(summary["summary"]["total"], 2);
        assert_eq!(summary["failure_counts"]["model_no_tool_call"], 1);
        assert_eq!(summary["failures"][0]["case"], "agent");
        assert_eq!(summary["failures"][0]["owner"], "model");
        assert_eq!(summary["next_actions"][0]["owner"], "model");
    }

    #[test]
    fn repeat_count_parser_defaults_to_one_and_rejects_zero() {
        assert_eq!(parse_repeat_count(None), 1);
        assert_eq!(parse_repeat_count(Some("")), 1);
        assert_eq!(parse_repeat_count(Some("0")), 1);
        assert_eq!(parse_repeat_count(Some("3")), 3);
    }

    #[test]
    fn agent_summary_aggregates_case_stability_across_repeats() {
        let runs = vec![
            CaseRun {
                name: "agent",
                group: "sub-agents",
                tags: &["real-only"],
                task: "Ask a sub-agent",
                assertion: "agent runs",
                mode: "real",
                events: vec![],
                verdict: Ok(()),
                latency_ms: 10,
            },
            CaseRun {
                name: "agent",
                group: "sub-agents",
                tags: &["real-only"],
                task: "Ask a sub-agent",
                assertion: "agent runs",
                mode: "real",
                events: vec![],
                verdict: Err("agent did not run".to_string()),
                latency_ms: 20,
            },
        ];

        let summary = build_agent_summary_json("real", "deepseek-v4-flash", &[], &runs);

        assert_eq!(summary["case_stability"][0]["case"], "agent");
        assert_eq!(summary["case_stability"][0]["passed"], 1);
        assert_eq!(summary["case_stability"][0]["total"], 2);
        assert_eq!(summary["case_stability"][0]["pass_rate"], 0.5);
    }

    #[test]
    fn report_write_is_skipped_without_output_dir() {
        let path = maybe_write_run_report(None, "mock", "MockLanguageModel", &[], &[]);
        assert!(path.is_none(), "default test runs should not write reports");
    }

    #[test]
    fn report_write_uses_explicit_output_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = maybe_write_run_report(
            Some(tmp.path().to_path_buf()),
            "mock",
            "MockLanguageModel",
            &[],
            &[],
        )
        .expect("explicit output dir should write report");

        assert!(
            path.exists(),
            "run report should exist at {}",
            path.display()
        );
        assert!(
            tmp.path().join("index.json").exists(),
            "index should be regenerated"
        );
        assert!(
            tmp.path().join("data.js").exists(),
            "viewer data should be regenerated"
        );
        assert!(
            tmp.path().join("latest-agent-summary.json").exists(),
            "agent-readable compact summary should be written"
        );
    }
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
            let Ok(text) = std::fs::read_to_string(ent.path()) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let ts = v
                .get("timestamp_unix")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            runs.push((ts, name, v));
        }
    }
    // Newest first; tie-break by filename desc so same-second runs stay stable.
    runs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    runs.truncate(DATA_JS_MAX_RUNS);

    let array: Vec<serde_json::Value> = runs.into_iter().map(|(_, _, v)| v).collect();
    let json =
        serde_json::to_string(&serde_json::Value::Array(array)).unwrap_or_else(|_| "[]".into());
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
            let Ok(text) = std::fs::read_to_string(ent.path()) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
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
        let ta = a
            .get("timestamp_unix")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let tb = b
            .get("timestamp_unix")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
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
        // `real_only` cases can't be scripted deterministically (runtime ids /
        // live network / sub-agent spawn) — they run only in the real suite.
        if case.real_only {
            eprintln!("⏭  {} — skipped (real-only case)", case.name);
            continue;
        }

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

    let all_passed = print_stderr_summary("MOCK capability suite (MockLanguageModel)", &runs);
    if let Some(run_path) = maybe_write_run_report(
        capability_report_output_dir(),
        "mock",
        "MockLanguageModel",
        &components,
        &runs,
    ) {
        eprintln!(
            "report run: {}\nopen viewer: double-click crates/alva-app-core/tests/reports/viewer.html (reads data.js, no server needed)",
            run_path.display()
        );
    } else {
        eprintln!(
            "report skipped: set {WRITE_REPORT_ENV}=1 or {REPORT_DIR_ENV}=<dir> to write capability reports"
        );
    }

    assert!(
        all_passed,
        "mock capability suite had failures — see report above / report run JSON"
    );
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
    let kind = std::env::var("ALVA_TEST_KIND")
        .ok()
        .filter(|s| !s.is_empty());
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
            Some("anthropic") => {
                Arc::new(alva_llm_provider::AnthropicProvider::new(config.clone()))
            }
            Some("openai-responses") => Arc::new(alva_llm_provider::OpenAIResponsesProvider::new(
                config.clone(),
            )),
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

    let repeat_count = repeat_count_from_env();
    if repeat_count > 1 {
        eprintln!("real_capability_suite repeat count: {repeat_count} ({REPEAT_ENV})");
    }

    let mut runs: Vec<CaseRun> = Vec::new();

    for repeat_index in 1..=repeat_count {
        if repeat_count > 1 {
            eprintln!("real_capability_suite repeat {repeat_index}/{repeat_count}");
        }

        for case in cases() {
            let tmp = tempfile::tempdir().unwrap();
            let ws = tmp.path().canonicalize().unwrap();
            (case.setup)(&ws);

            let agent = build_mini_agent(&ws, make_model(), &toggles).await;
            let start = std::time::Instant::now();
            let rx = agent.prompt_text(case.task);

            let (events, verdict) =
                match tokio::time::timeout(std::time::Duration::from_secs(120), collect_events(rx))
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
    }

    let header = format!(
        "REAL capability suite (model: {}, repeats: {})",
        model_name, repeat_count
    );
    let all_passed = print_stderr_summary(&header, &runs);
    if let Some(run_path) = maybe_write_run_report(
        capability_report_output_dir(),
        "real",
        &model_name,
        &components,
        &runs,
    ) {
        eprintln!(
            "report run: {}\nopen viewer: double-click crates/alva-app-core/tests/reports/viewer.html (reads data.js, no server needed)",
            run_path.display()
        );
    } else {
        eprintln!(
            "report skipped: set {WRITE_REPORT_ENV}=1 or {REPORT_DIR_ENV}=<dir> to write capability reports"
        );
    }

    assert!(
        all_passed,
        "real capability suite had failures against model `{model_name}` — see report above / report run JSON"
    );
}
