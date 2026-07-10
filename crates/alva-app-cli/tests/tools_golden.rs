// INPUT:  assert_cmd (real `alva` binary), tempfile
// OUTPUT: (none — golden binary tests)
// POS:    `alva tools list` — the discovery surface orchestrators query
//         before deciding an allowlist for a worker invocation. Key
//         contract: works with ZERO provider configuration (discovery is
//         assembly-only, no LLM traffic).

use assert_cmd::Command;

fn alva_hermetic() -> (Command, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("home tempdir");
    let ws = tempfile::tempdir().expect("workspace tempdir");
    let mut cmd = Command::cargo_bin("alva").expect("alva binary builds");
    cmd.current_dir(ws.path())
        .env("HOME", home.path())
        .env("ALVA_CONFIG_DIR", home.path().join(".alva"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env_remove("ALVA_API_KEY")
        .env_remove("ALVA_MODEL")
        .env_remove("ALVA_BASE_URL")
        .env_remove("ALVA_PROVIDER_KIND");
    (cmd, home, ws)
}

/// The whole point of the discovery command: an orchestrator (or a human)
/// can enumerate the tool surface WITHOUT any credentials configured.
#[test]
fn tools_list_json_works_with_zero_config_and_names_core_tools() {
    let (mut cmd, _home, _ws) = alva_hermetic();
    let assert = cmd
        .args(["tools", "list", "--output-format", "json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must be a JSON array");
    let arr = v.as_array().expect("top-level JSON array");
    assert!(!arr.is_empty(), "default component set registers tools");

    let names: Vec<&str> = arr.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in ["read_file", "execute_shell", "agent"] {
        assert!(
            names.contains(&expected),
            "default toggles must expose `{expected}`; got: {names:?}"
        );
    }
    assert!(
        arr.iter()
            .all(|t| t["description"].as_str().is_some_and(|d| !d.is_empty())),
        "every tool carries a non-empty description (the allowlist decision input)"
    );
}

#[test]
fn tools_list_text_prints_one_tool_per_line() {
    let (mut cmd, _home, _ws) = alva_hermetic();
    let assert = cmd.args(["tools", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.lines().any(|l| l.starts_with("read_file")),
        "text mode lists tools one per line: {stdout}"
    );
}
