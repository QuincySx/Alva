use super::builtins;
use super::types::{Command, CommandContext, CommandResult};

/// Command registry holding all available commands
pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: Vec::new(),
        };
        registry.register_builtins();
        registry
    }

    fn register_builtins(&mut self) {
        // Session commands
        self.register(Box::new(builtins::ClearCommand));
        self.register(Box::new(builtins::CompactCommand));
        self.register(Box::new(builtins::NewCommand));

        // Navigation commands
        self.register(Box::new(builtins::HelpCommand));
        self.register(Box::new(builtins::ExitCommand));

        // Info commands
        self.register(Box::new(builtins::CostCommand));
        self.register(Box::new(builtins::StatusCommand));
        self.register(Box::new(builtins::DoctorCommand));

        // Config commands
        self.register(Box::new(builtins::ConfigCommand));
        self.register(Box::new(builtins::ModelCommand));
        self.register(Box::new(builtins::ThemeCommand));
        self.register(Box::new(builtins::PermissionsCommand));

        // Mode commands
        self.register(Box::new(builtins::PlanCommand));
        self.register(Box::new(builtins::FastCommand));
        self.register(Box::new(builtins::VimCommand));

        // Git commands (prompt type)
        self.register(Box::new(builtins::CommitCommand));
        self.register(Box::new(builtins::ReviewCommand));

        // Export commands
        self.register(Box::new(builtins::ExportCommand));
        self.register(Box::new(builtins::CopyCommand));
        self.register(Box::new(builtins::SummaryCommand));

        // Tool commands
        self.register(Box::new(builtins::ToolsCommand));
        self.register(Box::new(builtins::McpCommand));

        // Agent commands
        self.register(Box::new(builtins::AgentsCommand));
        self.register(Box::new(builtins::TasksCommand));
    }

    pub fn register(&mut self, command: Box<dyn Command>) {
        self.commands.push(command);
    }

    /// Find a command by name or alias
    pub fn find(&self, name: &str) -> Option<&dyn Command> {
        self.commands
            .iter()
            .find(|cmd| cmd.name() == name || cmd.aliases().contains(&name))
            .map(|cmd| cmd.as_ref())
    }

    /// List all available commands
    pub fn list(&self) -> Vec<(&str, &str)> {
        self.commands
            .iter()
            .filter(|cmd| cmd.is_enabled())
            .map(|cmd| (cmd.name(), cmd.description()))
            .collect()
    }

    /// Parse and execute a slash command
    pub fn execute(&self, input: &str, ctx: &CommandContext) -> Option<CommandResult> {
        let input = input.trim();
        if !input.starts_with('/') {
            return None;
        }

        let input = &input[1..]; // Remove '/'
        let (name, args) = match input.find(char::is_whitespace) {
            Some(pos) => (&input[..pos], input[pos..].trim()),
            None => (input, ""),
        };

        if let Some(cmd) = self.find(name) {
            Some(cmd.execute(args, ctx))
        } else {
            Some(CommandResult::Error(format!("Unknown command: /{}", name)))
        }
    }
}
