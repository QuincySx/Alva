use alva_kernel_abi::{Phase, PhaseEffect};

#[test]
fn phase_wire_names_are_stable() {
    assert_eq!(
        serde_json::to_value(Phase::PrepareLlmRequest).unwrap(),
        serde_json::json!("prepare_llm_request")
    );
    assert_eq!(
        serde_json::from_value::<Phase>(serde_json::json!("tool_batch_declared")).unwrap(),
        Phase::ToolBatchDeclared
    );
}

#[test]
fn phase_effect_wire_names_are_stable() {
    assert_eq!(
        serde_json::to_value(PhaseEffect::Observe).unwrap(),
        serde_json::json!("observe")
    );
    assert_eq!(
        serde_json::from_value::<PhaseEffect>(serde_json::json!("decide")).unwrap(),
        PhaseEffect::Decide
    );
}
