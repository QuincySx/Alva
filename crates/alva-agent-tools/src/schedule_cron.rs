// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: ScheduleCronTool
// POS:    Creates cron-based schedules for recurring agent tasks.
//! schedule_cron — create cron schedules

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    cron: String,
    prompt: String,
    #[serde(default)]
    recurring: Option<bool>,
}

pub struct ScheduleCronTool;

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

#[async_trait]
impl Tool for ScheduleCronTool {
    fn name(&self) -> &str {
        "schedule_cron"
    }

    fn description(&self) -> &str {
        "Create a cron schedule for recurring agent tasks. Uses standard 5-field \
         cron format: minute hour day-of-month month day-of-week."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["cron", "prompt"],
            "properties": {
                "cron": {
                    "type": "string",
                    "description": "Cron expression (5 fields: min hour dom month dow). Example: '*/5 * * * *'"
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt/instructions to run on each trigger"
                },
                "recurring": {
                    "type": "boolean",
                    "description": "If true, schedule repeats (default true). If false, runs once at next match."
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

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
