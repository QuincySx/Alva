// INPUT:  assert_cmd (real `alva` binary), std::net (mock OpenAI SSE server), tempfile
// OUTPUT: (none — golden binary tests)
// POS:    Provider discovery + selection for orchestrators. `providers list`
//         is the machine-readable "which brains are configured" query
//         (stdout JSON, secrets never leak); `--provider <name>` picks a
//         NAMED profile per invocation, which is what lets two profiles of
//         the SAME kind (deepseek + local ollama, both openai-chat) coexist
//         in one config.json instead of colliding on the kind key.

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

/// Minimal OpenAI-compatible SSE mock that always answers `reply` — two of
/// these with distinct replies prove WHICH endpoint a run actually hit.
fn start_mock_openai_server(reply: &'static str) -> (String, std::thread::JoinHandle<()>) {
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
                    &format!(
                        r#"data: {{"id":"c1","object":"chat.completion.chunk","choices":[{{"index":0,"delta":{{"content":"{reply}"}}}}]}}"#
                    ),
                    "",
                    r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}"#,
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

/// Two same-kind named profiles + one legacy kind-keyed entry. This shape is
/// the whole point of named profiles: `deepseek` and `local` are BOTH
/// openai-chat and coexist. `anthropic` has no `kind` field — legacy schema
/// where the map key IS the kind.
fn write_named_profiles_config(home: &tempfile::TempDir, deepseek_url: &str, local_url: &str) {
    let alva_dir = home.path().join(".alva");
    std::fs::create_dir_all(&alva_dir).unwrap();
    std::fs::write(
        alva_dir.join("config.json"),
        serde_json::json!({
            "providers": {
                "deepseek": {
                    "kind": "openai-chat",
                    "api_key": "sk-deepseek-secret",
                    "model": "deepseek-chat",
                    "base_url": deepseek_url,
                },
                "local": {
                    "kind": "openai-chat",
                    "api_key": "ollama",
                    "model": "qwen3",
                    "base_url": local_url,
                },
                "anthropic": {
                    "api_key": "sk-ant-legacy-secret",
                    "model": "claude-opus-4-7",
                }
            },
            "active": "local"
        })
        .to_string(),
    )
    .unwrap();
}

/// Zero config → an orchestrator gets a well-formed empty array, not an
/// error. Discovery must never require credentials.
#[test]
fn providers_list_json_zero_config_prints_empty_array() {
    let (home, ws) = dirs();
    let assert = alva(&home, &ws)
        .args(["providers", "list", "--output-format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout is JSON");
    assert_eq!(v, serde_json::json!([]));
}

/// The discovery contract is a strict field WHITELIST: name + model +
/// active, nothing else. Endpoints and anything key-shaped (even a has_key
/// boolean) stay out of the machine channel by design — the orchestrator
/// only needs "which names exist, what model, which is on".
#[test]
fn providers_list_json_is_name_model_active_only() {
    let (home, ws) = dirs();
    write_named_profiles_config(&home, "http://127.0.0.1:1/v1", "http://127.0.0.1:2/v1");

    let assert = alva(&home, &ws)
        .args(["providers", "list", "--output-format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout is JSON");
    let arr = v.as_array().expect("top-level array");
    assert_eq!(arr.len(), 3, "all three profiles listed: {arr:?}");

    // Exactly the whitelisted keys on every row — a new field sneaking in
    // must consciously amend this test.
    for row in arr {
        let mut keys: Vec<&str> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["active", "model", "name"], "row: {row}");
    }

    let by_name = |name: &str| -> &serde_json::Value {
        arr.iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("profile `{name}` missing from {arr:?}"))
    };
    assert_eq!(by_name("deepseek")["model"], "deepseek-chat");
    assert_eq!(by_name("deepseek")["active"], false);
    assert_eq!(by_name("local")["active"], true);
    // Legacy kind-keyed entry listed under the same whitelist.
    assert_eq!(by_name("anthropic")["model"], "claude-opus-4-7");

    // The security floor: no secret, no endpoint, anywhere on stdout.
    for leaked in [
        "sk-deepseek-secret",
        "sk-ant-legacy-secret",
        "127.0.0.1:1",
        "127.0.0.1:2",
        "http",
    ] {
        assert!(
            !stdout.contains(leaked),
            "providers list must not print `{leaked}`: {stdout}"
        );
    }
}

/// `--provider <name>` routes the run to THAT profile's endpoint even when
/// `active` points elsewhere. Two live mocks with distinct replies make the
/// routing observable: if the flag were ignored, `active: local` would
/// answer "from-local" and this test fails without touching the network.
#[test]
fn print_mode_provider_flag_selects_named_profile_over_active() {
    let (home, ws) = dirs();
    let (deepseek_url, _s1) = start_mock_openai_server("from-deepseek");
    let (local_url, _s2) = start_mock_openai_server("from-local");
    write_named_profiles_config(
        &home,
        &format!("{deepseek_url}/v1"),
        &format!("{local_url}/v1"),
    );

    let assert = alva(&home, &ws)
        .args([
            "-p",
            "--provider",
            "deepseek",
            "--output-format",
            "json",
            "hello",
        ])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON object");
    assert_eq!(v["result"], "from-deepseek", "run hit the named profile");
    assert_eq!(v["is_error"], false);
}

/// A typo'd profile name must fail LOUDLY and list what IS configured —
/// silently falling back to `active` would run the wrong (possibly
/// expensive) model.
#[test]
fn print_mode_unknown_provider_fails_loudly_listing_configured() {
    let (home, ws) = dirs();
    write_named_profiles_config(&home, "http://127.0.0.1:1/v1", "http://127.0.0.1:2/v1");

    let assert = alva(&home, &ws)
        .args(["-p", "--provider", "nope", "--output-format", "json", "hi"])
        .assert()
        .code(1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("nope"),
        "error names the unknown profile: {stderr}"
    );
    for known in ["deepseek", "local", "anthropic"] {
        assert!(
            stderr.contains(known),
            "error lists configured profile `{known}`: {stderr}"
        );
    }
}

/// `settings set` with a custom profile name: without --kind there is no way
/// to know the wire protocol → loud error (NOT a silent openai fallback);
/// with --kind the profile round-trips through `providers list`.
#[test]
fn settings_set_custom_name_requires_kind_then_roundtrips() {
    let (home, ws) = dirs();

    let assert = alva(&home, &ws)
        .args(["settings", "set", "myprofile", "--api-key", "k1"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("--kind"),
        "error tells the user to pass --kind: {stderr}"
    );

    alva(&home, &ws)
        .args([
            "settings",
            "set",
            "myprofile",
            "--kind",
            "openai-chat",
            "--api-key",
            "k1",
            "--base-url",
            "http://127.0.0.1:9/v1",
        ])
        .assert()
        .success();

    // Discovery lists it (whitelist fields only)…
    let assert = alva(&home, &ws)
        .args(["providers", "list", "--output-format", "json"])
        .assert()
        .success();
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&assert.get_output().stdout).trim()).unwrap();
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "myprofile"),
        "myprofile listed: {v}"
    );
    // …while the full round-trip (kind + endpoint) is checked on the HUMAN
    // surface, `settings get`, which is allowed to show config detail.
    let assert = alva(&home, &ws)
        .args(["settings", "get", "myprofile"])
        .assert()
        .success();
    let g: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&assert.get_output().stdout).trim()).unwrap();
    assert_eq!(g["kind"], "openai-chat");
    assert_eq!(g["base_url"], "http://127.0.0.1:9/v1");

    // A bogus kind is rejected up front — it would produce a profile that
    // can never speak any wire protocol.
    alva(&home, &ws)
        .args(["settings", "set", "p2", "--kind", "not-a-protocol"])
        .assert()
        .failure();
}
