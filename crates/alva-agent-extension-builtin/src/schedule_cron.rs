// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: ScheduleCronTool
// POS:    Creates cron-based schedules for recurring agent tasks.
//! schedule_cron — create cron schedules

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Cron expression (5 fields: min hour dom month dow). Example: '*/5 * * * *'.
    cron: String,
    /// Prompt / instructions to run on each trigger.
    prompt: String,
    /// If true, schedule repeats (default true). If false, runs once at next match.
    #[serde(default)]
    recurring: Option<bool>,
}

/// Basic validation for a 5-field cron expression.
fn validate_cron(expr: &str) -> Result<(), String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "Expected 5 fields (minute hour day month weekday), got {}",
            fields.len()
        ));
    }
    Ok(())
}

#[derive(Tool)]
#[tool(
    name = "schedule_cron",
    description = "Create a cron schedule for recurring agent tasks. Uses standard 5-field \
        cron format: minute hour day-of-month month day-of-week.",
    input = Input,
)]
pub struct ScheduleCronTool;

impl ScheduleCronTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // Validate cron expression
        if let Err(msg) = validate_cron(&params.cron) {
            return Ok(ToolOutput::error(format!(
                "Invalid cron expression: {}",
                msg
            )));
        }

        let recurring = params.recurring.unwrap_or(true);
        let schedule_id = format!(
            "sched-{}",
            &alva_kernel_abi::generate_task_id(&alva_kernel_abi::TaskType::LocalWorkflow)[1..9]
        );

        Ok(ToolOutput::text(format!(
            "Schedule created.\n  ID: {}\n  Cron: {}\n  Recurring: {}\n  Prompt: {}\n  \
             Note: Cron scheduling is not yet wired to the runtime scheduler.",
            schedule_id, params.cron, recurring, params.prompt
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use alva_kernel_abi::{CancellationToken, Tool};
    use serde_json::json;

    struct TestContext {
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx() -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
        }
    }

    #[test]
    fn validate_cron_accepts_five_fields_rejects_others() {
        assert!(validate_cron("*/5 * * * *").is_ok());
        assert!(validate_cron("0 0 1 1 0").is_ok());
        // Wrong field counts — all rejected. Note the validator only counts
        // whitespace-separated fields; it doesn't verify field syntax.
        assert!(validate_cron("").is_err());
        assert!(validate_cron("* * * *").is_err(), "4 fields should error");
        assert!(
            validate_cron("* * * * * *").is_err(),
            "6 fields should error"
        );
        // Extra whitespace is forgiven by split_whitespace
        assert!(validate_cron("  *   *  *  *  *  ").is_ok());
    }

    #[tokio::test]
    async fn happy_path_echoes_schedule_fields() {
        let tool = ScheduleCronTool;
        let out = tool
            .execute(
                json!({
                    "cron": "*/5 * * * *",
                    "prompt": "check the deploy",
                    "recurring": true,
                }),
                &ctx(),
            )
            .await
            .expect("execute should succeed");

        assert!(!out.is_error);
        let text = out.model_text();
        assert!(text.contains("*/5 * * * *"), "cron missing: {text}");
        assert!(text.contains("check the deploy"), "prompt missing: {text}");
        assert!(
            text.contains("Recurring: true"),
            "recurring missing: {text}"
        );
        assert!(text.contains("sched-"), "schedule id missing: {text}");
    }

    #[tokio::test]
    async fn recurring_defaults_to_true_when_omitted() {
        let tool = ScheduleCronTool;
        let out = tool
            .execute(
                json!({ "cron": "0 9 * * 1-5", "prompt": "daily standup" }),
                &ctx(),
            )
            .await
            .expect("execute should succeed");

        let text = out.model_text();
        assert!(
            text.contains("Recurring: true"),
            "expected recurring=true default: {text}"
        );
    }

    #[tokio::test]
    async fn invalid_cron_surfaces_as_tool_output_error() {
        let tool = ScheduleCronTool;
        // 4 fields → validate_cron rejects → ToolOutput::error (not AgentError)
        let out = tool
            .execute(json!({ "cron": "* * * *", "prompt": "x" }), &ctx())
            .await
            .expect("invalid cron should still return Ok(ToolOutput)");

        assert!(out.is_error, "wrong field count should set is_error");
        let text = out.model_text();
        assert!(
            text.contains("Invalid cron expression"),
            "expected validation error message: {text}"
        );
    }

    /// Stub-output contract guard: when the runtime scheduler lands and
    /// this tool actually creates a schedule, this string changes and the
    /// test gets updated in lockstep.
    #[tokio::test]
    async fn stub_text_advertises_unwired_scheduler() {
        let tool = ScheduleCronTool;
        let out = tool
            .execute(json!({ "cron": "0 0 * * *", "prompt": "x" }), &ctx())
            .await
            .expect("execute should succeed");
        assert!(
            out.model_text().contains("not yet wired"),
            "stub disclosure missing — if you wired the scheduler, update this test"
        );
    }
}
