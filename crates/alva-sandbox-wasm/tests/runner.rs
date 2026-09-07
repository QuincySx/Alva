// INPUT:  std::{collections, fs, io, net, process, sync}, bundled wasm-env, alva_kernel_abi, alva_llm_wire, alva_sandbox_wasm, alva_test, async_trait, futures, serde_json, tempfile, tokio, wasmtime
// OUTPUT: Integration coverage for runner limits, preopens, context/LLM/fetch proxies, domain policy, escalation, and run_script
// POS:    Builds WASIp1 guests on demand and verifies isolation, environment injection, real tool alignment, and guest behavior.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use alva_kernel_abi::{
    AgentError, LanguageModel, StreamEvent, Tool, ToolExecutionContext, ToolOutput,
};
use alva_llm_wire::{LlmProxyRequest, LlmProxyResponse, ToolDefinition};
use alva_sandbox_wasm::{
    run_module, AuditEvent, Grant, RunLimits, RunRequest, SandboxRunner, SandboxStoreData,
    WasmEnvironmentContext,
};
use alva_test::fixtures::{make_assistant_message, make_tool_call_message};
use alva_test::mock_provider::MockLanguageModel;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use wasmtime::Linker;

static FIXTURE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static WORKER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static WORKER_RUNNER: OnceLock<SandboxRunner> = OnceLock::new();

const HOST_MARKER: &str = "SECRET-KEY-abc";
const FINAL_RESPONSE: &str = "sandboxed agent finished";

fn wasm_environment_injection() -> String {
    let skill_md = include_str!("../../../assets/skills/wasm-env/SKILL.md");
    let (_, body) = skill_md
        .split_once("\n---\n")
        .expect("wasm-env skill has YAML frontmatter");
    format!("## Skill: wasm-env\n\n{}\n", body.trim())
}

fn declared_top_level_tools(injection: &str) -> BTreeSet<String> {
    let (_, tail) = injection
        .split_once("```tool-names\n")
        .expect("wasm-env injection has a tool-names block");
    let (tool_names, _) = tail
        .split_once("\n```")
        .expect("tool-names block is closed");
    let lines = tool_names.lines().map(str::to_owned).collect::<Vec<_>>();
    let names = lines.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        lines.len(),
        names.len(),
        "tool-names block must not contain duplicates"
    );
    names
}

/// A host that registers only the LLM proxy must still be able to instantiate
/// the guest. The guest declares its host imports unconditionally, so anything
/// it needs but the caller does not care about — audit logging today — has to
/// have a default in the runner; otherwise forgetting one is an "unknown
/// import" trap that only fires at run time.
#[test]
fn guest_instantiates_without_the_caller_registering_the_audit_log() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let model: Arc<dyn LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message(FINAL_RESPONSE)));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build one host proxy runtime");
    let runtime_handle = runtime.handle().clone();

    let outcome = SandboxRunner::new()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work.path(), "/work")],
                args: worker_args("no audit log wired", "/work/result.txt"),
                allowed_domains: Vec::new(),
                limits: RunLimits::default(),
            },
            // Register the required environment context and LLM proxy, but
            // deliberately omit audit logging to exercise the runner default.
            move |linker| {
                let injection = wasm_environment_injection();
                alva_sandbox_wasm::register_wasm_environment_context_proxy(linker, move || {
                    Ok(WasmEnvironmentContext::new(injection.clone()))
                })?;
                alva_sandbox_wasm::register_llm_proxy(linker, move |request: LlmProxyRequest| {
                    let handle = runtime_handle.clone();
                    let model = Arc::clone(&model);
                    let events = handle.block_on(async move {
                        let tools: Vec<ToolDefinition> = request.tools.clone();
                        let tool_refs: Vec<Box<dyn Tool>> = tools
                            .into_iter()
                            .map(|definition| {
                                Box::new(DefinitionOnlyTool(definition)) as Box<dyn Tool>
                            })
                            .collect();
                        let borrowed: Vec<&dyn Tool> =
                            tool_refs.iter().map(|tool| tool.as_ref()).collect();
                        model
                            .stream(&request.messages, &borrowed, &request.config)
                            .collect::<Vec<_>>()
                            .await
                    });
                    Ok(LlmProxyResponse::new(events))
                })
            },
        )
        .expect("the runner defaults the audit-log import the guest always declares");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
}

#[test]
fn agent_loop_executes_file_tool_in_guest_without_leaking_host_marker() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let task = "Create out.txt in the granted workspace, then report completion";

    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "create_file",
            json!({"path": "out.txt", "content": "created inside WASI"}),
        ))
        .with_response(make_assistant_message(FINAL_RESPONSE));
    let recorded_mock = mock.clone();
    let model: Arc<dyn LanguageModel> = Arc::new(mock);
    let authorization_headers = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded_headers = Arc::clone(&authorization_headers);
    let proxied_tool_names = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let recorded_tool_names = Arc::clone(&proxied_tool_names);
    let audit_events = Arc::new(Mutex::new(Vec::<AuditEvent>::new()));
    let recorded_audit_events = Arc::clone(&audit_events);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build one host proxy runtime");
    let runtime_handle = runtime.handle().clone();

    let outcome = SandboxRunner::new()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work.path(), "/work")],
                args: worker_args(task, "/work/result.txt"),
                allowed_domains: Vec::new(),
                limits: RunLimits::default(),
            },
            move |linker| {
                register_llm_proxy(
                    linker,
                    model,
                    authorization_headers,
                    proxied_tool_names,
                    audit_events,
                    runtime_handle,
                )
            },
        )
        .expect("run real agent loop through blocking LLM host proxy");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty(), "stdout: {}", outcome.stdout);
    assert!(outcome.stderr.is_empty(), "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).expect("read guest result"),
        FINAL_RESPONSE
    );
    assert_eq!(
        fs::read_to_string(work.path().join("out.txt")).expect("read guest-created file"),
        "created inside WASI",
        "the create_file tool must execute inside the guest's preopen"
    );
    let audit_events = recorded_audit_events.lock().expect("audit event lock");
    assert_eq!(audit_events.len(), 1);
    assert_eq!(audit_events[0].kind, "tool_call");
    assert_eq!(audit_events[0].tool_name, "create_file");
    assert!(!audit_events[0].is_error);

    let calls = recorded_mock.calls();
    assert_eq!(calls.len(), 2, "the agent must make two model turns");
    let first_prompt = calls[0]
        .iter()
        .map(|message| message.text_content())
        .collect::<Vec<_>>()
        .join("\n");
    let environment_injection = wasm_environment_injection();
    assert!(
        first_prompt.contains(&environment_injection),
        "recorded wasm model request must contain the full environment skill: {first_prompt}"
    );
    assert!(calls[0]
        .iter()
        .any(|message| message.text_content() == task));
    assert!(calls[1]
        .iter()
        .any(|message| message.content.iter().any(|block| block
            .as_tool_result()
            .is_some_and(|(_, _, is_error)| !is_error))));
    let tool_names = recorded_tool_names.lock().expect("tool-name lock");
    assert_eq!(tool_names.len(), 2);
    assert!(tool_names
        .iter()
        .all(|names| names.iter().any(|name| name == "create_file")));
    let actual_tools = tool_names[0].iter().cloned().collect::<BTreeSet<_>>();
    let declared_tools = declared_top_level_tools(&environment_injection);
    assert_eq!(
        declared_tools, actual_tools,
        "wasm-env's machine-readable list must equal the real guest tool definitions"
    );
    assert_eq!(
        recorded_headers.lock().expect("header lock").as_slice(),
        [
            format!("Authorization: Bearer {HOST_MARKER}"),
            format!("Authorization: Bearer {HOST_MARKER}")
        ]
    );

    for entry in fs::read_dir(work.path()).expect("list guest-visible work directory") {
        let path = entry.expect("read work entry").path();
        if path.is_file() {
            let bytes = fs::read(&path).expect("read guest-visible file");
            assert!(
                !bytes
                    .windows(HOST_MARKER.len())
                    .any(|window| window == HOST_MARKER.as_bytes()),
                "host marker leaked into guest-visible file {path:?}"
            );
        }
    }
    assert!(!outcome.stdout.contains(HOST_MARKER));
    assert!(!outcome.stderr.contains(HOST_MARKER));
}

/// An empty completion is a legitimate turn outcome, so the `(0, 0)` ptr/len
/// encoding must round-trip as an empty result file — not a guest trap.
#[test]
fn empty_completion_round_trips_without_trapping() {
    let work = tempfile::tempdir().expect("create granted work directory");

    // An empty stream triggers the kernel's documented complete() fallback,
    // so queue one response for stream() and one for complete().
    let model: Arc<dyn LanguageModel> = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message(""))
            .with_response(make_assistant_message("")),
    );
    let headers = Arc::new(Mutex::new(Vec::<String>::new()));
    let tool_names = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build one host proxy runtime");
    let runtime_handle = runtime.handle().clone();

    let outcome = SandboxRunner::new()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work.path(), "/work")],
                args: worker_args("task", "/work/result.txt"),
                allowed_domains: Vec::new(),
                limits: RunLimits::default(),
            },
            move |linker| {
                register_llm_proxy(
                    linker,
                    model,
                    headers,
                    tool_names,
                    Arc::new(Mutex::new(Vec::new())),
                    runtime_handle,
                )
            },
        )
        .expect("an empty completion is an outcome, not a runner failure");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).expect("read guest result"),
        ""
    );
}

#[test]
fn denied_path_error_reaches_model_and_final_reason_reaches_caller() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let final_reason = "Task failed: /outside/secret.txt is outside the granted workspace";
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "read_file",
            json!({"path": "/outside/secret.txt"}),
        ))
        .with_response(make_assistant_message(final_reason));
    let recorded_mock = mock.clone();
    let model: Arc<dyn LanguageModel> = Arc::new(mock);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build one host proxy runtime");
    let runtime_handle = runtime.handle().clone();

    let outcome = SandboxRunner::new()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work.path(), "/work")],
                args: worker_args(
                    "Read /outside/secret.txt and report why the task cannot be completed",
                    "/work/result.txt",
                ),
                allowed_domains: Vec::new(),
                limits: RunLimits::default(),
            },
            move |linker| {
                register_llm_proxy(
                    linker,
                    model,
                    Arc::new(Mutex::new(Vec::new())),
                    Arc::new(Mutex::new(Vec::new())),
                    Arc::new(Mutex::new(Vec::new())),
                    runtime_handle,
                )
            },
        )
        .expect("worker returns the model's explicit failure reason");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).expect("read worker result"),
        final_reason
    );
    let calls = recorded_mock.calls();
    assert_eq!(calls.len(), 2);
    let error_text = calls[1]
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| block.as_tool_result())
        .find_map(|(_, content, is_error)| {
            is_error.then(|| {
                content
                    .iter()
                    .filter_map(|item| item.as_text())
                    .collect::<String>()
            })
        })
        .expect("the second model turn sees an error tool result");
    assert!(
        error_text.contains("outside/secret.txt") || error_text.contains("failed to find"),
        "tool error should identify the denied path: {error_text}"
    );
}

#[test]
fn run_script_batches_file_mutations_in_one_tool_call() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"
                mkdir("nested");
                writeFile("one.txt", "one");
                writeJson("nested/two.json", { value: 2 });
                appendFile("one.txt", "+more");
                copyFile("one.txt", "nested/copied.txt");
                "batch-ok";
            "#}),
        ))
        .with_response(make_assistant_message("batch complete"));
    let (outcome, recorded) = run_mock_worker(&work, mock, "batch files with JavaScript");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("one.txt")).unwrap(),
        "one+more"
    );
    assert_eq!(
        fs::read_to_string(work.path().join("nested/copied.txt")).unwrap(),
        "one+more"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(work.path().join("nested/two.json")).unwrap()
        )
        .unwrap(),
        json!({"value": 2})
    );
    let calls = recorded.calls();
    assert_eq!(calls.len(), 2, "one script call plus final turn");
    let (text, is_error) = first_tool_result(&calls[1]);
    assert!(!is_error, "{text}");
    assert!(text.contains("batch-ok"), "{text}");
}

#[test]
fn run_script_file_bindings_cannot_escape_preopens() {
    let root = tempfile::tempdir().expect("create test root");
    let work = root.path().join("work");
    let outside = root.path().join("outside");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let outside_file = outside.join("escaped.txt");
    fs::write(&outside_file, "unchanged").unwrap();

    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"writeFile("/outside/escaped.txt", "escaped")"#}),
        ))
        .with_response(make_assistant_message("escape was blocked"));
    let (outcome, recorded) = run_mock_worker_path(&work, mock, "try denied script write");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(fs::read_to_string(&outside_file).unwrap(), "unchanged");
    let calls = recorded.calls();
    let (text, is_error) = first_tool_result(&calls[1]);
    assert!(is_error, "denied binding unexpectedly succeeded: {text}");
    assert!(
        text.contains("outside/escaped.txt") || text.contains("failed to find"),
        "{text}"
    );
}

#[test]
fn run_script_timeout_is_readable_and_agent_continues() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": "while (true) {}"}),
        ))
        .with_response(make_assistant_message("agent recovered after timeout"));
    let (outcome, recorded) = run_mock_worker(&work, mock, "run a bounded script");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).unwrap(),
        "agent recovered after timeout"
    );
    let calls = recorded.calls();
    assert_eq!(calls.len(), 2, "agent must make a post-timeout model turn");
    let (text, is_error) = first_tool_result(&calls[1]);
    assert!(is_error, "{text}");
    assert!(text.contains("timed out"), "{text}");
}

#[test]
fn run_script_memory_limit_is_readable_and_agent_continues() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"
                const chunks = [];
                while (true) chunks.push(new ArrayBuffer(1024 * 1024));
            "#}),
        ))
        .with_response(make_assistant_message("agent recovered after memory limit"));
    let (outcome, recorded) = run_mock_worker(&work, mock, "run a memory bounded script");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).unwrap(),
        "agent recovered after memory limit"
    );
    let calls = recorded.calls();
    assert_eq!(calls.len(), 2, "agent must make a post-OOM model turn");
    let (text, is_error) = first_tool_result(&calls[1]);
    assert!(is_error, "{text}");
    assert!(text.to_ascii_lowercase().contains("memory limit"), "{text}");
}

#[test]
fn run_script_has_neither_require_nor_import() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"require("missing")"#}),
        ))
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"import value from "missing";"#}),
        ))
        .with_response(make_assistant_message("modules unavailable"));
    let (outcome, recorded) = run_mock_worker(&work, mock, "try unavailable modules");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    let calls = recorded.calls();
    assert_eq!(calls.len(), 3);
    let (require_error, require_is_error) = first_tool_result(&calls[1]);
    let (import_error, import_is_error) = first_tool_result(&calls[2]);
    assert!(require_is_error, "{require_error}");
    assert!(require_error.contains("require"), "{require_error}");
    assert!(import_is_error, "{import_error}");
    assert!(
        import_error.contains("Unexpected identifier")
            || import_error.contains("SyntaxError")
            || import_error.contains("import"),
        "static import must be rejected as script syntax: {import_error}"
    );
}

#[test]
fn empty_domain_allowlist_is_catchable_while_llm_proxy_still_works() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"
                try {
                  fetch("http://blocked.invalid/data");
                  "unexpected fetch success";
                } catch (error) {
                  `caught: ${String(error)}`;
                }
            "#}),
        ))
        .with_response(make_assistant_message("LLM channel remained available"));
    let (outcome, recorded) = run_mock_worker(&work, mock, "test independent channels");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        recorded.calls().len(),
        2,
        "LLM proxy must complete both turns"
    );
    let (text, is_error) = first_tool_result(&recorded.calls()[1]);
    assert!(
        !is_error,
        "the script should catch the fetch denial: {text}"
    );
    assert!(text.contains("caught:"), "{text}");
    assert!(text.contains("not in the job domain allowlist"), "{text}");
}

#[test]
fn allowed_loopback_domain_fetches_from_local_server() {
    // See the note in the redirect test: warm the guest build first.
    let _ = worker_wasm();
    let Some((url, server)) = start_one_response_server(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nallowed-body",
    ) else {
        return;
    };
    let work = tempfile::tempdir().expect("create granted work directory");
    let script =
        format!(r#"const response = fetch({url:?}); `${{response.status}}:${{response.body}}`;"#);
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": script}),
        ))
        .with_response(make_assistant_message("fetch complete"));
    let (outcome, recorded, audit_events) = run_mock_worker_with_domains_and_audit(
        work.path(),
        mock,
        "fetch allowed local URL",
        vec!["127.0.0.1".to_string()],
    );
    server.join().expect("local fetch server completed");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    let (text, is_error) = first_tool_result(&recorded.calls()[1]);
    assert!(!is_error, "{text}");
    assert!(text.contains("200:allowed-body"), "{text}");
    let audit_events = audit_events.lock().expect("audit event lock");
    assert_eq!(
        audit_events
            .iter()
            .map(|event| event.tool_name.as_str())
            .collect::<Vec<_>>(),
        ["fetch", "run_script"]
    );
    assert!(audit_events[0].result_summary.contains("HTTP 200"));
}

#[test]
fn explicit_allowlist_still_rejects_other_domains() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": r#"
                try { fetch("http://blocked.invalid/data"); "unexpected"; }
                catch (error) { String(error); }
            "#}),
        ))
        .with_response(make_assistant_message("outside domain rejected"));
    let (outcome, recorded, audit_events) = run_mock_worker_with_domains_and_audit(
        work.path(),
        mock,
        "reject outside domain",
        vec!["example.com".to_string()],
    );

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    let (text, is_error) = first_tool_result(&recorded.calls()[1]);
    assert!(!is_error, "the script should catch the denial: {text}");
    assert!(text.contains("blocked.invalid"), "{text}");
    assert!(text.contains("not in the job domain allowlist"), "{text}");
    let audit_events = audit_events.lock().expect("audit event lock");
    assert_eq!(
        audit_events
            .iter()
            .map(|event| event.tool_name.as_str())
            .collect::<Vec<_>>(),
        ["fetch", "run_script"]
    );
    assert!(audit_events[0].is_error);
    assert!(audit_events[0]
        .result_summary
        .contains("not in the job domain allowlist"));
}

#[test]
fn redirect_to_unlisted_domain_is_blocked_before_second_hop() {
    // Build the guest before the server starts: the module carries QuickJS and
    // takes tens of seconds to compile cold, which would otherwise burn the
    // server's accept deadline before the guest ever reaches its fetch.
    let _ = worker_wasm();
    let Some((url, server)) = start_one_response_server(
        "HTTP/1.1 302 Found\r\nLocation: http://blocked.invalid/escaped\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ) else {
        return;
    };
    let work = tempfile::tempdir().expect("create granted work directory");
    let script =
        format!(r#"try {{ fetch({url:?}); "unexpected"; }} catch (error) {{ String(error); }}"#);
    let mock = MockLanguageModel::new()
        .with_response(make_tool_call_message(
            "run_script",
            json!({"script": script}),
        ))
        .with_response(make_assistant_message("redirect blocked"));
    let (outcome, recorded) = run_mock_worker_with_domains(
        work.path(),
        mock,
        "reject redirect allowlist escape",
        vec!["127.0.0.1".to_string()],
    );
    server.join().expect("redirect server completed");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    let (text, is_error) = first_tool_result(&recorded.calls()[1]);
    assert!(
        !is_error,
        "the script should catch the redirect denial: {text}"
    );
    assert!(text.contains("blocked.invalid"), "{text}");
    assert!(text.contains("not in the job domain allowlist"), "{text}");
}

/// How long the one-shot fetch server waits for the guest to connect.
///
/// This is a watchdog, not a latency assertion. Everything between starting the
/// server and the guest's fetch sits inside it: wasmtime compiling a
/// QuickJS-carrying 22MB module, then two agent turns, then QuickJS booting.
/// That is ~55s on this dev machine and roughly 5x that on a 2-core CI runner —
/// the suite takes 105s locally against 525s there — so the previous 120s
/// expired mid-compile and surfaced as "timed out waiting for guest fetch
/// request", which reads like a policy bug rather than an impatient harness.
/// Sized off that measured ratio with room to spare; a healthy run never waits.
const FETCH_SERVER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(600);

/// Serves exactly one response on a loopback port, for tests that need the
/// guest's fetch to reach a real socket.
///
/// The deadline has to cover everything between this call and the guest's
/// fetch: compiling a QuickJS-carrying guest module is tens of seconds on a
/// cold build. If the server gives up first it drops the listener, and the
/// guest's fetch then fails with a bare "Connection refused" that looks like a
/// policy bug rather than an expired harness. Callers should also force the
/// guest build before starting the server; the generous deadline is the
/// backstop, not the plan.
fn start_one_response_server(
    response: &'static str,
) -> Option<(String, std::thread::JoinHandle<()>)> {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("SKIP: sandbox forbids binding a loopback mock HTTP server: {error}");
            return None;
        }
        Err(error) => panic!("bind local fetch server: {error}"),
    };
    listener
        .set_nonblocking(true)
        .expect("make local fetch server nonblocking");
    let url = format!("http://{}/data", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + FETCH_SERVER_DEADLINE;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "timed out waiting for guest fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("accept guest fetch request: {error}"),
            }
        };
        // The listener is non-blocking so the accept loop can honour the
        // deadline, and on macOS the accepted stream inherits that flag — a
        // plain read would then return WouldBlock before the request bytes
        // arrive. Reads are back to blocking, bounded by a socket timeout.
        stream
            .set_nonblocking(false)
            .expect("make accepted fetch stream blocking");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .expect("bound the fetch request read");
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).expect("read guest fetch request");
        assert!(read > 0, "fetch request must contain headers");
        stream
            .write_all(response.as_bytes())
            .expect("write local fetch response");
    });
    Some((url, server))
}

fn run_mock_worker(
    work: &tempfile::TempDir,
    mock: MockLanguageModel,
    task: &str,
) -> (alva_sandbox_wasm::RunOutcome, MockLanguageModel) {
    run_mock_worker_path(work.path(), mock, task)
}

fn run_mock_worker_path(
    work: &Path,
    mock: MockLanguageModel,
    task: &str,
) -> (alva_sandbox_wasm::RunOutcome, MockLanguageModel) {
    run_mock_worker_with_domains(work, mock, task, Vec::new())
}

fn run_mock_worker_with_domains(
    work: &Path,
    mock: MockLanguageModel,
    task: &str,
    allowed_domains: Vec<String>,
) -> (alva_sandbox_wasm::RunOutcome, MockLanguageModel) {
    let (outcome, recorded, _) =
        run_mock_worker_with_domains_and_audit(work, mock, task, allowed_domains);
    (outcome, recorded)
}

fn run_mock_worker_with_domains_and_audit(
    work: &Path,
    mock: MockLanguageModel,
    task: &str,
    allowed_domains: Vec<String>,
) -> (
    alva_sandbox_wasm::RunOutcome,
    MockLanguageModel,
    Arc<Mutex<Vec<AuditEvent>>>,
) {
    let recorded = mock.clone();
    let audit_events = Arc::new(Mutex::new(Vec::new()));
    let proxy_audit_events = Arc::clone(&audit_events);
    let model: Arc<dyn LanguageModel> = Arc::new(mock);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build one host proxy runtime");
    let runtime_handle = runtime.handle().clone();
    let outcome = worker_runner()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work, "/work")],
                args: worker_args(task, "/work/result.txt"),
                allowed_domains,
                limits: RunLimits::default(),
            },
            move |linker| {
                register_llm_proxy(
                    linker,
                    model,
                    Arc::new(Mutex::new(Vec::new())),
                    Arc::new(Mutex::new(Vec::new())),
                    proxy_audit_events,
                    runtime_handle,
                )
            },
        )
        .expect("run worker with mocked host model");
    (outcome, recorded, audit_events)
}

fn first_tool_result(messages: &[alva_kernel_abi::Message]) -> (String, bool) {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| block.as_tool_result())
        .next_back()
        .map(|(_, content, is_error)| {
            (
                content
                    .iter()
                    .filter_map(|item| item.as_text())
                    .collect::<String>(),
                is_error,
            )
        })
        .expect("model turn contains a tool result")
}

struct DefinitionOnlyTool(ToolDefinition);

#[async_trait]
impl Tool for DefinitionOnlyTool {
    fn name(&self) -> &str {
        &self.0.name
    }

    fn description(&self) -> &str {
        &self.0.description
    }

    fn parameters_schema(&self, _ctx: &alva_kernel_abi::ToolSchemaContext) -> serde_json::Value {
        self.0.parameters.clone()
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        Err(AgentError::ToolError {
            tool_name: self.name().to_string(),
            message: "definition-only host proxy tool cannot execute".to_string(),
        })
    }
}

fn register_llm_proxy(
    linker: &mut Linker<SandboxStoreData>,
    model: Arc<dyn LanguageModel>,
    authorization_headers: Arc<Mutex<Vec<String>>>,
    proxied_tool_names: Arc<Mutex<Vec<Vec<String>>>>,
    audit_events: Arc<Mutex<Vec<AuditEvent>>>,
    runtime_handle: tokio::runtime::Handle,
) -> Result<(), wasmtime::Error> {
    let injection = wasm_environment_injection();
    alva_sandbox_wasm::register_wasm_environment_context_proxy(linker, move || {
        Ok(WasmEnvironmentContext::new(injection.clone()))
    })?;
    alva_sandbox_wasm::register_job_log_proxy(linker, move |event| {
        audit_events.lock().expect("audit event lock").push(event);
        Ok(())
    })?;
    alva_sandbox_wasm::register_llm_proxy(linker, move |request: LlmProxyRequest| {
        // This is the host-only credential seam: the marker is consumed
        // while preparing the provider call but never enters response
        // bytes, guest memory, WASI args, or a preopen.
        authorization_headers
            .lock()
            .expect("header lock")
            .push(format!("Authorization: Bearer {HOST_MARKER}"));
        let tools: Vec<Box<dyn Tool>> = request
            .tools
            .into_iter()
            .map(|definition| Box::new(DefinitionOnlyTool(definition)) as Box<dyn Tool>)
            .collect();
        let tool_refs: Vec<&dyn Tool> = tools.iter().map(Box::as_ref).collect();
        proxied_tool_names.lock().expect("tool-name lock").push(
            tool_refs
                .iter()
                .map(|tool| tool.name().to_string())
                .collect(),
        );
        let events: Vec<StreamEvent> = runtime_handle.block_on(async {
            model
                .stream(&request.messages, &tool_refs, &request.config)
                .collect()
                .await
        });
        Ok(LlmProxyResponse::new(events))
    })
}

fn worker_args(task: &str, result: &str) -> Vec<String> {
    vec![
        // argv[0] is the program name per WASI convention; the guest skips it.
        "alva-worker-wasm".into(),
        "--workspace".into(),
        "/work".into(),
        "--task".into(),
        task.into(),
        "--result".into(),
        result.into(),
        "--grant".into(),
        "/work".into(),
        "--permission-mode".into(),
        "bypass".into(),
        "--max-turns".into(),
        "100".into(),
    ]
}

#[test]
fn granted_directory_supports_crud_and_blocks_escape() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    let outside = root.path().join("outside");
    fs::create_dir_all(&granted).expect("create granted directory");
    fs::create_dir_all(&outside).expect("create outside directory");
    fs::write(granted.join("existing.txt"), "before").expect("seed granted file");
    let outside_secret = outside.join("secret.txt");
    fs::write(&outside_secret, "must stay hidden").expect("seed outside file");

    let outcome = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted.clone(), "/work")],
        args: vec![
            outside_secret.to_string_lossy().into_owned(),
            "job-arg".into(),
        ],
        allowed_domains: Vec::new(),
        limits: RunLimits::default(),
    })
    .expect("run fixture module");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty(), "stderr: {}", outcome.stderr);
    // Fuel metering (sampling step, no enforced ceiling yet): a run that does
    // real filesystem work must report nonzero consumption, proving the meter
    // is wired up and producing a plausible, non-trivial value.
    assert!(
        outcome.fuel_consumed > 0,
        "expected nonzero fuel consumption for a guest that does real work"
    );
    assert!(outcome.stdout.contains("READ existing.txt: before"));
    assert!(outcome
        .stdout
        .contains("LIST /work: [\"existing.txt\", \"new.txt\"]"));
    assert!(outcome.stdout.contains("ARGS: job-arg"));
    assert!(outcome.stdout.contains("ESCAPE-1 blocked: NotFound"));
    assert!(outcome
        .stdout
        .contains("ESCAPE-2 blocked: PermissionDenied"));
    assert!(!outcome.stdout.contains("!!!"));

    assert_eq!(
        fs::read_to_string(granted.join("existing.txt")).expect("read overwritten file"),
        "before+modified"
    );
    assert_eq!(
        fs::read_to_string(granted.join("subdir/renamed.txt")).expect("read renamed file"),
        "created-in-sandbox"
    );
    assert!(granted.join("subdir").is_dir());
    assert!(!granted.join("new.txt").exists());
    assert!(!granted.join("tmp-delete-me.txt").exists());
    assert_eq!(
        fs::read_to_string(&outside_secret).expect("read outside file after guest run"),
        "must stay hidden"
    );
}

#[test]
fn wasi_exit_and_stderr_are_returned_as_an_outcome() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    let outside_secret = root.path().join("outside-secret.txt");
    fs::create_dir_all(&granted).expect("create granted directory");
    fs::write(granted.join("existing.txt"), "before").expect("seed granted file");
    fs::write(&outside_secret, "must stay hidden").expect("seed outside file");

    let outcome = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec![
            outside_secret.to_string_lossy().into_owned(),
            "exit-7".into(),
        ],
        allowed_domains: Vec::new(),
        limits: RunLimits::default(),
    })
    .expect("WASI proc_exit is a process outcome, not a runner failure");

    assert_eq!(outcome.exit_code, 7);
    assert!(outcome.stderr.contains("fixture requested exit 7"));
}

#[test]
fn read_only_grant_blocks_mutation() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    let outside_secret = root.path().join("outside-secret.txt");
    fs::create_dir_all(&granted).expect("create granted directory");
    fs::write(granted.join("existing.txt"), "before").expect("seed granted file");
    fs::write(&outside_secret, "must stay hidden").expect("seed outside file");

    // The fixture's first action is `fs::write("/work/new.txt", ...)`; under a
    // read-only mount that write must fail, so the guest traps rather than
    // exiting cleanly, and the host directory is left untouched.
    let result = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_only(granted.clone(), "/work")],
        args: vec![outside_secret.to_string_lossy().into_owned(), "job".into()],
        allowed_domains: Vec::new(),
        limits: RunLimits::default(),
    });

    assert!(
        result.is_err() || result.as_ref().is_ok_and(|o| o.exit_code != 0),
        "read-only mount let the guest exit cleanly: {result:?}"
    );
    assert!(
        !granted.join("new.txt").exists(),
        "read-only mount allowed a new file to be created"
    );
    assert_eq!(
        fs::read_to_string(granted.join("existing.txt")).expect("existing file survives"),
        "before",
        "read-only mount allowed an overwrite"
    );
}

#[test]
fn host_epoch_deadline_traps_non_yielding_guest() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    fs::create_dir_all(&granted).unwrap();

    let error = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec!["fixture".into(), "spin".into()],
        allowed_domains: Vec::new(),
        limits: RunLimits {
            wall_clock: std::time::Duration::from_millis(50),
            max_memory_bytes: 64 * 1024 * 1024,
            ..RunLimits::default()
        },
    })
    .expect_err("epoch deadline must interrupt a busy guest");

    assert!(error.to_string().contains("wall-clock limit"), "{error}");
}

/// Measures the wasm-native execution rate of a genuinely non-yielding CPU
/// spin, in fuel units per second of real wall-clock time. This is the
/// evidence `DEFAULT_WASM_FUEL` in `alva-sandbox-wasm/src/lib.rs` is derived
/// from.
///
/// Method: give the spin fixture a fuel budget and a wall clock generous
/// enough that `Trap::OutOfFuel`, not the epoch deadline, is what trips;
/// then divide the (fixed, known) fuel budget by real elapsed time around
/// the call. The budget is large (5e10) so that module
/// compilation/instantiation overhead — which this wall-clock measurement,
/// unlike the host's own internal `elapsed_ms`, cannot subtract out — is
/// negligible next to the multi-second timed window; this trades a slightly
/// pessimistic (i.e. safely conservative) rate estimate for a test that
/// doesn't depend on capturing the host's `tracing` output, which is
/// unreliable here: `tracing`'s per-callsite interest cache is shared
/// process-wide, and this test binary runs many `#[test]`s concurrently
/// that all execute the same `tracing::info!` call site in
/// `run_with_imports` without a subscriber installed, which can memoize
/// that callsite as uninteresting before a thread-scoped
/// `tracing::subscriber::with_default` in this test gets a chance to
/// observe it — confirmed by this test flaking exactly that way under
/// `cargo test` (all tests, parallel) while passing every time run alone.
///
/// An earlier version of this test measured the rate the other way around:
/// small *wall-clock* deadline, read `fuel_consumed` off the resulting
/// `Trap::Interrupt`. That produced a fuel figure which stayed constant
/// (~19.3k) regardless of whether the configured deadline was 200ms or
/// 1000ms — i.e. it was measuring some fixed guest-startup fuel cost, not
/// loop-iteration throughput, and is not reliable evidence for calibrating a
/// ceiling. Timing a real `Trap::OutOfFuel` avoids that confound: the
/// elapsed time is bounded by "how long did it take to spend this many fuel
/// units", which is precisely the question the ceiling's derivation needs
/// answered.
#[test]
fn spin_fuel_rate_is_within_expected_order_of_magnitude() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    fs::create_dir_all(&granted).unwrap();

    let fuel_budget = 50_000_000_000;
    let started = std::time::Instant::now();
    let error = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec!["fixture".into(), "spin".into()],
        allowed_domains: Vec::new(),
        limits: RunLimits {
            // Generous enough that the fuel budget above, not this
            // deadline, is guaranteed to be what trips.
            wall_clock: std::time::Duration::from_secs(30),
            max_memory_bytes: 64 * 1024 * 1024,
            fuel: fuel_budget,
        },
    })
    .expect_err("fuel budget must trap the spin fixture");
    // Includes module compile + instantiate, unlike the host's internal
    // `elapsed_ms`; see the doc comment above for why that tradeoff is
    // deliberate here.
    let elapsed = started.elapsed();

    assert!(
        error.to_string().contains("fuel budget"),
        "expected an out-of-fuel trap, not a wall-clock one: {error}"
    );
    assert_eq!(error.fuel_consumed(), Some(fuel_budget));

    let fuel_per_second = fuel_budget as f64 / elapsed.as_secs_f64();
    eprintln!(
        "measured wasm-native spin rate: {fuel_budget} fuel in {elapsed:?} => \
         {fuel_per_second:.0} fuel/sec (includes module compile+instantiate \
         overhead, so this understates the guest's true rate)"
    );

    // Sanity bounds wide enough to tolerate machine-speed variance across CI
    // hardware, and the compile/instantiate overhead baked into this
    // measurement, while still catching a metering regression (fuel not
    // wired up, wrong units, a build that stops actually spinning). Isolated
    // local measurement of *just* the guest call (excluding compile
    // overhead), at several fuel-budget magnitudes (2e9, 1e10, 5e10),
    // consistently landed in the 1.7e10 - 3.2e10 fuel/sec range; these
    // bounds give roughly 10x headroom on either side of that for slower or
    // faster CI hardware plus this measurement's extra overhead.
    assert!(
        fuel_per_second > 1_000_000_000.0,
        "spin rate implausibly low, fuel metering may be broken: {fuel_per_second} fuel/sec"
    );
    assert!(
        fuel_per_second < 300_000_000_000.0,
        "spin rate implausibly high, sanity check the measurement: {fuel_per_second} fuel/sec"
    );
}

/// A small fuel budget must cut off a non-yielding guest well before its
/// generous wall-clock allowance would, proving the fuel ceiling is a real,
/// enforced cutoff and not just the observational metering from the prior
/// step.
#[test]
fn host_fuel_budget_traps_non_yielding_guest() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    fs::create_dir_all(&granted).unwrap();

    let small_fuel = 200_000;
    let error = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec!["fixture".into(), "spin".into()],
        allowed_domains: Vec::new(),
        limits: RunLimits {
            // Generous enough that if the fuel ceiling did not work, this
            // test would hang for a very long time instead of quietly
            // passing for the wrong reason.
            wall_clock: std::time::Duration::from_secs(30),
            max_memory_bytes: 64 * 1024 * 1024,
            fuel: small_fuel,
        },
    })
    .expect_err("fuel budget must trap a non-yielding guest");

    assert!(
        error.to_string().contains("fuel budget"),
        "expected an out-of-fuel error, not a wall-clock trap: {error}"
    );
    assert!(
        error.to_string().contains(&small_fuel.to_string()),
        "{error}"
    );
    assert_eq!(
        error.fuel_consumed(),
        Some(small_fuel),
        "an out-of-fuel trap must report consuming the entire budget"
    );
}

/// A normal (non-spinning) fixture run must stay far under the default fuel
/// ceiling, proving legitimate workloads have headroom.
#[test]
fn default_fuel_budget_does_not_trip_a_normal_run() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    fs::create_dir_all(&granted).unwrap();
    fs::write(granted.join("existing.txt"), "before").unwrap();

    let outcome = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec!["fixture".into(), "job".into()],
        allowed_domains: Vec::new(),
        limits: RunLimits::default(),
    })
    .expect("a normal fixture run must not trip the default fuel budget");

    assert_eq!(outcome.exit_code, 0);
    assert!(
        outcome.fuel_consumed < RunLimits::default().fuel / 30,
        "a normal fixture run should have at least 30x headroom under the default fuel \
         ceiling, got {} of {} fuel",
        outcome.fuel_consumed,
        RunLimits::default().fuel
    );
}

#[test]
fn host_store_limiter_traps_linear_memory_growth() {
    let root = tempfile::tempdir().expect("create test root");
    let granted = root.path().join("granted");
    fs::create_dir_all(&granted).unwrap();

    let error = run_module(RunRequest {
        module: fixture_wasm().to_vec(),
        grants: vec![Grant::read_write(granted, "/work")],
        args: vec!["fixture".into(), "grow-memory".into()],
        allowed_domains: Vec::new(),
        limits: RunLimits {
            wall_clock: std::time::Duration::from_secs(5),
            max_memory_bytes: 8 * 1024 * 1024,
            ..RunLimits::default()
        },
    })
    .expect_err("store limiter must interrupt memory growth");

    assert!(error.to_string().contains("memory limit"), "{error}");
}

fn fixture_wasm() -> &'static [u8] {
    FIXTURE_WASM.get_or_init(build_fixture).as_slice()
}

fn worker_wasm() -> &'static [u8] {
    WORKER_WASM.get_or_init(build_worker).as_slice()
}

fn worker_runner() -> &'static SandboxRunner {
    WORKER_RUNNER.get_or_init(SandboxRunner::new)
}

/// Compiles a wasip1 guest on demand and returns its bytes.
///
/// One helper for every guest we build: the artifact filename keeps the
/// package name verbatim — **hyphens are not converted to underscores** for
/// bin crates (that conversion only applies to lib identifiers). Getting this
/// wrong panics at `fs::read`; keeping the rule in one place keeps it right.
fn build_guest(manifest: &Path, package: &str) -> Vec<u8> {
    let target_dir = tempfile::tempdir().expect("create guest target directory");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));

    let mut command = Command::new(cargo);
    // The guest is test *data* — a wasm artifact handed to wasmtime — not code
    // whose coverage we measure. `cargo llvm-cov` does not use RUSTFLAGS; it
    // installs itself as RUSTC_WRAPPER and instruments the crates named in
    // __CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES, which includes
    // alva_worker_wasm. A nested cargo inherits that wrapper and tries to
    // instrument the guest, but wasm32-wasip1 ships no profiler runtime, so the
    // build dies with "can't find crate for `profiler_builtins`". Strip the
    // whole mechanism so this build does not depend on how the test binary
    // itself was compiled.
    for instrumented in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "__CARGO_LLVM_COV_RUSTC_WRAPPER",
        "__CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS",
        "__CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES",
        "CARGO_LLVM_COV",
        "CARGO_LLVM_COV_TARGET_DIR",
        "CARGO_LLVM_COV_BUILD_DIR",
        "LLVM_PROFILE_FILE",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        command.env_remove(instrumented);
    }
    if package == "alva-worker-wasm" {
        command.env("WASI_SDK", cached_wasi_sdk());
    }
    let output = command
        .arg("build")
        .arg("--offline")
        .arg("--locked")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip1")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("-p")
        .arg(package)
        .arg("--target-dir")
        .arg(target_dir.path())
        .output()
        .unwrap_or_else(|error| panic!("spawn cargo to build {package}: {error}"));

    assert!(
        output.status.success(),
        "{package} build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let wasm = target_dir
        .path()
        .join("wasm32-wasip1")
        .join("release")
        .join(format!("{package}.wasm"));
    fs::read(&wasm).unwrap_or_else(|error| panic!("read {package} wasm at {wasm:?}: {error}"))
}

fn workspace_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("sandbox crate is under workspace/crates")
        .join("Cargo.toml")
}

fn build_worker() -> Vec<u8> {
    build_guest(&workspace_manifest(), "alva-worker-wasm")
}

fn cached_wasi_sdk() -> PathBuf {
    // Prefer an explicitly installed SDK. CI sets WASI_SDK to a cached install
    // so rquickjs-sys's 113MB download happens once (on a cache miss), not on
    // every cold build; this build then reuses it too. Without the env we fall
    // back to scanning whatever a prior rquickjs-sys download left in OUT_DIR,
    // which is the local-dev path and keeps behavior unchanged there.
    if let Some(explicit) = std::env::var_os("WASI_SDK") {
        let path = PathBuf::from(explicit);
        if path.join("bin/clang").is_file() {
            return path;
        }
    }
    let build_root = workspace_manifest()
        .parent()
        .expect("workspace manifest has a parent")
        .join("target/wasm32-wasip1/debug/build");
    fs::read_dir(&build_root)
        .unwrap_or_else(|error| panic!("read cached WASI build directory {build_root:?}: {error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("out/wasi-sdk"))
        .find(|path| path.join("bin/clang").is_file())
        .unwrap_or_else(|| {
            panic!(
                "cached rquickjs WASI SDK not found under {build_root:?}; run `cargo build --offline -p alva-worker-wasm --target wasm32-wasip1` first"
            )
        })
}

fn build_fixture() -> Vec<u8> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fs-guest/Cargo.toml");
    build_guest(&manifest, "alva-sandbox-wasm-fixture")
}
