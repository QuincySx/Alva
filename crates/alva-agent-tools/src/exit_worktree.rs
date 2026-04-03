// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ExitWorktreeTool
// POS:    Exits and optionally cleans up a git worktree.
//! exit_worktree — exit and cleanup a git worktree

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    discard_changes: Option<bool>,
}

pub struct ExitWorktreeTool;

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "exit_worktree"
    }

    fn description(&self) -> &str {
        "Exit and optionally remove the current git worktree. \
         Use 'keep' to preserve the worktree or 'remove' to delete it."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["keep", "remove"],
                    "description": "Whether to keep or remove the worktree"
                },
                "discard_changes": {
                    "type": "boolean",
                    "description": "If true and action is 'remove', discard uncommitted changes. Default false."
                }
            }
        })
    }

    fn is_destructive(&self, input: &Value) -> bool {
        input.get("action").and_then(|v| v.as_str()) == Some("remove")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        match params.action.as_str() {
            "keep" => {
                Ok(ToolOutput::text(
                    "Worktree kept. You can return to it later or remove it with exit_worktree action='remove'."
                ))
            }
            "remove" => {
                let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "workspace context required".into(),
                })?;

                let fallback = LocalToolFs::new(workspace);
                let fs = ctx.tool_fs().unwrap_or(&fallback);

                let force_flag = if params.discard_changes.unwrap_or(false) {
                    " --force"
                } else {
                    ""
                };

                let cmd = format!(
                    "git worktree remove {}{}",
                    workspace.display(),
                    force_flag
                );

                // Run from parent directory since we're removing the current worktree
                let parent = workspace.parent().map(|p| p.to_str().unwrap_or(""));

                match fs.exec(&cmd, parent, 30_000).await {
                    Ok(result) if result.success() => {
                        Ok(ToolOutput::text("Worktree removed successfully."))
                    }
                    Ok(result) => {
                        Ok(ToolOutput::error(format!(
                            "Failed to remove worktree:\n{}{}",
                            result.stdout,
                            result.stderr
                        )))
                    }
                    Err(e) => Err(AgentError::ToolError {
                        tool_name: self.name().into(),
                        message: format!("Failed to run git worktree: {}", e),
                    }),
                }
            }
            other => {
                Ok(ToolOutput::error(format!(
                    "Invalid action '{}'. Must be 'keep' or 'remove'.",
                    other
                )))
            }
        }
    }
}
