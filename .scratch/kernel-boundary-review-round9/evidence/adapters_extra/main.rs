// INPUT: Real Codex/Pi execute APIs and bounded local JSONL fixture processes.
// OUTPUT: Rejects contradictory fatal Error plus successful Completed.result.
// POS: Round 9 independent adapter boundary probe, no production modifications.
use alva_engine_runtime::{collect_stream, EngineRuntime, RuntimeEvent, RuntimeRequest};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_adapter_pi::{PiAdapter, PiAdapterConfig};
use std::os::unix::fs::PermissionsExt;

fn fixture(dir: &std::path::Path, name: &str, lines: &[&str], bad_utf8: bool) -> String {
    let path = dir.join(name);
    let mut source = String::from("#!/bin/sh\n");
    for line in lines {
        source.push_str(&format!("printf '%s\\n' '{}'\n", line.replace('\'', "'\\''")));
    }
    if bad_utf8 { source.push_str("printf '\\377\\n'\n"); }
    std::fs::write(&path, source).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path.to_string_lossy().into_owned()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dir = tempfile::tempdir().unwrap();
    let codex_lines = [
        r#"{"type":"thread.started","thread_id":"codex-fixture"}"#,
        r#"{"type":"item.completed","item":{"id":"answer","type":"agent_message","text":"expected successful answer"}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":5}}"#,
    ];
    let pi_lines = [
        r#"{"type":"session","id":"pi-fixture"}"#,
        r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"expected successful answer"}],"stopReason":"stop","usage":{"input":3,"output":5}}}"#,
        r#"{"type":"agent_end","willRetry":false}"#,
    ];
    let mut contradictions = 0;
    for bad_utf8 in [false, true] {
        let codex = CodexAdapter::new(CodexAdapterConfig {
            codex_path: fixture(dir.path(), "fake-codex", &codex_lines, bad_utf8),
            default_cwd: Some(dir.path().into()),
            ..Default::default()
        });
        let pi = PiAdapter::new(PiAdapterConfig {
            pi_path: fixture(dir.path(), "fake-pi", &pi_lines, bad_utf8),
            session_dir: dir.path().join("pi-sessions"),
            ..Default::default()
        });
        for (name, engine) in [("codex", &codex as &dyn EngineRuntime), ("pi", &pi as &dyn EngineRuntime)] {
            let mut request = RuntimeRequest::new("local fixture");
            request.working_directory = Some(dir.path().into());
            let events = tokio::time::timeout(std::time::Duration::from_secs(5), collect_stream(engine, request)).await.unwrap().unwrap();
            let fatal = events.iter().any(|e| matches!(e, RuntimeEvent::Error { recoverable: false, .. }));
            let has_result = matches!(events.last(), Some(RuntimeEvent::Completed { result: Some(_), .. }));
            println!("{name}: invalid_utf8={bad_utf8}, fatal={fatal}, has_result={has_result}\n{events:#?}");
            if !bad_utf8 { assert!(!fatal && has_result, "healthy control must succeed"); }
            if fatal && has_result { contradictions += 1; }
        }
    }
    let retry_lines = [
        r#"{"type":"session","id":"pi-retry-fixture"}"#,
        r#"{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"role":"assistant","stopReason":"error","errorMessage":"503 Service Unavailable","content":[]}}}"#,
        r#"{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"503 Service Unavailable","content":[]}}"#,
        r#"{"type":"agent_end","willRetry":true}"#,
        r#"{"type":"auto_retry_start","attempt":1,"maxAttempts":3,"errorMessage":"503 Service Unavailable"}"#,
        r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"recovered answer"}],"stopReason":"stop","usage":{"input":3,"output":5}}}"#,
        r#"{"type":"auto_retry_end","success":true}"#,
        r#"{"type":"agent_end","willRetry":false}"#,
    ];
    let pi_retry = PiAdapter::new(PiAdapterConfig {
        pi_path: fixture(dir.path(), "fake-pi-retry", &retry_lines, false),
        session_dir: dir.path().join("pi-retry-sessions"),
        ..Default::default()
    });
    let events = tokio::time::timeout(std::time::Duration::from_secs(5), collect_stream(&pi_retry, RuntimeRequest::new("retry fixture").with_cwd(dir.path()))).await.unwrap().unwrap();
    let retry_failed = events.iter().any(|e| matches!(e, RuntimeEvent::Error { recoverable: false, .. }))
        || !matches!(events.last(), Some(RuntimeEvent::Completed { result: Some(text), .. }) if text == "recovered answer");
    println!("pi: recoverable_retry_lost={retry_failed}\n{events:#?}");
    // Either establish a committed-success boundary and ignore trailing bytes,
    // or report the I/O failure and clear the result. Both are coherent; the
    // current adapter's fatal+success pair is not. Do not prescribe one policy.
    assert_eq!((contradictions, retry_failed), (0, false), "fatal streams must not report success; retry must survive provisional assistant errors");
}
