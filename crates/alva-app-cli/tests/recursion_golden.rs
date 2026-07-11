// INPUT:  assert_cmd (real `alva` binary), std::net (mock OpenAI SSE server), tempfile
// OUTPUT: (none — golden binary tests)
// POS:    The cross-process recursion gate. Workers with shell access can run
//         `alva -p ...` themselves (agent-spawns-agent across processes); the
//         in-process subagent depth limit cannot see that. The gate: every
//         agent run reads ALVA_AGENT_DEPTH, refuses at the configured
//         `subagent_depth` limit (ONE config knob governs both recursion
//         forms), and exports depth+1 so any shelled-out alva inherits it.
//         Plus `--max-turns`: the orchestrator's per-worker turn budget.

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
        .env_remove("ALVA_PROVIDER_KIND")
        .env_remove("ALVA_AGENT_DEPTH");
    cmd
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (
        tempfile::tempdir().expect("home"),
        tempfile::tempdir().expect("ws"),
    )
}

/// Same minimal OpenAI-compatible SSE mock as the sibling golden files.
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
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"depth ok"}}]}"#,
                    "",
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#,
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

fn write_config(home: &tempfile::TempDir, base_url: &str, subagent_depth: Option<u32>) {
    let alva_dir = home.path().join(".alva");
    std::fs::create_dir_all(&alva_dir).unwrap();
    let mut cfg = serde_json::json!({
        "providers": {
            "openai-chat": { "api_key": "test-key", "model": "mock", "base_url": base_url }
        },
        "active": "openai-chat"
    });
    if let Some(d) = subagent_depth {
        cfg["subagent_depth"] = serde_json::json!(d);
    }
    std::fs::write(alva_dir.join("config.json"), cfg.to_string()).unwrap();
}

/// At the default limit (3), depth 3 refuses — BEFORE provider/key
/// resolution: no config, no key, totally hermetic, and the refusal still
/// explains itself (env var, limit, and the config knob that raises it).
#[test]
fn depth_at_default_limit_refuses_before_any_config() {
    let (home, ws) = dirs();
    let assert = alva(&home, &ws)
        .env("ALVA_AGENT_DEPTH", "3")
        .args(["-p", "hi"])
        .assert()
        .code(1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("ALVA_AGENT_DEPTH"),
        "refusal names the env var: {stderr}"
    );
    assert!(
        stderr.contains("subagent_depth"),
        "refusal names the config knob that raises the limit: {stderr}"
    );
}

/// The ONE knob: `subagent_depth` in config.json also governs the
/// cross-process gate. Depth 3 is refused by default but runs fine when the
/// user configured 5 layers.
#[test]
fn config_subagent_depth_raises_the_cross_process_limit() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();
    write_config(&home, &format!("{url}/v1"), Some(5));

    let assert = alva(&home, &ws)
        .env("ALVA_AGENT_DEPTH", "3")
        .args(["-p", "--output-format", "json", "hi"])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&assert.get_output().stdout).trim())
            .expect("one JSON object");
    assert_eq!(v["result"], "depth ok");
}

/// A garbage depth value means something upstream is broken — fail loudly
/// rather than silently treating it as depth 0 (which would disarm the gate).
#[test]
fn garbage_depth_env_fails_loudly() {
    let (home, ws) = dirs();
    let assert = alva(&home, &ws)
        .env("ALVA_AGENT_DEPTH", "banana")
        .args(["-p", "hi"])
        .assert()
        .code(1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("ALVA_AGENT_DEPTH"),
        "error names the env var: {stderr}"
    );
}

/// Subcommands that never run an agent (discovery/config) stay usable at any
/// depth — a worker inspecting the tool surface is not recursion.
#[test]
fn non_agent_subcommands_ignore_depth() {
    let (home, ws) = dirs();
    alva(&home, &ws)
        .env("ALVA_AGENT_DEPTH", "99")
        .args(["providers", "list", "--output-format", "json"])
        .assert()
        .success();
}

/// `--max-turns`: the wiring golden — a valid budget runs; a garbage value
/// fails loudly instead of silently keeping the default.
#[test]
fn max_turns_valid_runs_and_garbage_fails_loudly() {
    let (home, ws) = dirs();
    let (url, _server) = start_mock_openai_server();
    write_config(&home, &format!("{url}/v1"), None);

    let assert = alva(&home, &ws)
        .args(["-p", "--max-turns", "2", "--output-format", "json", "hi"])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&assert.get_output().stdout).trim())
            .expect("one JSON object");
    assert_eq!(v["result"], "depth ok");

    for bad in [
        &["-p", "--max-turns", "banana", "hi"][..],
        &["-p", "--max-turns", "0", "hi"][..],
    ] {
        let assert = alva(&home, &ws).args(bad).assert().code(1);
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        assert!(
            stderr.contains("--max-turns"),
            "error names the flag: {stderr}"
        );
    }
}
