// INPUT:  std::{fs, io, net, process::Command, sync::{Arc, Mutex, OnceLock}}, alva_kernel_abi, alva_llm_wire, alva_sandbox_wasm, alva_test, async_trait, futures, serde_json, tempfile, tokio, wasmtime
// OUTPUT: Integration coverage for runner limits, preopens, independent LLM/fetch proxies, domain policy, and run_script
// POS:    Builds WASIp1 guests on demand and verifies host filesystem/network isolation plus guest QuickJS behavior.

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
    run_module, Grant, RunLimits, RunRequest, SandboxRunner, SandboxStoreData,
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

    let calls = recorded_mock.calls();
    assert_eq!(calls.len(), 2, "the agent must make two model turns");
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
            move |linker| register_llm_proxy(linker, model, headers, tool_names, runtime_handle),
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
    let (outcome, recorded) = run_mock_worker_with_domains(
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
    let (outcome, recorded) = run_mock_worker_with_domains(
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
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
    let recorded = mock.clone();
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
                    runtime_handle,
                )
            },
        )
        .expect("run worker with mocked host model");
    (outcome, recorded)
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

    fn parameters_schema(&self) -> serde_json::Value {
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
    runtime_handle: tokio::runtime::Handle,
) -> Result<(), wasmtime::Error> {
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
        },
    })
    .expect_err("epoch deadline must interrupt a busy guest");

    assert!(error.to_string().contains("wall-clock limit"), "{error}");
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
