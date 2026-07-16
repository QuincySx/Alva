// INPUT:  std::{fs, process::Command, sync::{Arc, Mutex, OnceLock}}, alva_kernel_abi, alva_sandbox_wasm, alva_test, tempfile, tokio, wasmtime
// OUTPUT: Integration coverage for the public WASIp1 runner seam
// POS:    Builds the fixture on demand and verifies only guest-visible output plus host filesystem effects.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use alva_kernel_abi::{LanguageModel, Message, ModelConfig};
use alva_sandbox_wasm::{run_module, Grant, RunRequest, SandboxRunner};
use alva_test::fixtures::make_assistant_message;
use alva_test::mock_provider::MockLanguageModel;
use wasmtime::{Caller, Extern, Linker};
use wasmtime_wasi::p1::WasiP1Ctx;

static FIXTURE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static WORKER_WASM: OnceLock<Vec<u8>> = OnceLock::new();

const HOST_MARKER: &str = "SECRET-KEY-abc";
const MOCK_RESPONSE: &str = "mock completion from host";

#[test]
fn blocking_llm_proxy_round_trips_ptr_len_without_leaking_host_marker() {
    let work = tempfile::tempdir().expect("create granted work directory");
    let task = "Summarize the granted task";
    fs::write(work.path().join("task.txt"), task).expect("seed guest task");

    let mock = MockLanguageModel::new().with_response(make_assistant_message(MOCK_RESPONSE));
    let recorded_mock = mock.clone();
    let model: Arc<dyn LanguageModel> = Arc::new(mock);
    let authorization_headers = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded_headers = Arc::clone(&authorization_headers);

    let outcome = SandboxRunner::new()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work.path(), "/work")],
                args: Vec::new(),
            },
            move |linker| register_llm_proxy(linker, model, authorization_headers),
        )
        .expect("run worker through blocking LLM host proxy");

    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty(), "stdout: {}", outcome.stdout);
    assert!(outcome.stderr.is_empty(), "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).expect("read guest result"),
        MOCK_RESPONSE
    );

    let calls = recorded_mock.calls();
    assert_eq!(calls.len(), 1, "host mock must receive exactly one request");
    assert_eq!(calls[0].len(), 1);
    assert_eq!(calls[0][0].text_content(), task);
    assert_eq!(
        recorded_headers.lock().expect("header lock").as_slice(),
        [format!("Authorization: Bearer {HOST_MARKER}")]
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
    fs::write(work.path().join("task.txt"), "task").expect("seed guest task");

    let model: Arc<dyn LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message("")));
    let headers = Arc::new(Mutex::new(Vec::<String>::new()));

    let outcome = SandboxRunner::new()
        .run_with_imports(
            RunRequest {
                module: worker_wasm().to_vec(),
                grants: vec![Grant::read_write(work.path(), "/work")],
                args: Vec::new(),
            },
            move |linker| register_llm_proxy(linker, model, headers),
        )
        .expect("an empty completion is an outcome, not a runner failure");

    assert_eq!(outcome.exit_code, 0, "stderr: {}", outcome.stderr);
    assert_eq!(
        fs::read_to_string(work.path().join("result.txt")).expect("read guest result"),
        ""
    );
}

fn register_llm_proxy(
    linker: &mut Linker<WasiP1Ctx>,
    model: Arc<dyn LanguageModel>,
    authorization_headers: Arc<Mutex<Vec<String>>>,
) -> Result<(), wasmtime::Error> {
    linker.func_wrap(
        "alva:host/llm",
        "llm_complete",
        move |mut caller: Caller<'_, WasiP1Ctx>, req_ptr: i32, req_len: i32| {
            let req_start = usize::try_from(req_ptr)
                .map_err(|_| wasmtime::Error::msg("negative request pointer"))?;
            let req_len = usize::try_from(req_len)
                .map_err(|_| wasmtime::Error::msg("negative request length"))?;
            let req_end = req_start
                .checked_add(req_len)
                .ok_or_else(|| wasmtime::Error::msg("request range overflow"))?;
            let memory = caller
                .get_export("memory")
                .and_then(Extern::into_memory)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
            let prompt = memory
                .data(&caller)
                .get(req_start..req_end)
                .ok_or_else(|| wasmtime::Error::msg("request range is outside guest memory"))?;
            let prompt = std::str::from_utf8(prompt)
                .map_err(|error| wasmtime::Error::msg(error.to_string()))?
                .to_owned();

            // This is the host-only credential seam: the marker is consumed
            // while preparing the provider call but never enters response
            // bytes, guest memory, WASI args, or a preopen.
            authorization_headers
                .lock()
                .expect("header lock")
                .push(format!("Authorization: Bearer {HOST_MARKER}"));
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .map_err(|error| wasmtime::Error::msg(error.to_string()))?;
            let completion = runtime
                .block_on(model.complete(&[Message::user(prompt)], &[], &ModelConfig::default()))
                .map_err(|error| wasmtime::Error::msg(error.to_string()))?;
            let response = completion.message.text_content().into_bytes();
            let resp_len = i32::try_from(response.len())
                .map_err(|_| wasmtime::Error::msg("response exceeds ptr/len ABI limit"))?;
            // An empty completion needs no guest allocation: (0, 0) is the
            // agreed encoding and the guest writes an empty result.
            if resp_len == 0 {
                return Ok(0);
            }
            let alloc = caller
                .get_export("alloc")
                .and_then(Extern::into_func)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export alloc"))?
                .typed::<i32, i32>(&caller)?;
            let resp_ptr = alloc.call(&mut caller, resp_len)?;
            let resp_start = usize::try_from(resp_ptr)
                .map_err(|_| wasmtime::Error::msg("guest alloc returned negative pointer"))?;
            memory.write(&mut caller, resp_start, &response)?;

            let packed = (u64::from(resp_ptr as u32) << 32) | u64::from(resp_len as u32);
            Ok(packed as i64)
        },
    )?;
    Ok(())
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
