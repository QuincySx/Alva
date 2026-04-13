// INPUT:  alva_types, async_trait, schemars, serde
// OUTPUT: ScheduleCronTool
// POS:    Creates cron-based schedules for recurring agent tasks.
//! schedule_cron — create cron schedules

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
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
            return Ok(ToolOutput::error(format!("Invalid cron expression: {}", msg)));
        }

        let recurring = params.recurring.unwrap_or(true);
        let schedule_id = format!(
            "sched-{}",
            &alva_types::generate_task_id(&alva_types::TaskType::LocalWorkflow)[1..9]
        );

        Ok(ToolOutput::text(format!(
            "Schedule created.\n  ID: {}\n  Cron: {}\n  Recurring: {}\n  Prompt: {}\n  \
             Note: Cron scheduling is not yet wired to the runtime scheduler.",
            schedule_id, params.cron, recurring, params.prompt
        )))
    }
}
