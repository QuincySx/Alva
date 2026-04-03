use super::types::*;

// === Session Commands ===

pub struct ClearCommand;
impl Command for ClearCommand {
    fn name(&self) -> &str {
        "clear"
    }
    fn description(&self) -> &str {
        "Clear conversation history"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("Conversation cleared.".to_string())
    }
}

pub struct CompactCommand;
impl Command for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }
    fn description(&self) -> &str {
        "Compact conversation to save context"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Compact {
            summary: "Conversation compacted.".to_string(),
        }
    }
}

pub struct NewCommand;
impl Command for NewCommand {
    fn name(&self) -> &str {
        "new"
    }
    fn description(&self) -> &str {
        "Start a new conversation"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("Starting new conversation...".to_string())
    }
}

// === Navigation Commands ===

pub struct HelpCommand;
impl Command for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }
    fn aliases(&self) -> Vec<&str> {
        vec!["h", "?"]
    }
    fn description(&self) -> &str {
        "Show available commands"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        let help_text = r#"Available commands:
  /help, /h, /?     Show this help message
  /clear             Clear conversation history
  /compact           Compact conversation to save context
  /new               Start a new conversation
  /resume            Resume a previous conversation
  /sessions          List all sessions
  /model <name>      Switch model
  /config            Show/edit configuration
  /cost              Show token usage and cost
  /status            Show system status
  /doctor            Run diagnostics
  /commit            Create a git commit (AI-assisted)
  /review            Review code changes (AI-assisted)
  /export            Export conversation
  /copy              Copy last response
  /summary           Summarize conversation
  /plan              Toggle plan mode (read-only)
  /fast              Toggle fast mode
  /vim               Toggle vim mode
  /tools             List available tools
  /mcp               Manage MCP servers
  /agents            List running agents
  /tasks             List tasks
  /permissions       Manage permissions
  /theme             Change theme
  /exit, /quit       Exit the application

  !<command>         Run shell command directly
  @<file>            Attach file to prompt"#;
        CommandResult::Text(help_text.to_string())
    }
}

pub struct ExitCommand;
impl Command for ExitCommand {
    fn name(&self) -> &str {
        "exit"
    }
    fn aliases(&self) -> Vec<&str> {
        vec!["quit", "q"]
    }
    fn description(&self) -> &str {
        "Exit the application"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("EXIT".to_string()) // REPL should handle this signal
    }
}

// === Info Commands ===

pub struct CostCommand;
impl Command for CostCommand {
    fn name(&self) -> &str {
        "cost"
    }
    fn aliases(&self) -> Vec<&str> {
        vec!["usage"]
    }
    fn description(&self) -> &str {
        "Show token usage and estimated cost"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        // Placeholder - will be wired to actual tracking
        CommandResult::Text("Token usage tracking not yet connected.".to_string())
    }
}

pub struct StatusCommand;
impl Command for StatusCommand {
    fn name(&self) -> &str {
        "status"
    }
    fn description(&self) -> &str {
        "Show system status"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandResult {
        let status = format!(
            "Model: {}\nWorkspace: {}\nSession: {}",
            ctx.model,
            ctx.workspace.display(),
            ctx.session_id,
        );
        CommandResult::Text(status)
    }
}

pub struct DoctorCommand;
impl Command for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }
    fn description(&self) -> &str {
        "Run diagnostic checks"
    }
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandResult {
        let mut checks = Vec::new();

        // Check workspace
        checks.push(format!("  Workspace: {}", ctx.workspace.display()));

        // Check git
        let git_check = std::process::Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(ctx.workspace)
            .output();
        match git_check {
            Ok(output) if output.status.success() => {
                checks.push("  Git repository detected".to_string())
            }
            _ => checks.push("  Not a git repository".to_string()),
        }

        // Check model
        checks.push(format!("  Model: {}", ctx.model));

        CommandResult::Text(checks.join("\n"))
    }
}

// === Config Commands ===

pub struct ConfigCommand;
impl Command for ConfigCommand {
    fn name(&self) -> &str {
        "config"
    }
    fn description(&self) -> &str {
        "Show or edit configuration"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            // Show current config
            let config_path = ctx.workspace.join(".claude").join("settings.json");
            match std::fs::read_to_string(&config_path) {
                Ok(content) => CommandResult::Text(format!(
                    "Settings ({}):\n{}",
                    config_path.display(),
                    content
                )),
                Err(_) => CommandResult::Text(
                    "No project settings found. Create .claude/settings.json to configure."
                        .to_string(),
                ),
            }
        } else {
            CommandResult::Text(format!("Config: {}", args))
        }
    }
}

pub struct ModelCommand;
impl Command for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }
    fn description(&self) -> &str {
        "Switch or show current model"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            CommandResult::Text(format!("Current model: {}", ctx.model))
        } else {
            CommandResult::Text(format!("MODEL_SWITCH:{}", args.trim()))
        }
    }
}

pub struct ThemeCommand;
impl Command for ThemeCommand {
    fn name(&self) -> &str {
        "theme"
    }
    fn description(&self) -> &str {
        "Change terminal theme"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            CommandResult::Text(
                "Available themes: dark (default), light, monokai, solarized".to_string(),
            )
        } else {
            CommandResult::Text(format!("Theme set to: {}", args.trim()))
        }
    }
}

pub struct PermissionsCommand;
impl Command for PermissionsCommand {
    fn name(&self) -> &str {
        "permissions"
    }
    fn description(&self) -> &str {
        "Show or manage permission rules"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("Permission rules: (use settings.json to configure)".to_string())
    }
}

// === Mode Commands ===

pub struct PlanCommand;
impl Command for PlanCommand {
    fn name(&self) -> &str {
        "plan"
    }
    fn description(&self) -> &str {
        "Toggle plan mode (read-only)"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("PLAN_MODE_TOGGLE".to_string())
    }
}

pub struct FastCommand;
impl Command for FastCommand {
    fn name(&self) -> &str {
        "fast"
    }
    fn description(&self) -> &str {
        "Toggle fast mode"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("FAST_MODE_TOGGLE".to_string())
    }
}

pub struct VimCommand;
impl Command for VimCommand {
    fn name(&self) -> &str {
        "vim"
    }
    fn description(&self) -> &str {
        "Toggle vim keybindings"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("VIM_MODE_TOGGLE".to_string())
    }
}

// === Git Commands (Prompt type) ===

pub struct CommitCommand;
impl Command for CommitCommand {
    fn name(&self) -> &str {
        "commit"
    }
    fn description(&self) -> &str {
        "Create a git commit (AI-assisted)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandResult {
        let prompt = if args.is_empty() {
            r#"Create a git commit for the current changes. Follow these steps:
1. Run `git status` and `git diff --staged` to see changes
2. If nothing is staged, suggest which files to stage
3. Write a concise commit message following conventional commits
4. Create the commit

Only commit what makes sense as a single logical change."#
                .to_string()
        } else {
            format!("Create a git commit with this guidance: {}", args)
        };

        CommandResult::Prompt {
            content: prompt,
            progress_message: Some("Creating commit...".to_string()),
            allowed_tools: Some(vec![
                "Bash(git add:*)".to_string(),
                "Bash(git status:*)".to_string(),
                "Bash(git diff:*)".to_string(),
                "Bash(git commit:*)".to_string(),
                "Bash(git log:*)".to_string(),
            ]),
        }
    }
}

pub struct ReviewCommand;
impl Command for ReviewCommand {
    fn name(&self) -> &str {
        "review"
    }
    fn aliases(&self) -> Vec<&str> {
        vec!["review-pr"]
    }
    fn description(&self) -> &str {
        "Review code changes (AI-assisted)"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandResult {
        let prompt = if args.is_empty() {
            "Review the current code changes. Run `git diff` to see unstaged changes and \
             `git diff --staged` for staged changes. Provide feedback on code quality, \
             potential bugs, and improvements."
                .to_string()
        } else {
            format!("Review PR or changes: {}", args)
        };

        CommandResult::Prompt {
            content: prompt,
            progress_message: Some("Reviewing code...".to_string()),
            allowed_tools: None,
        }
    }
}

// === Export Commands ===

pub struct ExportCommand;
impl Command for ExportCommand {
    fn name(&self) -> &str {
        "export"
    }
    fn description(&self) -> &str {
        "Export conversation to file"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandResult {
        let format = if args.is_empty() {
            "markdown"
        } else {
            args.trim()
        };
        CommandResult::Text(format!("EXPORT:{}", format))
    }
}

pub struct CopyCommand;
impl Command for CopyCommand {
    fn name(&self) -> &str {
        "copy"
    }
    fn description(&self) -> &str {
        "Copy last response to clipboard"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("COPY_LAST".to_string())
    }
}

pub struct SummaryCommand;
impl Command for SummaryCommand {
    fn name(&self) -> &str {
        "summary"
    }
    fn description(&self) -> &str {
        "Summarize the conversation"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Prompt {
            content: "Provide a brief summary of our conversation so far.".to_string(),
            progress_message: Some("Summarizing...".to_string()),
            allowed_tools: None,
        }
    }
}

// === Tool Commands ===

pub struct ToolsCommand;
impl Command for ToolsCommand {
    fn name(&self) -> &str {
        "tools"
    }
    fn description(&self) -> &str {
        "List available tools"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("TOOLS_LIST".to_string())
    }
}

pub struct McpCommand;
impl Command for McpCommand {
    fn name(&self) -> &str {
        "mcp"
    }
    fn description(&self) -> &str {
        "Manage MCP server connections"
    }
    fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            CommandResult::Text(
                "MCP commands: /mcp list, /mcp add <name>, /mcp remove <name>".to_string(),
            )
        } else {
            CommandResult::Text(format!("MCP: {}", args))
        }
    }
}

// === Agent Commands ===

pub struct AgentsCommand;
impl Command for AgentsCommand {
    fn name(&self) -> &str {
        "agents"
    }
    fn description(&self) -> &str {
        "List running agents"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("No agents running.".to_string())
    }
}

pub struct TasksCommand;
impl Command for TasksCommand {
    fn name(&self) -> &str {
        "tasks"
    }
    fn description(&self) -> &str {
        "List all tasks"
    }
    fn execute(&self, _args: &str, _ctx: &CommandContext) -> CommandResult {
        CommandResult::Text("TASKS_LIST".to_string())
    }
}
