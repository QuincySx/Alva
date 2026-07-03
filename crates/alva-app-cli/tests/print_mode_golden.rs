// INPUT:  assert_cmd (real `alva` binary), std::net (mock OpenAI SSE server), tempfile
// OUTPUT: (none — golden binary tests)
// POS:    The -p mode's REAL contract: argv/env/config resolution → stdin/stdout/
//         stderr bytes → exit code. Only a spawned binary covers main.rs — config
//         precedence, --permission-mode validation, the non-TTY branch. Keep this
//         suite to golden cases only (each one pays a process spawn); behavioral
//         breadth lives in the in-process tests (event_handler.rs).

use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;

/// Hermetic `alva` invocation: HOME + ALVA_CONFIG_DIR redirected into a fresh
/// tempdir, every ALVA_* provider var cleared, cwd = a fresh workspace. Each
/// test opts back INTO the config source it exercises.
fn alva(home: &tempfile::TempDir, ws: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("alva").expect("alva binary builds");
    cmd.current_dir(ws.path())
        .env("HOME", home.path())
        .env("ALVA_CONFIG_DIR", home.path().join(".alva"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env_remove("ALVA_API_KEY")
        .env_remove("ALVA_MODEL")
        .env_remove("ALVA_BASE_URL")
        .env_remove("ALVA_PROVIDER_KIND")
        .env_remove("ALVA_UI_MODE")
        .env_remove("ALVA_REPL");
    cmd
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (
        tempfile::tempdir().expect("home tempdir"),
        tempfile::tempdir().expect("workspace tempdir"),
    )
}

/// Minimal OpenAI-compatible SSE mock on an OS-assigned port (std thread —
/// the spawned binary owns its own tokio runtime, the test must not).
/// Serves every `POST /chat/completions` the same scripted text stream.
fn start_mock_openai_server() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let url = format!("http://{}", listener.local_addr().unwrap());

    let handle = std::thread::spawn(move || {
        // Serve a bounded number of connections then exit — tests make at
        // most a couple of calls; the thread must not outlive the test run.
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
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello "}}]}"#,
                    "",
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"from mock!"}}]}"#,
                    "",
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}"#,
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

// ---------------------------------------------------------------------------
// Golden cases
// ---------------------------------------------------------------------------

#[test]
fn help_prints_usage_and_exits_zero() {
    let (home, ws) = dirs();
    alva(&home, &ws)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--permission-mode"));
}

#[test]
fn empty_prompt_in_print_mode_exits_one_with_hint() {
    let (home, ws) = dirs();
    write_shared_config(&home, "http://127.0.0.1:9"); // config valid; prompt missing
    alva(&home, &ws)
        .arg("-p")
        .write_stdin("")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("no prompt provided"));
}

#[test]
fn no_config_non_tty_fails_fast_instead_of_entering_the_wizard() {
    // Piped stdin carries the PROMPT, not wizard answers — entering the
    // interactive setup here would swallow it. The contract is fail-fast
    // with config instructions.
    let (home, ws) = dirs();
    let assert = alva(&home, &ws).args(["-p", "hi"]).assert().code(1);
    let out = assert.get_output();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("ALVA_API_KEY") || combined.to_lowercase().contains("config"),
        "must point the user at a config path, got: {combined}"
    );
}

#[test]
fn unknown_permission_mode_is_rejected_with_the_valid_set() {
    let (home, ws) = dirs();
    write_shared_config(&home, "http://127.0.0.1:9");
    let assert = alva(&home, &ws)
        .args(["-p", "--permission-mode", "yolo", "hi"])
        .assert()
        .code(1);
    let err = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&assert.get_output().stdout),
        err
    );
    assert!(
        all.contains("ask|accept-edits|accept-shell|plan|bypass"),
        "rejection must name the valid modes, got: {all}"
    );
}

/// The core golden path: ALVA_CONFIG_DIR-redirected config.json points at a
/// local mock provider; stdout carries EXACTLY the assistant text + newline.
/// This pins (a) the ALVA_CONFIG_DIR override end-to-end through the real
/// binary and (b) the -p stdout byte contract.
#[test]
fn print_mode_happy_path_via_alva_config_dir() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();
    write_shared_config(&home, &format!("{url}/v1"));

    alva(&home, &ws)
        .args(["-p", "hi"])
        .assert()
        .success()
        .stdout("Hello from mock!\n");
}

/// Same run configured purely through ALVA_* env vars (no config.json) —
/// pins the env-first resolution order.
#[test]
fn print_mode_happy_path_via_env_vars() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();

    alva(&home, &ws)
        .args(["-p", "hi"])
        .env("ALVA_API_KEY", "test-key")
        .env("ALVA_MODEL", "mock-model")
        .env("ALVA_BASE_URL", format!("{url}/v1"))
        .env("ALVA_PROVIDER_KIND", "openai-chat")
        .assert()
        .success()
        .stdout("Hello from mock!\n");
}

/// accept-shell/bypass assume an OS sandbox; on platforms without one the
/// binary must refuse in headless mode rather than silently run unsandboxed.
/// (On macOS the sandbox is considered enforced, so the gate does not fire —
/// Linux CI is where this contract is exercised.)
#[cfg(not(target_os = "macos"))]
#[test]
fn accept_shell_without_sandbox_refuses_in_headless_mode() {
    let (home, ws) = dirs();
    write_shared_config(&home, "http://127.0.0.1:9");
    let assert = alva(&home, &ws)
        .args(["-p", "--permission-mode", "accept-shell", "hi"])
        .assert()
        .code(1);
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&assert.get_output().stdout),
        String::from_utf8_lossy(&assert.get_output().stderr)
    );
    assert!(
        all.contains("sandbox"),
        "refusal must explain the sandbox assumption, got: {all}"
    );
}
