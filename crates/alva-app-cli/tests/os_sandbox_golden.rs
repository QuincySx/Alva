// INPUT:  real alva binary, local OpenAI-compatible SSE server, cargo/rustc, /private/tmp test workspace
// OUTPUT: macOS/Linux OS-tier end-to-end coverage for file mutation plus sandbox-inherited cargo execution
// POS:    OS-host golden proving `--sandbox os-write` runs the complete native worker under kernel confinement.
#![cfg(any(target_os = "macos", target_os = "linux"))]

use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;

fn start_tool_server() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for turn in 0..3 {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            drain_request(&mut stream);
            let frames = match turn {
                0 => vec![
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-write","type":"function","function":{"name":"create_file","arguments":"{\"path\":\"src/lib.rs\",\"content\":\"pub fn answer() -> u32 { 42 }\\n\\n#[cfg(test)]\\nmod tests {\\n    #[test]\\n    fn answer_is_42() { assert_eq!(super::answer(), 42); }\\n}\\n\"}"}}]}}]}"#,
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                ],
                1 => vec![
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-test","type":"function","function":{"name":"execute_shell","arguments":"{\"command\":\"cargo test --offline\",\"cwd\":\".\",\"timeout\":120000}"}}]}}]}"#,
                    r#"data: {"id":"c2","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                ],
                _ => vec![
                    r#"data: {"id":"c3","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                    r#"data: {"id":"c3","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"OS sandbox edit and cargo test completed"},"finish_reason":"stop"}]}"#,
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
    (url, server)
}

fn drain_request(stream: &mut std::net::TcpStream) {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read request");
        assert!(read > 0, "request ended before headers");
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
        let read = stream.read(&mut chunk).expect("read request body");
        assert!(read > 0, "request body ended early");
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// This test cannot run inside Codex's managed macOS sandbox because nested
/// `sandbox_apply` is rejected before the worker starts. It is intentionally
/// not ignored: unrestricted macOS CI/maintainers must exercise it.
#[test]
fn os_tier_worker_edits_file_and_runs_cargo_test() {
    #[cfg(target_os = "macos")]
    let workspace = tempfile::Builder::new()
        .prefix("alva-os-e2e-")
        .tempdir_in("/private/tmp")
        .unwrap();
    #[cfg(target_os = "linux")]
    let workspace = tempfile::Builder::new()
        .prefix("alva-os-e2e-")
        .tempdir()
        .unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"os-sandbox-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let home = tempfile::tempdir().unwrap();
    let (url, server) = start_tool_server();

    let assert = Command::cargo_bin("alva")
        .unwrap()
        .current_dir(workspace.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("ALVA_API_KEY", "os-tier-test-key")
        .env("ALVA_MODEL", "mock-model")
        .env("ALVA_PROVIDER_KIND", "openai-chat")
        .env("ALVA_BASE_URL", format!("{url}/v1"))
        .args([
            "-p",
            "--sandbox",
            "os-write",
            "--grant",
            workspace.path().to_str().unwrap(),
            "--permission-mode",
            "bypass",
            "add the implementation and run its tests",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "OS sandbox edit and cargo test completed",
        ));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    #[cfg(target_os = "macos")]
    assert!(stderr.contains("does NOT restrict file reads"), "{stderr}");
    #[cfg(target_os = "linux")]
    assert!(
        stderr.contains("Landlock to confine reads and writes"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("--dangerously-allow-unsandboxed"),
        "an enforced OS worker must pass the elevated permission gate: {stderr}"
    );
    server.join().expect("provider completed");

    let source = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(source.contains("pub fn answer() -> u32 { 42 }"));
    assert!(workspace.path().join("target/debug").is_dir());
    drop(assert);
}
