// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: EnterWorktreeTool
// POS:    Creates an isolated git worktree for safe parallel development.
//! enter_worktree — create an isolated git worktree

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    #[serde(default)]
    name: Option<String>,
}

pub struct EnterWorktreeTool;

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "enter_worktree"
    }

    fn description(&self) -> &str {
        "Create an isolated git worktree for safe parallel development. \
         The worktree gets its own branch and working directory, allowing \
         changes without affecting the main workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name for the worktree (used for branch and directory). Auto-generated if omitted."
                }
            }
        })
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

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: self.name().into(),
            message: "workspace context required".into(),
        })?;

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        // Generate worktree name
        let wt_name = params.name.unwrap_or_else(|| {
            format!("wt-{}", &alva_types::generate_task_id(&alva_types::TaskType::LocalAgent)[1..9])
        });
        let branch_name = format!("worktree/{}", wt_name);
        let wt_path = workspace.parent().unwrap_or(workspace).join(&wt_name);

        // Create the worktree via git
        let cmd = format!(
            "git worktree add -b {} {} HEAD",
            branch_name,
            wt_path.display()
        );
        let cwd = workspace.to_str();

        match fs.exec(&cmd, cwd, 30_000).await {
            Ok(result) if result.success() => {
                Ok(ToolOutput::text(format!(
                    "Worktree created.\n  Path: {}\n  Branch: {}\n  \
                     Use exit_worktree to clean up when done.",
                    wt_path.display(),
                    branch_name
                )))
            }
            Ok(result) => {
                Ok(ToolOutput::error(format!(
                    "Failed to create worktree:\n{}{}",
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
}
