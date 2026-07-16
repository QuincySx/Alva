// INPUT:  assert_cmd (real `alva` binary), built alva-worker-wasm.wasm, std::net (mock OpenAI SSE server), tempfile
// OUTPUT: (none — golden binary tests)
// POS:    Golden coverage for daemonless job lifecycle, wasm flag passthrough, timeout/crash semantics, and host-owned tool audit logs.

use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, OnceLock};

fn alva(home: &tempfile::TempDir, ws: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("alva").expect("alva binary builds");
    cmd.current_dir(ws.path())
        .env("HOME", home.path())
        .env("ALVA_CONFIG_DIR", home.path().join(".alva"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env_remove("ALVA_API_KEY")
        .env_remove("ALVA_MODEL")
        .env_remove("ALVA_BASE_URL")
        .env_remove("ALVA_PROVIDER_KIND");
    cmd
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (
        tempfile::tempdir().expect("home"),
        tempfile::tempdir().expect("ws"),
    )
}

/// Same minimal OpenAI-compatible SSE mock as print_mode_golden.
fn start_mock_openai_server() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = vec![0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let request = String::from_utf8_lossy(&buf[..n]);
            if request.contains("POST") && request.contains("/chat/completions") {
                let sse_body = [
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    "",
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"job done"}}]}"#,
                    "",
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}"#,
                    "",
                    "data: [DONE]",
                    "",
                    "",
                ]
                .join("\n");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                     Cache-Control: no-cache\r\nConnection: close\r\n\r\n{sse_body}"
                );
                let _ = stream.write_all(response.as_bytes());
            } else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        }
    });
    (url, handle)
}

/// Two-turn provider for the wasm jobs golden. The first response is gated so
/// the test can deterministically observe `running` and timeout exit 124 before
/// allowing the worker to execute its file tool and finish.
fn start_gated_wasm_tool_server() -> (String, mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind wasm job server");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (release_tx, release_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        for turn in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            drain_http_request(&mut stream);
            if turn == 0 {
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(60))
                    .expect("test releases gated wasm job");
            }
            let frames = if turn == 0 {
                vec![
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-job-write","type":"function","function":{"name":"create_file","arguments":"{\"path\":\"job-output.txt\",\"content\":\"from job wasm\"}"}}]}}]}"#,
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                ]
            } else {
                vec![
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"job wasm done"},"finish_reason":"stop"}]}"#,
                ]
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
    (url, release_tx, handle)
}

fn drain_http_request(stream: &mut std::net::TcpStream) {
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
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
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
        assert!(read > 0, "provider request body ended early");
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn write_shared_config(home: &tempfile::TempDir, base_url: &str) {
    let alva_dir = home.path().join(".alva");
    std::fs::create_dir_all(&alva_dir).unwrap();
    std::fs::write(
        alva_dir.join("config.json"),
        serde_json::json!({
            "providers": {
                "openai-chat": {
                    "api_key": "test-key",
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

/// The full lifecycle: submit returns a job id immediately; wait blocks and
/// then emits the SAME result object shape as `-p --output-format json`;
/// status afterwards reads completed; result re-reads the stored object.
#[test]
fn jobs_submit_wait_status_result_lifecycle() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();
    write_shared_config(&home, &format!("{url}/v1"));

    // submit → immediate {job_id}
    let submit = alva(&home, &ws)
        .args(["jobs", "submit", "do the long thing"])
        .assert()
        .success();
    let sv: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&submit.get_output().stdout).trim())
            .expect("submit prints a JSON object");
    let job_id = sv["job_id"].as_str().expect("job_id present").to_string();
    assert!(!job_id.is_empty());

    // wait → blocks until done, then prints the -p json result object
    let wait = alva(&home, &ws)
        .args(["jobs", "wait", &job_id])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let wv: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&wait.get_output().stdout).trim())
            .expect("wait prints the result JSON");
    assert_eq!(wv["result"], "job done");
    assert_eq!(wv["is_error"], false);
    assert_eq!(wv["usage"]["input_tokens"], 10);

    // status → completed
    let status = alva(&home, &ws)
        .args(["jobs", "status", &job_id])
        .assert()
        .success();
    let stv: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&status.get_output().stdout).trim()).unwrap();
    assert_eq!(stv["state"], "completed");

    // result → same object again
    let result = alva(&home, &ws)
        .args(["jobs", "result", &job_id])
        .assert()
        .success();
    let rv: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&result.get_output().stdout).trim()).unwrap();
    assert_eq!(rv["result"], "job done");
}

#[test]
fn jobs_wasm_flags_lifecycle_and_tool_jsonl() {
    let (home, ws) = dirs();
    let (url, release, server) = start_gated_wasm_tool_server();
    write_shared_config(&home, &format!("{url}/v1"));

    let submit = alva(&home, &ws)
        .env("ALVA_WORKER_WASM", worker_wasm())
        .args([
            "jobs",
            "submit",
            "--sandbox",
            "wasm",
            "--grant",
            ws.path().to_str().unwrap(),
            "create job-output.txt",
        ])
        .assert()
        .success();
    let submitted: serde_json::Value = serde_json::from_slice(&submit.get_output().stdout).unwrap();
    let job_id = submitted["job_id"].as_str().unwrap().to_string();

    let status = alva(&home, &ws)
        .args(["jobs", "status", &job_id])
        .assert()
        .success();
    let status: serde_json::Value = serde_json::from_slice(&status.get_output().stdout).unwrap();
    assert_eq!(status["state"], "running");

    alva(&home, &ws)
        .args(["jobs", "wait", &job_id, "--timeout-secs", "0"])
        .assert()
        .code(124)
        .stderr(predicates::str::contains("timed out"));

    release.send(()).unwrap();
    let wait = alva(&home, &ws)
        .args(["jobs", "wait", &job_id])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    server.join().expect("wasm job provider completed");
    let waited: serde_json::Value = serde_json::from_slice(&wait.get_output().stdout).unwrap();
    assert_eq!(waited["result"], "job wasm done");
    assert_eq!(waited["is_error"], false);

    let status = alva(&home, &ws)
        .args(["jobs", "status", &job_id])
        .assert()
        .success();
    let status: serde_json::Value = serde_json::from_slice(&status.get_output().stdout).unwrap();
    assert_eq!(status["state"], "completed");

    let result = alva(&home, &ws)
        .args(["jobs", "result", &job_id])
        .assert()
        .success();
    let result: serde_json::Value = serde_json::from_slice(&result.get_output().stdout).unwrap();
    assert_eq!(result["result"], "job wasm done");
    assert_eq!(
        std::fs::read_to_string(ws.path().join("job-output.txt")).unwrap(),
        "from job wasm"
    );

    let log_path = home
        .path()
        .join(".alva/jobs")
        .join(&job_id)
        .join("tools.jsonl");
    let entries = std::fs::read_to_string(log_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["kind"], "tool_call");
    // The mock speaks OpenAI's wire format, where the id is `call-job-write`.
    // Alva normalizes every tool-use id to a `toolu_` prefix on the way in
    // (adapter/common.rs) so the agent loop never has to care which provider
    // it came from, and the log records the id the loop actually used.
    assert_eq!(entries[0]["tool_call_id"], "toolu_call-job-write");
    assert_eq!(entries[0]["tool_name"], "create_file");
    assert_eq!(entries[0]["is_error"], false);
    assert!(entries[0]["timestamp_ms"].as_i64().unwrap() > 0);
    assert!(!entries[0]["result_summary"].as_str().unwrap().is_empty());
}

#[test]
fn dead_worker_without_result_remains_crashed() {
    let (home, ws) = dirs();
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 23"])
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();

    let job_id = "crashed-job";
    let job_dir = home.path().join(".alva/jobs").join(job_id);
    std::fs::create_dir_all(&job_dir).unwrap();
    std::fs::write(job_dir.join("result.json"), "").unwrap();
    std::fs::write(job_dir.join("stderr.log"), "worker exploded").unwrap();
    std::fs::write(
        job_dir.join("meta.json"),
        serde_json::json!({
            "job_id": job_id,
            "pid": pid,
            "prompt": "crash",
            "workspace": ws.path(),
            "created_at_ms": 1,
        })
        .to_string(),
    )
    .unwrap();

    let status = alva(&home, &ws)
        .args(["jobs", "status", job_id])
        .assert()
        .success();
    let status: serde_json::Value = serde_json::from_slice(&status.get_output().stdout).unwrap();
    assert_eq!(status["state"], "crashed");
    alva(&home, &ws)
        .args(["jobs", "wait", job_id])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("worker exploded"));
}

/// Unknown job ids fail loudly on every inspection command.
#[test]
fn jobs_unknown_id_fails_loudly() {
    let (home, ws) = dirs();
    for sub in ["status", "wait", "result"] {
        alva(&home, &ws)
            .args(["jobs", sub, "no-such-job"])
            .assert()
            .code(1)
            .stderr(predicates::str::contains("job not found"));
    }
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
