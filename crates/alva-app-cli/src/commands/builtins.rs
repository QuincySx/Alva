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
    fn aliases(&self) -> Vec<&str> {
        vec!["compact-summary"]
    }
    fn description(&self) -> &str {
        "Compact conversation to save context"
    }
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult {
        let custom_instructions = if args.is_empty() {
            None
        } else {
            Some(args.to_string())
        };

        let prompt = if let Some(instructions) = custom_instructions {
            format!(
                "Summarize the conversation so far into a concise but complete summary that \
                 preserves all important context, decisions, and code changes. \
                 Additional instructions: {}. \
                 Then continue the conversation from the summary.",
                instructions,
            )
        } else {
            "Summarize the conversation so far into a concise but complete summary that \
             preserves all important context, decisions, and code changes. \
             Then continue the conversation from the summary."
                .to_string()
        };

        let msg_count = ctx.message_count;
        let tokens = ctx.token_usage.total();

        CommandResult::Compact {
            summary: format!(
                "Compacting {} messages ({} tokens). {}",
                msg_count,
                tokens,
                prompt,
            ),
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
  /compact [text]    Compact conversation to save context
  /new               Start a new conversation
  /resume            Resume a previous conversation
  /sessions          List all sessions
  /fork              Fork current session
  /rewind            Rewind to checkpoint
  /model <name>      Switch model
  /config            Show configuration
  /cost              Show token usage and cost
  /status            Show system status
  /doctor            Run diagnostics
  /commit            Create a git commit (AI-assisted)
  /review            Review code changes (AI-assisted)
  /export [file]     Export conversation to file
  /copy              Copy last response to clipboard
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
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandResult {
        let usage = &ctx.token_usage;
        let cost = usage.estimated_cost_usd();

        let mut lines = Vec::new();
        lines.push(format!("Session: {}", ctx.session_id));
        lines.push(format!("Model:   {}", ctx.model));
        lines.push(String::new());
        lines.push(format!("Input tokens:  {:>10}", format_number(usage.input_tokens)));
        lines.push(format!("Output tokens: {:>10}", format_number(usage.output_tokens)));
        lines.push(format!("Total tokens:  {:>10}", format_number(usage.total())));
        lines.push(String::new());
        lines.push(format!("Estimated cost: ${:.4}", cost));
        lines.push(format!("Messages:       {}", ctx.message_count));

        CommandResult::Text(lines.join("\n"))
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
        let mut lines = Vec::new();
        lines.push(format!("Model:       {}", ctx.model));
        lines.push(format!("Workspace:   {}", ctx.workspace.display()));
        lines.push(format!("Session:     {}", ctx.session_id));
        lines.push(format!("Messages:    {}", ctx.message_count));
        lines.push(format!(
            "Plan mode:   {}",
            if ctx.plan_mode { "ON" } else { "OFF" }
        ));
        lines.push(format!(
            "Tokens used: {}",
            format_number(ctx.token_usage.total())
        ));
        lines.push(format!("Tools:       {} registered", ctx.tool_names.len()));

        // Git info
        if let Some(branch) = git_current_branch(ctx.workspace) {
            lines.push(format!("Git branch:  {}", branch));
        }

        CommandResult::Text(lines.join("\n"))
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
        let mut checks: Vec<(bool, String)> = Vec::new();

        // 1. Workspace
        let ws_exists = ctx.workspace.exists();
        checks.push((
            ws_exists,
            format!("Workspace: {}", ctx.workspace.display()),
        ));

        // 2. Git
        let git_ok = run_cmd(ctx.workspace, "git", &["rev-parse", "--git-dir"]);
        checks.push((git_ok.is_some(), "Git repository detected".to_string()));

        if let Some(branch) = git_current_branch(ctx.workspace) {
            checks.push((true, format!("Git branch: {}", branch)));
        }

        // 3. Model / API
        checks.push((!ctx.model.is_empty(), format!("Model: {}", ctx.model)));

        let api_key_set = std::env::var("ALVA_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .is_ok();
        checks.push((api_key_set, "API key configured".to_string()));

        // 4. ripgrep
        let rg_ok = run_cmd(ctx.workspace, "rg", &["--version"]);
        checks.push((rg_ok.is_some(), "ripgrep (rg) available".to_string()));

        // 5. Config files
        let global_config = dirs::home_dir()
            .map(|h| h.join(".config").join("alva").join("settings.json"))
            .unwrap_or_default();
        checks.push((
            global_config.exists(),
            format!("Global config: {}", global_config.display()),
        ));

        let project_config = ctx.workspace.join(".claude").join("settings.json");
        checks.push((
            project_config.exists(),
            format!("Project config: {}", project_config.display()),
        ));

        // 6. Session store
        let session_dir = ctx.workspace.join(".alva").join("sessions");
        let session_count = std::fs::read_dir(&session_dir)
            .map(|rd| rd.count())
            .unwrap_or(0);
        checks.push((
            session_dir.exists(),
            format!("Session store: {} sessions", session_count),
        ));

        // 7. Tools
        checks.push((
            !ctx.tool_names.is_empty(),
            format!("Tools: {} registered", ctx.tool_names.len()),
        ));

        // Format output
        let mut output = Vec::new();
        output.push("Diagnostics:".to_string());
        for (ok, msg) in &checks {
            let icon = if *ok { "✓" } else { "✗" };
            output.push(format!("  {} {}", icon, msg));
        }

        let pass_count = checks.iter().filter(|(ok, _)| *ok).count();
        let total = checks.len();
        output.push(String::new());
        output.push(format!("{}/{} checks passed", pass_count, total));

        CommandResult::Text(output.join("\n"))
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
            let mut lines = Vec::new();
            lines.push(format!("Model:     {}", ctx.model));
            lines.push(format!("Workspace: {}", ctx.workspace.display()));
            lines.push(format!("Session:   {}", ctx.session_id));
            lines.push(format!(
                "Plan mode: {}",
                if ctx.plan_mode { "ON" } else { "OFF" }
            ));
            lines.push(String::new());

            // Show config file paths
            lines.push("Config files:".to_string());
            let paths = [
                (
                    "Global",
                    dirs::home_dir()
                        .map(|h| h.join(".config").join("alva").join("settings.json"))
                        .unwrap_or_default(),
                ),
                (
                    "Project",
                    ctx.workspace.join(".claude").join("settings.json"),
                ),
                (
                    "Local",
                    ctx.workspace.join(".claude").join("settings.local.json"),
                ),
            ];
            for (label, path) in &paths {
                let mark = if path.exists() { "" } else { " (not found)" };
                lines.push(format!("  {}: {}{}", label, path.display(), mark));
            }

            // Show project settings content if exists
            let project_settings = ctx.workspace.join(".claude").join("settings.json");
            if project_settings.exists() {
                if let Ok(content) = std::fs::read_to_string(&project_settings) {
                    lines.push(String::new());
                    lines.push("Project settings:".to_string());
                    lines.push(content);
                }
            }

            CommandResult::Text(lines.join("\n"))
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
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandResult {
        let mut lines = Vec::new();
        lines.push(format!(
            "Permission mode: {}",
            if ctx.plan_mode { "plan (read-only)" } else { "ask" }
        ));
        lines.push(String::new());
        lines.push("Configure permissions in settings.json:".to_string());
        lines.push("  .claude/settings.json (project)".to_string());
        lines.push("  ~/.config/alva/settings.json (global)".to_string());
        lines.push(String::new());
        lines.push("Example:".to_string());
        lines.push(r#"  { "permissions": { "allow": ["Bash(git *)"], "deny": ["Bash(rm *)"] } }"#.to_string());
        CommandResult::Text(lines.join("\n"))
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
        let user_guidance = if args.is_empty() {
            String::new()
        } else {
            format!("\n\nUser guidance: {}", args)
        };

        let prompt = format!(
            r#"## Context

- Current git status: run `git status`
- Current git diff (staged and unstaged): run `git diff HEAD`
- Current branch: run `git branch --show-current`
- Recent commits: run `git log --oneline -10`

## Git Safety Protocol

- NEVER update the git config
- NEVER skip hooks (--no-verify, --no-gpg-sign, etc) unless the user explicitly requests it
- CRITICAL: ALWAYS create NEW commits. NEVER use git commit --amend, unless the user explicitly requests it
- Do not commit files that likely contain secrets (.env, credentials.json, etc). Warn the user if they specifically request to commit those files
- If there are no changes to commit (i.e., no untracked files and no modifications), do not create an empty commit
- Never use git commands with the -i flag (like git rebase -i or git add -i) since they require interactive input which is not supported

## Your task

Based on the above changes, create a single git commit:

1. Run git status and git diff to see the current state
2. Analyze all changes and draft a commit message:
   - Look at the recent commits to follow this repository's commit message style
   - Summarize the nature of the changes (new feature, enhancement, bug fix, refactoring, test, docs, etc.)
   - Draft a concise (1-2 sentences) commit message focusing on the "why" rather than the "what"
3. Stage relevant files and create the commit using HEREDOC syntax:
```bash
git commit -m "$(cat <<'EOF'
Commit message here.
EOF
)"
```

You have the capability to call multiple tools in a single response. Stage and create the commit using a single message. Do not use any other tools.{}"#,
            user_guidance,
        );

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
             `git diff --staged` for staged changes. Provide feedback on:\n\
             1. Code quality and style\n\
             2. Potential bugs or edge cases\n\
             3. Security concerns\n\
             4. Performance implications\n\
             5. Suggestions for improvement"
                .to_string()
        } else if args.starts_with("http") || args.contains('#') {
            format!(
                "Review the pull request at: {}. \
                 Fetch the PR diff and provide feedback on code quality, bugs, security, and improvements.",
                args
            )
        } else {
            format!("Review code changes: {}", args)
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
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult {
        let filename = if args.is_empty() {
            let timestamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
            format!("conversation-{}.md", timestamp)
        } else {
            let name = args.trim().to_string();
            if name.contains('.') {
                name
            } else {
                format!("{}.md", name)
            }
        };

        let path = ctx.workspace.join(&filename);

        // The actual export content will be provided by the REPL layer which has message access.
        // Here we return the signal with the path.
        CommandResult::Text(format!("EXPORT:{}", path.display()))
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
            content: "Provide a brief summary of our conversation so far, including:\n\
                      1. Main topics discussed\n\
                      2. Key decisions made\n\
                      3. Code changes completed\n\
                      4. Outstanding items"
                .to_string(),
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
    fn execute(&self, _args: &str, ctx: &CommandContext) -> CommandResult {
        if ctx.tool_names.is_empty() {
            return CommandResult::Text("No tools registered.".to_string());
        }

        let mut lines = Vec::new();
        lines.push(format!("Available tools ({}):", ctx.tool_names.len()));
        for name in &ctx.tool_names {
            lines.push(format!("  - {}", name));
        }
        CommandResult::Text(lines.join("\n"))
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
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult {
        if args.is_empty() {
            let mut lines = Vec::new();
            lines.push("MCP commands:".to_string());
            lines.push("  /mcp list       List connected MCP servers".to_string());
            lines.push("  /mcp add <cfg>  Add an MCP server".to_string());
            lines.push("  /mcp remove <n> Remove an MCP server".to_string());
            lines.push(String::new());

            // Show MCP config files
            let project_mcp = ctx.workspace.join(".claude").join("mcp.json");
            let global_mcp = dirs::home_dir()
                .map(|h| h.join(".config").join("alva").join("mcp.json"))
                .unwrap_or_default();

            lines.push("Config files:".to_string());
            if project_mcp.exists() {
                lines.push(format!("  Project: {}", project_mcp.display()));
            }
            if global_mcp.exists() {
                lines.push(format!("  Global:  {}", global_mcp.display()));
            }
            if !project_mcp.exists() && !global_mcp.exists() {
                lines.push("  No MCP config found.".to_string());
            }

            CommandResult::Text(lines.join("\n"))
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
        CommandResult::Text("No active tasks.".to_string())
    }
}

// === Helpers ===

fn format_number(n: u64) -> String {
    super::types::format_token_count(n)
}

fn run_cmd(cwd: &std::path::Path, cmd: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn git_current_branch(cwd: &std::path::Path) -> Option<String> {
    run_cmd(cwd, "git", &["rev-parse", "--abbrev-ref", "HEAD"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext<'static> {
        CommandContext {
            workspace: std::path::Path::new("/tmp/test-workspace"),
            home_dir: std::path::PathBuf::from("/tmp/test-home"),
            model: "claude-sonnet-4-20250514",
            session_id: "test-session-123",
            message_count: 42,
            token_usage: TokenUsage {
                input_tokens: 15000,
                output_tokens: 3000,
            },
            tool_names: vec![
                "Bash".to_string(),
                "Read".to_string(),
                "Edit".to_string(),
                "Write".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
            ],
            plan_mode: false,
            extra: HashMap::new(),
        }
    }

    #[test]
    fn cost_shows_token_counts() {
        let ctx = test_ctx();
        let result = CostCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("15.0K"), "should show input tokens: {}", text);
        assert!(text.contains("3.0K"), "should show output tokens: {}", text);
        assert!(text.contains("18.0K"), "should show total tokens: {}", text);
        assert!(text.contains("$"), "should show cost estimate: {}", text);
        assert!(text.contains("42"), "should show message count: {}", text);
    }

    #[test]
    fn cost_alias_usage() {
        let cmd = CostCommand;
        assert!(cmd.aliases().contains(&"usage"));
    }

    #[test]
    fn status_shows_session_info() {
        let ctx = test_ctx();
        let result = StatusCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("claude-sonnet-4-20250514"), "{}", text);
        assert!(text.contains("test-session-123"), "{}", text);
        assert!(text.contains("42"), "should show message count: {}", text);
        assert!(text.contains("6 registered"), "should show tool count: {}", text);
        assert!(text.contains("Plan mode:   OFF"), "{}", text);
    }

    #[test]
    fn status_shows_plan_mode_on() {
        let mut ctx = test_ctx();
        ctx.plan_mode = true;
        let result = StatusCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            _ => panic!("expected Text"),
        };
        assert!(text.contains("Plan mode:   ON"), "{}", text);
    }

    #[test]
    fn doctor_runs_diagnostics() {
        let ctx = test_ctx();
        let result = DoctorCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("Diagnostics:"), "{}", text);
        assert!(text.contains("checks passed"), "{}", text);
        assert!(text.contains("6 registered"), "should check tool count: {}", text);
    }

    #[test]
    fn tools_lists_tool_names() {
        let ctx = test_ctx();
        let result = ToolsCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("Available tools (6):"), "{}", text);
        assert!(text.contains("- Bash"), "{}", text);
        assert!(text.contains("- Grep"), "{}", text);
    }

    #[test]
    fn tools_empty_registry() {
        let mut ctx = test_ctx();
        ctx.tool_names = vec![];
        let result = ToolsCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            _ => panic!("expected Text"),
        };
        assert!(text.contains("No tools registered"), "{}", text);
    }

    #[test]
    fn commit_returns_prompt() {
        let ctx = test_ctx();
        let result = CommitCommand.execute("", &ctx);
        match result {
            CommandResult::Prompt {
                content,
                progress_message,
                allowed_tools,
            } => {
                assert!(content.contains("git status"), "should mention git status");
                assert!(content.contains("git diff"), "should mention git diff");
                assert!(content.contains("HEREDOC"), "should mention HEREDOC syntax");
                assert!(content.contains("NEVER use git commit --amend"), "should include safety protocol");
                assert_eq!(
                    progress_message,
                    Some("Creating commit...".to_string())
                );
                assert!(allowed_tools.is_some());
            }
            other => panic!("expected Prompt, got {:?}", other),
        }
    }

    #[test]
    fn commit_with_guidance() {
        let ctx = test_ctx();
        let result = CommitCommand.execute("fix auth bug", &ctx);
        match result {
            CommandResult::Prompt { content, .. } => {
                assert!(content.contains("fix auth bug"), "should include user guidance");
            }
            other => panic!("expected Prompt, got {:?}", other),
        }
    }

    #[test]
    fn review_default_prompt() {
        let ctx = test_ctx();
        let result = ReviewCommand.execute("", &ctx);
        match result {
            CommandResult::Prompt { content, .. } => {
                assert!(content.contains("git diff"), "{}", content);
                assert!(content.contains("Security concerns"), "{}", content);
            }
            other => panic!("expected Prompt, got {:?}", other),
        }
    }

    #[test]
    fn review_with_pr_url() {
        let ctx = test_ctx();
        let result = ReviewCommand.execute("https://github.com/org/repo/pull/42", &ctx);
        match result {
            CommandResult::Prompt { content, .. } => {
                assert!(content.contains("https://github.com/org/repo/pull/42"), "{}", content);
            }
            other => panic!("expected Prompt, got {:?}", other),
        }
    }

    #[test]
    fn export_generates_filename() {
        let ctx = test_ctx();
        let result = ExportCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.starts_with("EXPORT:"), "{}", text);
        assert!(text.contains("conversation-"), "{}", text);
        assert!(text.contains(".md"), "{}", text);
    }

    #[test]
    fn export_custom_name() {
        let ctx = test_ctx();
        let result = ExportCommand.execute("my-export.txt", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            _ => panic!("expected Text"),
        };
        assert!(text.contains("my-export.txt"), "{}", text);
    }

    #[test]
    fn compact_includes_stats() {
        let ctx = test_ctx();
        let result = CompactCommand.execute("", &ctx);
        match result {
            CommandResult::Compact { summary } => {
                assert!(summary.contains("42 messages"), "{}", summary);
                assert!(summary.contains("18000 tokens"), "{}", summary);
            }
            other => panic!("expected Compact, got {:?}", other),
        }
    }

    #[test]
    fn compact_with_custom_instructions() {
        let ctx = test_ctx();
        let result = CompactCommand.execute("focus on code changes", &ctx);
        match result {
            CommandResult::Compact { summary } => {
                assert!(summary.contains("focus on code changes"), "{}", summary);
            }
            other => panic!("expected Compact, got {:?}", other),
        }
    }

    #[test]
    fn help_lists_all_commands() {
        let ctx = test_ctx();
        let result = HelpCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            _ => panic!("expected Text"),
        };
        // Verify key commands are listed
        for cmd in &["/help", "/cost", "/status", "/doctor", "/commit", "/tools", "/compact", "/export"] {
            assert!(text.contains(cmd), "help should list {}: {}", cmd, text);
        }
    }

    #[test]
    fn format_number_works() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1.0K");
        assert_eq!(format_number(15000), "15.0K");
        assert_eq!(format_number(1_500_000), "1.5M");
    }

    #[test]
    fn registry_finds_by_name_and_alias() {
        let registry = super::super::registry::CommandRegistry::new();
        assert!(registry.find("cost").is_some());
        assert!(registry.find("usage").is_some()); // alias
        assert!(registry.find("exit").is_some());
        assert!(registry.find("quit").is_some()); // alias
        assert!(registry.find("q").is_some()); // alias
        assert!(registry.find("nonexistent").is_none());
    }

    #[test]
    fn registry_execute_dispatches() {
        let registry = super::super::registry::CommandRegistry::new();
        let ctx = test_ctx();
        let result = registry.execute("/cost", &ctx);
        assert!(result.is_some());
        match result.unwrap() {
            CommandResult::Text(t) => assert!(t.contains("tokens")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn registry_unknown_command() {
        let registry = super::super::registry::CommandRegistry::new();
        let ctx = test_ctx();
        let result = registry.execute("/foobar", &ctx);
        assert!(result.is_some());
        match result.unwrap() {
            CommandResult::Error(e) => assert!(e.contains("foobar")),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn permissions_shows_mode() {
        let ctx = test_ctx();
        let result = PermissionsCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            _ => panic!("expected Text"),
        };
        assert!(text.contains("ask"), "{}", text);
    }

    #[test]
    fn config_shows_info() {
        let ctx = test_ctx();
        let result = ConfigCommand.execute("", &ctx);
        let text = match result {
            CommandResult::Text(t) => t,
            _ => panic!("expected Text"),
        };
        assert!(text.contains("claude-sonnet-4-20250514"), "{}", text);
        assert!(text.contains("test-session-123"), "{}", text);
    }
}
