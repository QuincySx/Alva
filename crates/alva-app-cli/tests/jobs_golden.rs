// INPUT:  assert_cmd (real `alva` binary), std::net (mock OpenAI SSE server), tempfile
// OUTPUT: (none — golden binary tests)
// POS:    Async job mode for long-running worker tasks: `jobs submit` returns
//         a job id IMMEDIATELY; `jobs wait` BLOCKS until done (the caller
//         never writes a polling loop); `jobs status/result` inspect. State
//         lives under $ALVA_CONFIG_DIR/jobs/<id>/ — no daemon.

use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;

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
