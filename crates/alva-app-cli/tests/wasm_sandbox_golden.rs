// INPUT:  real alva binary, built alva-worker-wasm.wasm sidecar, local recording OpenAI SSE server, tempfile
// OUTPUT: CLI wasm-tier golden coverage for file/domain flags, host-only auth, file-tool loop, JSON output, and optional real provider
// POS:    End-to-end contract proving provider HTTP stays native while the agent/file tools execute in WASIp1.

// The OS tier's flag name is platform-specific (macOS os-write / Linux os),
// so the "legal values" list the CLI prints differs per platform. This golden
// is not cfg-gated, so it must expect the local name.
#[cfg(target_os = "linux")]
const OS_TIER: &str = "os";
#[cfg(not(target_os = "linux"))]
const OS_TIER: &str = "os-write";

use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const TEST_KEY: &str = "wasm-host-only-test-key";

#[derive(Debug)]
struct RecordedRequest {
    headers: String,
    body: String,
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (
        tempfile::tempdir().expect("home tempdir"),
        tempfile::tempdir().expect("workspace tempdir"),
    )
}

fn alva(home: &tempfile::TempDir, workspace: &tempfile::TempDir) -> Command {
    let mut command = Command::cargo_bin("alva").expect("alva binary builds");
    command
        .current_dir(workspace.path())
        .env("HOME", home.path())
        .env("ALVA_CONFIG_DIR", home.path().join(".alva"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("ALVA_WORKER_WASM", worker_wasm())
        .env_remove("ALVA_API_KEY")
        .env_remove("ALVA_MODEL")
        .env_remove("ALVA_BASE_URL")
        .env_remove("ALVA_PROVIDER_KIND")
        .env_remove("ALVA_UI_MODE")
        .env_remove("ALVA_REPL");
    command
}

fn write_shared_config(home: &tempfile::TempDir, base_url: &str) {
    let alva_dir = home.path().join(".alva");
    std::fs::create_dir_all(&alva_dir).unwrap();
    std::fs::write(
        alva_dir.join("config.json"),
        serde_json::json!({
            "providers": {
                "openai-chat": {
                    "api_key": TEST_KEY,
                    "model": "mock-model",
                    "base_url": base_url,
                }
            },
            "active": "openai-chat"
        })
        .to_string(),
    )
    .unwrap();
}

fn start_recording_tool_server() -> (
    String,
    Arc<Mutex<Vec<RecordedRequest>>>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind recording server");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let records = Arc::new(Mutex::new(Vec::new()));
    let server_records = Arc::clone(&records);
    let handle = std::thread::spawn(move || {
        for turn in 0..3 {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let (headers, body) = read_http_request(&mut stream);
            server_records
                .lock()
                .expect("record lock")
                .push(RecordedRequest { headers, body });

            let frames = match turn {
                0 => vec![
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-read","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]}}]}"#,
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                ],
                1 => vec![
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-write","type":"function","function":{"name":"create_file","arguments":"{\"path\":\"b.txt\",\"content\":\"HELLO WASM\"}"}}]}}]}"#,
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                ],
                _ => vec![
                    r#"data: {"id":"c3","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c3","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Wrote uppercase content to b.txt"},"finish_reason":"stop"}]}"#,
                ],
            };
            let sse = frames
                .into_iter()
                .chain(["data: [DONE]"])
                .collect::<Vec<_>>()
                .join("\n\n");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{sse}\n\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (url, records, handle)
}

fn read_http_request(stream: &mut std::net::TcpStream) -> (String, String) {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read provider request");
        assert!(read > 0, "provider request ended before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut chunk).expect("read provider request body");
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    (
        headers,
        String::from_utf8_lossy(&bytes[header_end..header_end + content_length]).to_string(),
    )
}

#[test]
fn wasm_tier_runs_file_loop_through_host_provider_and_emits_json() {
    let (home, workspace) = dirs();
    std::fs::write(workspace.path().join("a.txt"), "hello wasm").unwrap();
    let (url, records, server) = start_recording_tool_server();
    write_shared_config(&home, &format!("{url}/v1"));

    let assert = alva(&home, &workspace)
        .args([
            "-p",
            "--sandbox",
            "wasm",
            "--grant",
            workspace.path().to_str().unwrap(),
            "--output-format",
            "json",
            "Read a.txt, uppercase it, and write b.txt",
        ])
        .assert()
        .success();
    server.join().expect("recording server completed");

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("one JSON result object");
    assert_eq!(result["type"], "result");
    assert_eq!(result["is_error"], false);
    assert_eq!(result["result"], "Wrote uppercase content to b.txt");
    assert!(
        result["session_id"].is_null(),
        "one-shot wasm runs are not resumable"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("b.txt")).unwrap(),
        "HELLO WASM"
    );

    let records = records.lock().expect("record lock");
    assert_eq!(records.len(), 3, "read, write, final model turns");
    assert!(records.iter().all(|request| request
        .headers
        .to_ascii_lowercase()
        .contains(&format!("authorization: bearer {TEST_KEY}"))));
    assert!(records[0].body.contains("read_file"));
    assert!(records[1].body.contains("hello wasm"));
    assert!(records[2].body.contains("create_file"));

    for entry in std::fs::read_dir(workspace.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(
                !std::fs::read(&path)
                    .unwrap()
                    .windows(TEST_KEY.len())
                    .any(|window| window == TEST_KEY.as_bytes()),
                "host key leaked into guest-visible file {}",
                path.display()
            );
        }
    }
    assert!(!assert
        .get_output()
        .stdout
        .windows(TEST_KEY.len())
        .any(|window| window == TEST_KEY.as_bytes()));
    assert!(!assert
        .get_output()
        .stderr
        .windows(TEST_KEY.len())
        .any(|window| window == TEST_KEY.as_bytes()));
    assert!(!std::fs::read(worker_wasm())
        .unwrap()
        .windows(TEST_KEY.len())
        .any(|window| window == TEST_KEY.as_bytes()));
}

#[test]
fn unknown_sandbox_tier_fails_before_provider_setup() {
    let (home, workspace) = dirs();
    alva(&home, &workspace)
        .args([
            "-p",
            "--sandbox",
            "native",
            "--grant",
            workspace.path().to_str().unwrap(),
            "task",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains(format!(
            "legal values: wasm|{OS_TIER}"
        )));
}

#[test]
fn nonexistent_grant_fails_before_provider_setup() {
    let (home, workspace) = dirs();
    let missing = workspace.path().join("missing");
    alva(&home, &workspace)
        .args([
            "-p",
            "--sandbox",
            "wasm",
            "--grant",
            missing.to_str().unwrap(),
            "task",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("does not exist"));
}

#[test]
fn wasm_tier_without_grant_fails_before_provider_setup() {
    let (home, workspace) = dirs();
    alva(&home, &workspace)
        .args(["-p", "--sandbox", "wasm", "task"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("requires at least one --grant"));
}

#[test]
fn invalid_allow_domain_fails_before_provider_setup() {
    let (home, workspace) = dirs();
    alva(&home, &workspace)
        .args([
            "-p",
            "--sandbox",
            "wasm",
            "--grant",
            workspace.path().to_str().unwrap(),
            "--allow-domain",
            "https://example.com",
            "task",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("invalid --allow-domain"));
}

#[test]
fn allow_domain_without_wasm_tier_fails_before_provider_setup() {
    let (home, workspace) = dirs();
    alva(&home, &workspace)
        .args(["-p", "--allow-domain", "example.com", "task"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains(
            "--allow-domain requires --sandbox wasm",
        ));
}

/// Manual-only real provider smoke:
/// ALVA_TEST_API_KEY=... ALVA_TEST_MODEL=... [ALVA_TEST_BASE_URL=...]
/// [ALVA_TEST_PROVIDER_KIND=openai-chat] cargo test -p alva-app-cli
/// --test wasm_sandbox_golden real_provider_wasm_file_roundtrip -- --ignored --nocapture
#[test]
#[ignore = "requires ALVA_TEST_API_KEY and a live provider"]
fn real_provider_wasm_file_roundtrip() {
    let key = std::env::var("ALVA_TEST_API_KEY").expect("ALVA_TEST_API_KEY is required");
    let model = std::env::var("ALVA_TEST_MODEL").expect("ALVA_TEST_MODEL is required");
    let (home, workspace) = dirs();
    std::fs::write(workspace.path().join("a.txt"), "hello from real provider").unwrap();
    let mut command = alva(&home, &workspace);
    command
        .env("ALVA_API_KEY", key)
        .env("ALVA_MODEL", model)
        .env(
            "ALVA_PROVIDER_KIND",
            std::env::var("ALVA_TEST_PROVIDER_KIND").unwrap_or_else(|_| "openai-chat".into()),
        );
    if let Ok(base_url) = std::env::var("ALVA_TEST_BASE_URL") {
        command.env("ALVA_BASE_URL", base_url);
    }
    let assert = command
        .args([
            "-p",
            "--sandbox",
            "wasm",
            "--grant",
            workspace.path().to_str().unwrap(),
            "Read a.txt and write its uppercase content to b.txt. Finish only after verifying b.txt.",
        ])
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("b.txt")).unwrap(),
        "HELLO FROM REAL PROVIDER"
    );
    eprintln!("{}", String::from_utf8_lossy(&assert.get_output().stdout));
}

fn worker_wasm() -> &'static Path {
    static WORKER: OnceLock<PathBuf> = OnceLock::new();
    WORKER.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("CLI crate is under workspace/crates");
        let path = workspace
            .join("target")
            .join("wasm32-wasip1")
            .join("debug")
            .join("alva-worker-wasm.wasm");
        if !path.is_file() {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let output = std::process::Command::new(cargo)
                .current_dir(workspace)
                .args([
                    "build",
                    "--offline",
                    "-p",
                    "alva-worker-wasm",
                    "--target",
                    "wasm32-wasip1",
                ])
                .output()
                .expect("build alva-worker-wasm");
            assert!(
                output.status.success(),
                "worker build failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        path.canonicalize().expect("canonical worker wasm path")
    })
}
