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

/// Recording variant of the mock: appends every request BODY to
/// `record_path` (separated by \n---8<---\n) so tests can assert what the
/// binary actually SENT — resumed history, system prompt, filtered tools.
/// Reads each request fully (headers + Content-Length body) so large
/// resumed-history bodies are not truncated by a single read().
fn start_recording_mock_openai_server(
    record_path: std::path::PathBuf,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let url = format!("http://{}", listener.local_addr().unwrap());

    let handle = std::thread::spawn(move || {
        for _ in 0..8 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            // Read headers fully.
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let header_end = loop {
                match stream.read(&mut tmp) {
                    Ok(0) => break None,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(pos + 4);
                        }
                    }
                    Err(_) => break None,
                }
            };
            let Some(header_end) = header_end else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let content_length: usize = headers
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse().ok())?
                })
                .unwrap_or(0);
            while buf.len() < header_end + content_length {
                match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    Err(_) => break,
                }
            }
            let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&record_path)
            {
                let _ = writeln!(f, "{body}\n---8<---");
            }

            if headers.contains("POST") && headers.contains("/chat/completions") {
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

/// Machine contract for orchestrators: `--output-format json` emits ONE json
/// object on stdout — result text, usage, session_id (resume handle),
/// duration — instead of streaming text. This is what a planning agent
/// (Claude Code skill) parses to dispatch work to alva-backed workers.
#[test]
fn print_mode_json_output_carries_result_usage_and_session() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();
    write_shared_config(&home, &format!("{url}/v1"));

    let assert = alva(&home, &ws)
        .args(["-p", "--output-format", "json", "hi"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must be a single JSON object");

    assert_eq!(v["type"], "result");
    assert_eq!(v["result"], "Hello from mock!");
    assert_eq!(v["is_error"], false);
    assert_eq!(
        v["usage"]["input_tokens"], 10,
        "usage from the mock SSE chunk"
    );
    assert_eq!(v["usage"]["output_tokens"], 3);
    assert!(
        v["session_id"].as_str().map_or(false, |s| !s.is_empty()),
        "session_id is the --resume handle, must be present: {v}"
    );
    assert!(v["duration_ms"].is_number());
}

/// --resume: the second invocation must carry the FIRST run's history to
/// the model (the whole point of a resumable worker) and keep the same
/// session id.
#[test]
fn print_mode_resume_carries_history_and_keeps_session_id() {
    let (home, ws) = dirs();
    let record = ws.path().join("requests.log");
    let (url, _server) = start_recording_mock_openai_server(record.clone());
    write_shared_config(&home, &format!("{url}/v1"));

    let first = alva(&home, &ws)
        .args(["-p", "--output-format", "json", "first question"])
        .assert()
        .success();
    let v1: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&first.get_output().stdout).trim()).unwrap();
    let sid = v1["session_id"].as_str().unwrap().to_string();

    let second = alva(&home, &ws)
        .args([
            "-p",
            "--output-format",
            "json",
            "--resume",
            &sid,
            "second question",
        ])
        .assert()
        .success();
    let v2: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&second.get_output().stdout).trim()).unwrap();
    assert_eq!(
        v2["session_id"],
        sid.as_str(),
        "resume keeps the session id"
    );

    let log = std::fs::read_to_string(&record).unwrap();
    let requests: Vec<&str> = log.split("---8<---").collect();
    assert!(requests.len() >= 2, "two model calls recorded");
    let second_req = requests[1];
    assert!(
        second_req.contains("Hello from mock!"),
        "resumed run must send the prior assistant turn as history"
    );
    assert!(
        second_req.contains("first question"),
        "resumed run must send the prior user turn as history"
    );
}

/// --resume with an unknown id fails loudly instead of silently starting a
/// fresh session (the orchestrator would lose the thread without noticing).
#[test]
fn print_mode_resume_unknown_session_fails_loudly() {
    let (home, ws) = dirs();
    write_shared_config(&home, "http://127.0.0.1:9");
    alva(&home, &ws)
        .args(["-p", "--resume", "no-such-session", "hi"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("session not found"));
}

/// --system-prompt replaces the default persona in what's actually SENT.
#[test]
fn print_mode_system_prompt_reaches_the_model() {
    let (home, ws) = dirs();
    let record = ws.path().join("requests.log");
    let (url, _server) = start_recording_mock_openai_server(record.clone());
    write_shared_config(&home, &format!("{url}/v1"));

    alva(&home, &ws)
        .args([
            "-p",
            "--system-prompt",
            "You are WORKER-42, a terse executor.",
            "hi",
        ])
        .assert()
        .success();

    let log = std::fs::read_to_string(&record).unwrap();
    assert!(
        log.contains("WORKER-42"),
        "overridden system prompt must appear in the request"
    );
    assert!(
        !log.contains("helpful coding assistant"),
        "the default persona must be REPLACED, not appended to"
    );
}

/// --allowed-tools filters what the model is offered; unknown names fail
/// loudly with a pointer to `alva tools list`.
#[test]
fn print_mode_allowed_tools_filters_the_offer_and_rejects_typos() {
    let (home, ws) = dirs();
    let record = ws.path().join("requests.log");
    let (url, _server) = start_recording_mock_openai_server(record.clone());
    write_shared_config(&home, &format!("{url}/v1"));

    alva(&home, &ws)
        .args(["-p", "--allowed-tools", "read_file", "hi"])
        .assert()
        .success();
    let log = std::fs::read_to_string(&record).unwrap();
    assert!(log.contains("read_file"), "allowed tool offered");
    assert!(
        !log.contains("execute_shell"),
        "tools outside the allowlist must not be offered to the model"
    );

    // Typo → loud failure, not a silent no-op.
    alva(&home, &ws)
        .args(["-p", "--allowed-tools", "read_fiel", "hi"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("unknown tool"));
}

/// The explicit escape hatch: on platforms with no OS sandbox, headless
/// accept-shell is allowed ONLY with --dangerously-allow-unsandboxed, and
/// a loud warning lands on stderr. (macOS treats the sandbox as enforced,
/// so this contract is exercised on Linux CI.)
#[cfg(not(target_os = "macos"))]
#[test]
fn dangerously_allow_unsandboxed_opens_the_headless_gate_with_a_warning() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();
    write_shared_config(&home, &format!("{url}/v1"));

    let assert = alva(&home, &ws)
        .args([
            "-p",
            "--permission-mode",
            "accept-shell",
            "--dangerously-allow-unsandboxed",
            "hi",
        ])
        .assert()
        .success()
        .stdout("Hello from mock!\n");
    let err = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        err.contains("WARNING") && err.contains("dangerously-allow-unsandboxed"),
        "the escape hatch must be LOUD on stderr: {err}"
    );
}

/// Without the hatch the refusal must now point at it.
#[cfg(not(target_os = "macos"))]
#[test]
fn unsandboxed_refusal_mentions_the_escape_hatch() {
    let (home, ws) = dirs();
    write_shared_config(&home, "http://127.0.0.1:9");
    alva(&home, &ws)
        .args(["-p", "--permission-mode", "accept-shell", "hi"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("--dangerously-allow-unsandboxed"));
}
