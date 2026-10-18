// INPUT: real adapter execute APIs, local successful fake CLI scripts
// OUTPUT: observed terminal events and Normal harness false-positive result
// POS: standalone review reproduction, no product modification
use alva_engine_runtime::{collect_stream, assert_conforms_events, EngineRuntime, RuntimeEvent, RuntimeRequest, Scenario};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_adapter_pi::{PiAdapter, PiAdapterConfig};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let codex = CodexAdapter::new(CodexAdapterConfig {
        codex_path: base.join("fake-codex.sh").to_string_lossy().into_owned(),
        default_cwd: Some(base.clone()),
        ..Default::default()
    });
    let pi = PiAdapter::new(PiAdapterConfig {
        pi_path: base.join("fake-pi.sh").to_string_lossy().into_owned(),
        ..Default::default()
    });
    let mut regressions = 0;
    for (name, engine) in [("codex", &codex as &dyn EngineRuntime), ("pi", &pi as &dyn EngineRuntime)] {
        let events = tokio::time::timeout(std::time::Duration::from_secs(5), collect_stream(engine, RuntimeRequest::new("local fixture"))).await.unwrap().unwrap();
        let fatal = events.iter().any(|e| matches!(e, RuntimeEvent::Error { recoverable: false, .. }));
        let answer = matches!(events.last(), Some(RuntimeEvent::Completed { result: Some(text), .. }) if text == "expected successful answer");
        let harness_accepts = std::panic::catch_unwind(|| assert_conforms_events(&events, Scenario::Normal)).is_ok();
        println!("{name}: fatal={fatal}, answer_preserved={answer}, normal_harness_accepts={harness_accepts}\n{events:#?}");
        if fatal || !answer { regressions += 1; }
    }
    assert_eq!(regressions, 0, "successful adapter runs must preserve success");
}
