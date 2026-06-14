//! Agent-capability eval against a real OpenAI-compatible local model.
//!
//! Validates that the agent — given a natural-language prompt — actually
//! selects the right tool(s) and reaches a correct final state. Unlike
//! the MockLanguageModel-driven `e2e_tool_coverage.rs` tests, this one
//! does NOT script the assistant turn; the LLM's tool-selection is the
//! thing under test.
//!
//! ## Running
//!
//! ```bash
//! cargo test -p alva-app-core --test eval_agent_tools -- --ignored --nocapture
//! ```
//!
//! The test is `#[ignore]` by default so `cargo test` doesn't accidentally
//! spend real LLM time. If the endpoint isn't reachable, the test prints
//! a notice and returns success (skip semantics).
//!
//! ## Configuration (env vars)
//!
//! - `EVAL_BASE_URL`   default: http://10.10.1.100:10443/v1
//! - `EVAL_MODEL`      default: Qwen3.5-9B-MLX-4bit
//! - `EVAL_API_KEY`    default: 123456
//! - `EVAL_REPEATS`    default: 3   (each case runs N times, majority wins)
//! - `EVAL_MAX_ITERS`  default: 15  (per-prompt iteration cap)
//!
//! ## What "pass" means (medium-strictness scoring)
//!
//! For each case the agent must:
//!   1. End cleanly (no error, no runaway loop)
//!   2. Have called every tool in `required_tools` at least once
//!   3. (if `any_of_tools` non-empty) have called at least one of them
//!   4. (if `fs_check` set) leave the filesystem in the expected state
//!
//! Tool ORDER is NOT enforced — the agent may interleave or repeat as it
//! reasons. Order-correctness lives in `e2e_tool_coverage.rs::stage3_*`,
//! which uses mock-scripted sequences.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use alva_app_core::base_agent::{BaseAgent, PermissionMode};
use alva_app_core::extension::{ApprovalExtension, PermissionExtension};
use alva_app_core::AgentEvent;
use alva_llm_provider::{OpenAIChatProvider, ProviderConfig};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DEFAULT_BASE_URL: &str = "http://10.10.1.100:10443/v1";
const DEFAULT_MODEL: &str = "Qwen3.5-9B-MLX-4bit";
const DEFAULT_API_KEY: &str = "123456";

fn eval_base_url() -> String {
    std::env::var("EVAL_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}
fn eval_model() -> String {
    std::env::var("EVAL_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}
fn eval_api_key() -> String {
    std::env::var("EVAL_API_KEY").unwrap_or_else(|_| DEFAULT_API_KEY.to_string())
}
fn eval_repeats() -> usize {
    std::env::var("EVAL_REPEATS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3)
}
fn eval_max_iters() -> u32 {
    std::env::var("EVAL_MAX_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15)
}

// ---------------------------------------------------------------------------
// Endpoint probe (skip if unreachable)
// ---------------------------------------------------------------------------

fn probe_endpoint() -> bool {
    use std::net::ToSocketAddrs;
    let url = eval_base_url();
    let host_port = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("");
    if host_port.is_empty() {
        eprintln!("[probe] could not extract host:port from {url}");
        return false;
    }
    let addrs = match host_port.to_socket_addrs() {
        Ok(iter) => iter.collect::<Vec<_>>(),
        Err(e) => {
            eprintln!("[probe] DNS / socket-addr parse failed for {host_port}: {e}");
            return false;
        }
    };
    if addrs.is_empty() {
        eprintln!("[probe] empty addr list for {host_port}");
        return false;
    }
    for addr in &addrs {
        if std::net::TcpStream::connect_timeout(addr, Duration::from_secs(3)).is_ok() {
            return true;
        }
    }
    eprintln!("[probe] could not TCP-connect to any of {addrs:?}");
    false
}

// ---------------------------------------------------------------------------
// Agent construction
// ---------------------------------------------------------------------------

fn build_model() -> Arc<dyn alva_kernel_abi::LanguageModel> {
    Arc::new(OpenAIChatProvider::new(ProviderConfig {
        api_key: eval_api_key(),
        model: eval_model(),
        base_url: eval_base_url(),
        max_tokens: 4096,
        custom_headers: Default::default(),
        kind: Some("openai-chat".into()),
    }))
}

async fn build_agent(workspace: &Path) -> BaseAgent {
    let (approval_ext, mut approval_rx) = ApprovalExtension::with_channel();
    let model = build_model();

    let agent = BaseAgent::builder()
        .workspace(workspace)
        .system_prompt(
            "You are a helpful coding assistant working in a sandboxed workspace. \
             You have tools for reading/writing files, searching, running shell commands, \
             and tracking tasks. Use the tools to complete the user's request. \
             When the work is done, briefly state what you did. Do not ask follow-up questions.",
        )
        .plugin(Box::new(PermissionExtension::new().with_initial(PermissionMode::AcceptShell)))
        .plugin(Box::new(approval_ext))
        .plugin(Box::new(alva_app_core::extension::CoreExtension))
        .plugin(Box::new(alva_app_core::extension::ShellExtension))
        .plugin(Box::new(alva_app_core::extension::PlanningExtension))
        .plugin(Box::new(alva_app_core::extension::TaskExtension::default()))
        .plugin(Box::new(alva_app_core::extension::UtilityExtension))
        .plugin(Box::new(alva_app_core::extension::WebExtension))
        .middleware(Arc::new(alva_kernel_core::builtins::LoopDetectionMiddleware::new()))
        .middleware(Arc::new(alva_kernel_core::builtins::DanglingToolCallMiddleware::new()))
        .middleware(Arc::new(alva_kernel_core::builtins::ToolTimeoutMiddleware::default()))
        .middleware(Arc::new(alva_host_native::middleware::CheckpointMiddleware::new()))
        .max_iterations(eval_max_iters())
        .build(model)
        .await
        .expect("agent build should succeed");

    agent.set_permission_mode(PermissionMode::AcceptShell);

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
// Event collection
// ---------------------------------------------------------------------------

struct RunResult {
    tools: Vec<String>,
    ended_cleanly: bool,
    end_error: Option<String>,
    iterations_seen: usize,
}

async fn collect_run(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    overall_timeout: Duration,
) -> RunResult {
    let mut tools = Vec::new();
    let mut ended_cleanly = false;
    let mut end_error = None;
    let mut iters = 0;

    let deadline = tokio::time::Instant::now() + overall_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            end_error = Some("timeout waiting for events".to_string());
            break;
        }
        let next = tokio::time::timeout(remaining, rx.recv()).await;
        let event = match next {
            Ok(Some(ev)) => ev,
            Ok(None) => break,
            Err(_) => {
                end_error = Some("timeout waiting for events".to_string());
                break;
            }
        };
        match &event {
            AgentEvent::ToolExecutionEnd { tool_call, .. } => {
                tools.push(tool_call.name.clone());
            }
            AgentEvent::MessageStart { .. } => {
                iters += 1;
            }
            AgentEvent::AgentEnd { error } => {
                ended_cleanly = error.is_none();
                end_error = error.clone();
                break;
            }
            _ => {}
        }
    }
    RunResult {
        tools,
        ended_cleanly,
        end_error,
        iterations_seen: iters,
    }
}

// ---------------------------------------------------------------------------
// Case definition
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct EvalCase {
    name: &'static str,
    prompt: &'static str,
    /// Every name here must appear at least once in the tool sequence.
    required_tools: Vec<&'static str>,
    /// At least one of these must appear (any-of). Empty = no constraint.
    any_of_tools: Vec<&'static str>,
    /// Optional workspace seed (run before the agent starts).
    setup: Option<fn(&Path)>,
    /// Optional filesystem post-condition.
    fs_check: Option<fn(&Path) -> bool>,
}

fn cases() -> Vec<EvalCase> {
    vec![
        // ---- Group A: single-tool selection ----
        EvalCase {
            name: "A1_read_file",
            prompt: "Read the file hello.txt and tell me what's in it.",
            required_tools: vec!["read_file"],
            any_of_tools: vec![],
            setup: Some(|ws| {
                std::fs::write(ws.join("hello.txt"), "hello world from eval").unwrap()
            }),
            fs_check: None,
        },
        EvalCase {
            name: "A2_list_or_find",
            prompt: "What .rs files exist in this directory? List them.",
            required_tools: vec![],
            any_of_tools: vec!["list_files", "find_files"],
            setup: Some(|ws| {
                std::fs::write(ws.join("a.rs"), "fn main() {}").unwrap();
                std::fs::write(ws.join("b.rs"), "fn other() {}").unwrap();
            }),
            fs_check: None,
        },
        EvalCase {
            name: "A3_grep",
            prompt: "Find all occurrences of the literal text 'TODO' in this directory's files.",
            required_tools: vec!["grep_search"],
            any_of_tools: vec![],
            setup: Some(|ws| {
                std::fs::write(ws.join("notes.md"), "TODO: fix this\nMore text\nTODO: another one").unwrap();
            }),
            fs_check: None,
        },
        EvalCase {
            name: "A4_shell",
            prompt: "Run the shell command `echo eval_marker_xyz` and tell me what it printed.",
            required_tools: vec!["execute_shell"],
            any_of_tools: vec![],
            setup: None,
            fs_check: None,
        },
        // ---- Group B: tool composition ----
        EvalCase {
            name: "B5_read_then_edit",
            prompt: "Read the file a.txt to see its contents, then edit it: change the word 'old' to 'new'.",
            required_tools: vec!["file_edit"],
            any_of_tools: vec![],
            setup: Some(|ws| {
                std::fs::write(ws.join("a.txt"), "this is old content").unwrap()
            }),
            fs_check: Some(|ws| {
                std::fs::read_to_string(ws.join("a.txt"))
                    .map(|s| s.contains("new") && !s.contains("old"))
                    .unwrap_or(false)
            }),
        },
        EvalCase {
            name: "B6_find_then_read",
            prompt: "Look at the Rust files in this directory and tell me the contents of hello.rs.",
            required_tools: vec!["read_file"],
            any_of_tools: vec!["find_files", "list_files"],
            setup: Some(|ws| {
                std::fs::write(ws.join("hello.rs"), "fn hello() { println!(\"hi\"); }").unwrap()
            }),
            fs_check: None,
        },
        EvalCase {
            name: "B7_create_then_list",
            prompt: "Create a new file called todo.md with the exact content '# TODO'. Then list the directory to confirm the file is there.",
            required_tools: vec!["create_file"],
            any_of_tools: vec!["list_files", "find_files"],
            setup: None,
            fs_check: Some(|ws| {
                let p = ws.join("todo.md");
                p.exists()
                    && std::fs::read_to_string(&p)
                        .map(|s| s.contains("# TODO"))
                        .unwrap_or(false)
            }),
        },
        // ---- Group C: error recovery (agent must NOT loop forever) ----
        EvalCase {
            name: "C8_read_nonexistent",
            prompt: "Try to read the file /tmp/eval_nonexistent_xyz_abc_123.txt. If it doesn't exist, just say so and stop.",
            required_tools: vec!["read_file"],
            any_of_tools: vec![],
            setup: None,
            fs_check: None,
        },
        EvalCase {
            name: "C9_edit_no_match",
            prompt: "Edit a.txt: replace the text 'nothing_matches_this_xyz' with 'whatever'. If that text doesn't exist in the file, just report the failure and stop.",
            required_tools: vec!["file_edit"],
            any_of_tools: vec!["read_file"],
            setup: Some(|ws| {
                std::fs::write(ws.join("a.txt"), "real content here, no marker").unwrap()
            }),
            fs_check: Some(|ws| {
                // File must be UNCHANGED.
                std::fs::read_to_string(ws.join("a.txt"))
                    .map(|s| s == "real content here, no marker")
                    .unwrap_or(false)
            }),
        },
        // ---- Group D: task tracking ----
        EvalCase {
            name: "D10_task_lifecycle",
            prompt: "Use the task tools: create a task with subject 'review PR' and description 'check the new e2e tests'. Then update its status to completed. Then look it up to confirm the status is completed.",
            required_tools: vec!["task_create"],
            any_of_tools: vec!["task_update", "task_get"],
            setup: None,
            fs_check: None,
        },
    ]
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

struct AttemptOutcome {
    passed: bool,
    reasons: Vec<String>,
    tools: Vec<String>,
    iterations: usize,
}

async fn run_attempt(case: &EvalCase) -> AttemptOutcome {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws: PathBuf = tmp
        .path()
        .canonicalize()
        .expect("canonicalize workspace");
    if let Some(setup) = case.setup {
        setup(&ws);
    }

    let agent = build_agent(&ws).await;
    let rx = agent.prompt_text(case.prompt);

    // Wide overall timeout — local model can be slow on a multi-step
    // tool loop. If it blows past this, we treat it as failure.
    let run = collect_run(rx, Duration::from_secs(120)).await;

    let mut reasons = Vec::new();
    let mut passed = true;

    if !run.ended_cleanly {
        passed = false;
        reasons.push(format!(
            "agent did not end cleanly: {}",
            run.end_error.as_deref().unwrap_or("(no error)")
        ));
    }

    for req in &case.required_tools {
        if !run.tools.iter().any(|t| t == req) {
            passed = false;
            reasons.push(format!("missing required tool `{req}`"));
        }
    }

    if !case.any_of_tools.is_empty() {
        let hit = case
            .any_of_tools
            .iter()
            .any(|aot| run.tools.iter().any(|t| t == aot));
        if !hit {
            passed = false;
            reasons.push(format!(
                "none of any-of tools called: {:?}",
                case.any_of_tools
            ));
        }
    }

    if let Some(check) = case.fs_check {
        if !check(&ws) {
            passed = false;
            reasons.push("fs_check failed".to_string());
        }
    }

    AttemptOutcome {
        passed,
        reasons,
        tools: run.tools,
        iterations: run.iterations_seen,
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Local Network Privacy warmup: macOS 15 (Sequoia) gates outbound TCP
/// to RFC1918 (10.x / 192.168.x / 172.16-31.x) on a per-binary-identity
/// basis. The first connect from a new identity fails fast with
/// EHOSTUNREACH while the system queues a GUI prompt. This test stays
/// alive long enough for that prompt to surface and for the user to
/// click Allow, retrying every few seconds. Once granted, all later
/// runs of binaries signed with the same identity inherit the decision.
///
/// Run this once after first build, in a GUI terminal (iTerm /
/// Terminal.app), and click Allow when macOS prompts:
///
/// ```bash
/// ./scripts/run-eval.sh --warmup-only
/// ```
#[tokio::test]
#[ignore]
async fn eval_warmup() {
    use std::net::{TcpStream, ToSocketAddrs};
    let url = eval_base_url();
    let host_port = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("");
    let addr = match host_port.to_socket_addrs().ok().and_then(|mut i| i.next()) {
        Some(a) => a,
        None => {
            eprintln!("[warmup] could not resolve {host_port}");
            return;
        }
    };

    eprintln!("\n[warmup] target = {addr}");
    eprintln!("[warmup] ⚠️  If a macOS prompt appears asking to allow Local Network");
    eprintln!("[warmup]     access — CLICK ALLOW. The test will keep retrying for");
    eprintln!("[warmup]     up to ~60 seconds to give you time.");
    eprintln!();

    let max_attempts = 12;
    let between = Duration::from_secs(5);
    for attempt in 1..=max_attempts {
        eprint!("[warmup] attempt {attempt}/{max_attempts} → ");
        match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(s) => {
                eprintln!(
                    "✓ CONNECTED — Local Network access granted (local={:?}).",
                    s.local_addr().ok()
                );
                eprintln!(
                    "[warmup] You can now run the full eval. The grant is per\n\
                     [warmup] codesign identity, so it persists across rebuilds\n\
                     [warmup] that re-sign with the same identity."
                );
                return;
            }
            Err(e) => {
                eprintln!(
                    "✗ {e} (raw_os_error={:?}) — sleeping {}s",
                    e.raw_os_error(),
                    between.as_secs()
                );
            }
        }
        tokio::time::sleep(between).await;
    }
    eprintln!(
        "[warmup] ✗ never connected within {} attempts.\n\
         [warmup] Possible reasons:\n\
         [warmup]   - You're running over SSH / headless: no GUI prompt can appear.\n\
         [warmup]     Re-run inside a GUI terminal (iTerm / Terminal.app).\n\
         [warmup]   - Prompt fired but was dismissed. Check System Settings →\n\
         [warmup]     Privacy & Security → Local Network and verify the entry\n\
         [warmup]     for this binary's terminal (or for cargo / alva-eval-signing)\n\
         [warmup]     is toggled ON.\n\
         [warmup]   - Binary not signed with stable identity. Re-run via\n\
         [warmup]     scripts/run-eval.sh which signs with alva-eval-signing.",
        max_attempts
    );
}

/// Minimal `std::net::TcpStream::connect` probe — bypasses all of
/// reqwest/hyper/tokio. Confirms whether the issue is at the Rust
/// process-level networking syscall or higher in the stack.
#[tokio::test]
#[ignore]
async fn eval_raw_tcp_probe() {
    use std::net::{TcpStream, ToSocketAddrs};
    let url = eval_base_url();
    let host_port = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("");
    eprintln!("\n[raw_tcp_probe] target = {host_port}");

    let addrs: Vec<_> = host_port.to_socket_addrs().map(|i| i.collect()).unwrap_or_default();
    eprintln!("[raw_tcp_probe] getaddrinfo returned {} addrs: {:?}", addrs.len(), addrs);

    for (i, addr) in addrs.iter().enumerate() {
        eprintln!("\n[raw_tcp_probe] attempt #{} → {addr}", i + 1);
        match TcpStream::connect_timeout(addr, Duration::from_secs(5)) {
            Ok(s) => {
                eprintln!("[raw_tcp_probe]   ✓ connected, local addr = {:?}, peer = {:?}",
                    s.local_addr().ok(), s.peer_addr().ok());
            }
            Err(e) => {
                eprintln!("[raw_tcp_probe]   ✗ failed: {e} (raw_os_error = {:?})", e.raw_os_error());
            }
        }
    }

    // For comparison: also try the blocking `connect` (no timeout) which
    // takes a different code path through std::net.
    eprintln!("\n[raw_tcp_probe] now trying TcpStream::connect (no timeout)…");
    match TcpStream::connect(host_port) {
        Ok(s) => eprintln!("[raw_tcp_probe]   ✓ connected peer={:?}", s.peer_addr().ok()),
        Err(e) => eprintln!("[raw_tcp_probe]   ✗ {e} (raw_os_error = {:?})", e.raw_os_error()),
    }
}

/// Standalone probe that fires a real reqwest GET to the configured
/// endpoint and unwinds the full error chain. Used to diagnose
/// "HTTP request failed" without going through the full agent stack.
#[tokio::test]
#[ignore]
async fn eval_reqwest_probe() {
    let url = format!("{}/models", eval_base_url());
    eprintln!("\n[reqwest_probe] GET {url}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");
    let r = client
        .get(&url)
        .bearer_auth(eval_api_key())
        .send()
        .await;
    match r {
        Ok(resp) => {
            eprintln!("[reqwest_probe] STATUS = {}", resp.status());
            let body = resp.text().await.unwrap_or_default();
            eprintln!("[reqwest_probe] body (first 300): {}", &body[..body.len().min(300)]);
        }
        Err(e) => {
            use std::error::Error as _;
            eprintln!("[reqwest_probe] ERROR top: {e}");
            eprintln!("[reqwest_probe] Debug:     {e:?}");
            let mut src: Option<&dyn std::error::Error> = e.source();
            let mut depth = 1;
            while let Some(s) = src {
                eprintln!("[reqwest_probe]   caused by ({depth}): {s}");
                src = s.source();
                depth += 1;
            }
        }
    }
}

#[tokio::test]
#[ignore]
async fn eval_agent_tools_main() {
    let skip_probe = std::env::var("EVAL_SKIP_PROBE")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);
    if !skip_probe && !probe_endpoint() {
        eprintln!(
            "\n[eval_agent_tools] endpoint {} unreachable from this process — skipping.\n\
             If you know it's actually up (e.g. you ran cargo from a sandboxed shell\n\
             that blocks outbound TCP for the test binary), bypass the probe with:\n\
             \n    EVAL_SKIP_PROBE=1 cargo test -p alva-app-core --test eval_agent_tools \\\n        -- --ignored --nocapture\n",
            eval_base_url()
        );
        return;
    }

    let cases = cases();
    let n_repeats = eval_repeats();

    eprintln!(
        "\n=== alva-agent tool eval ===\n  model:  {}\n  url:    {}\n  cases:  {}\n  repeats: {}\n",
        eval_model(),
        eval_base_url(),
        cases.len(),
        n_repeats,
    );

    let started = std::time::Instant::now();
    let mut summary: Vec<(String, usize, usize, Vec<String>)> = Vec::new();

    for case in &cases {
        eprintln!("--- {} ---", case.name);
        eprintln!("  prompt: {}", case.prompt);
        let mut pass = 0;
        let mut failure_reasons = Vec::new();
        for attempt in 1..=n_repeats {
            let t = std::time::Instant::now();
            let out = run_attempt(case).await;
            let elapsed_ms = t.elapsed().as_millis();
            let marker = if out.passed { "✓" } else { "✗" };
            eprintln!(
                "  attempt {}/{} {} ({} ms, {} iters, tools: {:?})",
                attempt, n_repeats, marker, elapsed_ms, out.iterations, out.tools
            );
            if out.passed {
                pass += 1;
            } else {
                for r in &out.reasons {
                    eprintln!("      reason: {r}");
                    failure_reasons.push(r.clone());
                }
            }
        }
        summary.push((case.name.to_string(), pass, n_repeats, failure_reasons));
    }

    let total_elapsed = started.elapsed();

    eprintln!("\n=== Eval Summary ({:.1}s total) ===", total_elapsed.as_secs_f64());
    let mut cases_passed = 0;
    let mut total_pass = 0;
    let mut total_runs = 0;
    for (name, pass, total, _reasons) in &summary {
        let majority = pass * 2 > *total; // strict majority of repeats
        if majority {
            cases_passed += 1;
        }
        total_pass += pass;
        total_runs += total;
        let marker = if majority { "PASS" } else { "FAIL" };
        eprintln!("  [{marker}] {name}: {pass}/{total}");
    }
    eprintln!(
        "\n  cases passing majority: {}/{}\n  total runs passed:      {}/{}\n",
        cases_passed,
        summary.len(),
        total_pass,
        total_runs
    );

    // We do NOT panic on eval failures — the eval is observational. The
    // test exit code only reflects "did the framework itself work". Treat
    // the printed summary as the actual signal.
}
