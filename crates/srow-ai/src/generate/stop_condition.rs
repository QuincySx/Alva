use std::sync::Arc;

use super::types::StepResult;

/// Trait for determining when to stop the generate loop.
pub trait StopCondition: Send + Sync {
    fn should_stop(&self, steps: &[StepResult]) -> bool;
}

/// Stops after a fixed number of steps.
pub struct StepCountIs(pub u32);

impl StopCondition for StepCountIs {
    fn should_stop(&self, steps: &[StepResult]) -> bool {
        steps.len() as u32 >= self.0
    }
}

/// Stops when the last step contains a tool call with the given name.
pub struct HasToolCall(pub String);

impl StopCondition for HasToolCall {
    fn should_stop(&self, steps: &[StepResult]) -> bool {
        steps
            .last()
            .map_or(false, |step| step.tool_calls.iter().any(|tc| tc.name == self.0))
    }
}

/// Convenience: create a boxed `StepCountIs` condition.
pub fn step_count_is(n: u32) -> Arc<dyn StopCondition> {
    Arc::new(StepCountIs(n))
}

/// Convenience: create a boxed `HasToolCall` condition.
pub fn has_tool_call(name: impl Into<String>) -> Arc<dyn StopCondition> {
    Arc::new(HasToolCall(name.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use srow_core::domain::tool::{ToolCall, ToolResult};
    use srow_core::ui_message_stream::{FinishReason, TokenUsage};

    fn make_step(tool_names: &[&str]) -> StepResult {
        StepResult {
            text: String::new(),
            reasoning: None,
            tool_calls: tool_names
                .iter()
                .map(|name| ToolCall {
                    id: "tc_1".to_string(),
                    name: name.to_string(),
                    input: serde_json::Value::Null,
                })
                .collect(),
            tool_results: vec![],
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
        }
    }

    #[test]
    fn step_count_is_returns_false_for_fewer_steps() {
        let cond = step_count_is(3);
        let steps = vec![make_step(&[]), make_step(&[])];
        assert!(!cond.should_stop(&steps));
    }

    #[test]
    fn step_count_is_returns_true_when_reached() {
        let cond = step_count_is(3);
        let steps = vec![make_step(&[]), make_step(&[]), make_step(&[])];
        assert!(cond.should_stop(&steps));
    }

    #[test]
    fn has_tool_call_returns_true_when_last_step_has_matching_tool() {
        let cond = has_tool_call("search");
        let steps = vec![make_step(&["search"])];
        assert!(cond.should_stop(&steps));
    }

    #[test]
    fn has_tool_call_returns_false_when_no_matching_tool() {
        let cond = has_tool_call("search");
        let steps = vec![make_step(&["read_file"])];
        assert!(!cond.should_stop(&steps));
    }

    #[test]
    fn has_tool_call_returns_false_for_empty_steps() {
        let cond = has_tool_call("search");
        let steps: Vec<StepResult> = vec![];
        assert!(!cond.should_stop(&steps));
    }
}
