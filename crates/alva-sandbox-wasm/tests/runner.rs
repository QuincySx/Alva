// INPUT:  std::{fs, process::Command, sync::{Arc, Mutex, OnceLock}}, alva_kernel_abi, alva_llm_wire, alva_sandbox_wasm, alva_test, async_trait, futures, serde_json, tempfile, tokio, wasmtime
// OUTPUT: Integration coverage for the public WASIp1 runner seam
// POS:    Builds WASIp1 guests on demand and verifies sandboxed files plus the structured blocking LLM proxy boundary.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use alva_kernel_abi::{
    AgentError, LanguageModel, StreamEvent, Tool, ToolExecutionContext, ToolOutput,
};
use alva_llm_wire::{LlmProxyRequest, LlmProxyResponse, ToolDefinition};
use alva_sandbox_wasm::{run_module, Grant, RunRequest, SandboxRunner};
use alva_test::fixtures::{make_assistant_message, make_tool_call_message};
use alva_test::mock_provider::MockLanguageModel;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use wasmtime::Linker;
use wasmtime_wasi::p1::WasiP1Ctx;

static FIXTURE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static WORKER_WASM: OnceLock<Vec<u8>> = OnceLock::new();

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
    linker: &mut Linker<WasiP1Ctx>,
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

fn fixture_wasm() -> &'static [u8] {
    FIXTURE_WASM.get_or_init(build_fixture).as_slice()
}

fn worker_wasm() -> &'static [u8] {
    WORKER_WASM.get_or_init(build_worker).as_slice()
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

    let output = Command::new(cargo)
        .arg("build")
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

fn build_fixture() -> Vec<u8> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fs-guest/Cargo.toml");
    build_guest(&manifest, "alva-sandbox-wasm-fixture")
}
