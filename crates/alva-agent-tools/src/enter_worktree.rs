// INPUT:  alva_types, async_trait, schemars, serde, crate::local_fs::LocalToolFs
// OUTPUT: EnterWorktreeTool
// POS:    Creates an isolated git worktree for safe parallel development.
//! enter_worktree — create an isolated git worktree

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Name for the worktree (used for branch and directory). Auto-generated if omitted.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "enter_worktree",
    description = "Create an isolated git worktree for safe parallel development. \
        The worktree gets its own branch and working directory, allowing \
        changes without affecting the main workspace.",
    input = Input,
)]
pub struct EnterWorktreeTool;

impl EnterWorktreeTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "enter_worktree".into(),
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
                tool_name: "enter_worktree".into(),
                message: format!("Failed to run git worktree: {}", e),
            }),
        }
    }
}
