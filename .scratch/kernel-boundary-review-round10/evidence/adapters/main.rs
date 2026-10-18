// INPUT: Real Pi execute API and locally generated JSONL fixture processes.
// OUTPUT: Healthy control and context-overflow recovery verdicts.
// POS: Independent round10 spec probe; no provider calls or product edits.
use alva_engine_adapter_pi::{PiAdapter, PiAdapterConfig};
use alva_engine_runtime::{collect_stream, RuntimeEvent, RuntimeRequest};
use std::os::unix::fs::PermissionsExt;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dir = tempfile::tempdir().unwrap();
    let mut lost = 0;
    for recovery in [false, true] {
        let mut lines = vec![r#"{"type":"session","id":"pi-fixture"}"#];
        if recovery {
            lines.extend([
                r#"{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"role":"assistant","stopReason":"error","errorMessage":"Context window exceeded","content":[]}}}"#,
                r#"{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"Context window exceeded","content":[]}}"#,
                r#"{"type":"agent_end","willRetry":false}"#,
                r#"{"type":"compaction_start","reason":"overflow"}"#,
                r#"{"type":"compaction_end","reason":"overflow","aborted":false,"willRetry":true,"result":{"summary":"compressed context"}}"#,
                r#"{"type":"agent_start"}"#,
            ]);
        }
        lines.extend([
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"recovered answer"}],"stopReason":"stop","usage":{"input":3,"output":5}}}"#,
            r#"{"type":"agent_end","willRetry":false}"#,
            r#"{"type":"agent_settled"}"#,
        ]);
        let path = dir.path().join(if recovery { "recovery" } else { "control" });
        let mut script = String::from("#!/bin/sh\n");
        for line in lines { script.push_str(&format!("printf '%s\\n' '{line}'\n")); }
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let adapter = PiAdapter::new(PiAdapterConfig {
            pi_path: path.to_string_lossy().into_owned(),
            session_dir: dir.path().join("sessions"),
            ..Default::default()
        });
        let events = tokio::time::timeout(std::time::Duration::from_secs(5), collect_stream(&adapter, RuntimeRequest::new("fixture").with_cwd(dir.path()))).await.unwrap().unwrap();
        let fatal = events.iter().any(|e| matches!(e, RuntimeEvent::Error { recoverable: false, .. }));
        let answer = matches!(events.last(), Some(RuntimeEvent::Completed { result: Some(text), .. }) if text == "recovered answer");
        println!("compaction_recovery={recovery}, fatal={fatal}, answer_preserved={answer}\n{events:#?}");
        if fatal || !answer { lost += 1; }
    }
    assert_eq!(lost, 0, "Pi context-overflow recovery must reach its final answer");
}
