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

#[cfg(test)]
mod tests {
    //! Pure-logic tests for CommandRegistry. We do NOT exercise the
    //! individual command bodies here — builtins.rs already has its own
    //! 22 tests for those. The registry's job is *routing*: name+alias
    //! lookup, list-with-enabled-filter, and slash-parse → dispatch.
    //!
    //! For dispatch-shape tests we register a tiny `TestCmd` so a failure
    //! pinpoints the registry, not whatever real command happened to be
    //! standing in for it.
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    use super::super::types::TokenUsage;

    /// Test command that records the args it was last invoked with, so
    /// `execute_*` tests can assert the slash parser passed through
    /// exactly what was after the first whitespace.
    struct TestCmd {
        name: &'static str,
        aliases: Vec<&'static str>,
        enabled: bool,
        last_args: Mutex<Option<String>>,
    }
    impl TestCmd {
        fn new(name: &'static str) -> Self {
            Self { name, aliases: vec![], enabled: true, last_args: Mutex::new(None) }
        }
        fn with_aliases(mut self, aliases: Vec<&'static str>) -> Self {
            self.aliases = aliases;
            self
        }
        fn disabled(mut self) -> Self {
            self.enabled = false;
            self
        }
    }
    impl Command for TestCmd {
        fn name(&self) -> &str { self.name }
        fn aliases(&self) -> Vec<&str> { self.aliases.clone() }
        fn description(&self) -> &str { "test command" }
        fn is_enabled(&self) -> bool { self.enabled }
        fn execute(&self, args: &str, _ctx: &CommandContext) -> CommandResult {
            *self.last_args.lock().unwrap() = Some(args.to_string());
            CommandResult::Text(format!("ran with: {args}"))
        }
    }

    fn test_ctx() -> CommandContext<'static> {
        CommandContext {
            workspace: Path::new("/tmp/wsp"),
            model: "claude-sonnet-4-20250514",
            session_id: "sid",
            message_count: 0,
            token_usage: TokenUsage::default(),
            tool_names: vec![],
            plan_mode: false,
        }
    }

    #[test]
    fn new_populates_builtins_and_list_returns_pairs() {
        let reg = CommandRegistry::new();
        let list = reg.list();
        // 25 builtins are registered in register_builtins(); allow >=20 to
        // tolerate one or two becoming `is_enabled() == false` later.
        assert!(list.len() >= 20, "expected >=20 enabled builtins, got {}", list.len());
        // Spot-check a stable subset
        let names: Vec<&str> = list.iter().map(|(n, _)| *n).collect();
        for must in ["clear", "help", "model", "config", "status"] {
            assert!(names.contains(&must), "missing builtin `{must}`: {names:?}");
        }
    }

    #[test]
    fn register_and_find_custom_command() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(TestCmd::new("ztest")));
        let cmd = reg.find("ztest").expect("registered command should be findable");
        assert_eq!(cmd.name(), "ztest");
    }

    #[test]
    fn find_by_alias_works() {
        // `cost` builtin advertises `usage` as an alias (see
        // builtins::tests::cost_alias_usage). Use a real registered alias
        // so this test breaks loudly if the alias is renamed.
        let reg = CommandRegistry::new();
        let by_alias = reg.find("usage").expect("alias `usage` should resolve");
        assert_eq!(by_alias.name(), "cost", "alias should map to cost command");
    }

    #[test]
    fn find_unknown_returns_none() {
        let reg = CommandRegistry::new();
        assert!(reg.find("definitely-not-a-command-zzzz").is_none());
    }

    #[test]
    fn list_filters_out_disabled_commands() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(TestCmd::new("hidden").disabled()));
        let list = reg.list();
        assert!(
            !list.iter().any(|(n, _)| *n == "hidden"),
            "disabled command leaked into list(): {list:?}"
        );
        // But it must still be findable by name — `is_enabled` only gates
        // listing, not lookup (mirrors real builtins that may be hidden
        // from help but still invokable).
        assert!(reg.find("hidden").is_some(), "disabled command should still be findable");
    }

    #[test]
    fn execute_non_slash_input_returns_none() {
        let reg = CommandRegistry::new();
        let ctx = test_ctx();
        assert!(reg.execute("hello world", &ctx).is_none(), "non-slash must short-circuit");
        // Empty string also lacks the leading `/`
        assert!(reg.execute("", &ctx).is_none(), "empty input must short-circuit");
    }

    #[test]
    fn execute_unknown_slash_returns_error_result() {
        let reg = CommandRegistry::new();
        let ctx = test_ctx();
        let out = reg.execute("/totally-fake-cmd", &ctx).expect("slash must produce Some");
        match out {
            CommandResult::Error(msg) => {
                assert!(msg.contains("Unknown command"), "error wording changed: {msg}");
                assert!(msg.contains("totally-fake-cmd"), "error should echo name: {msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn execute_parses_args_after_first_whitespace_and_dispatches() {
        // Verifies: (a) leading `/` is stripped, (b) name == chars up to
        // first whitespace, (c) args == rest, trimmed, (d) the right
        // command gets invoked. Register a TestCmd that records args.
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(TestCmd::new("targs")));
        let ctx = test_ctx();

        // Trailing/leading whitespace inside args is trimmed by execute()
        let out = reg.execute("/targs   foo bar   ", &ctx).expect("slash → Some");
        match out {
            CommandResult::Text(t) => assert!(t.contains("foo bar"), "args echo missing: {t}"),
            other => panic!("expected Text, got {other:?}"),
        }

        // Name-only (no args, no trailing whitespace) still dispatches with empty args
        let out2 = reg.execute("/targs", &ctx).expect("slash → Some");
        match out2 {
            CommandResult::Text(t) => assert_eq!(t, "ran with: ", "empty-args echo wrong: {t}"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// Guard: aliases vector returned by Command trait is consulted with
    /// `.contains(&name)`, which compares `&&str`. If a refactor changes
    /// the alias storage shape (e.g. to `&[&str]`) this test still passes,
    /// but if someone accidentally compares with `==` against a different
    /// type it should break here.
    #[test]
    fn find_does_not_match_partial_or_case_variant() {
        let reg = CommandRegistry::new();
        // `cost` exists; `cos` is a prefix but not a registered name/alias
        assert!(reg.find("cos").is_none(), "prefix match should NOT resolve");
        // case-sensitive
        assert!(reg.find("CLEAR").is_none(), "lookup should be case-sensitive");
        // sanity: lowercase form still works
        assert!(reg.find("clear").is_some());
    }

    /// Isolated alias dispatch test using a TestCmd. The
    /// `find_by_alias_works` test above relies on the bundled
    /// `cost`→`usage` alias as a fixture; if that alias is ever renamed
    /// or removed, that test fails for the wrong reason (looks like a
    /// dispatch regression but is actually a builtin contract change).
    /// This test pins dispatch logic for ARBITRARY aliases independent
    /// of any builtin, and exercises the `with_aliases` test helper.
    #[test]
    fn find_resolves_arbitrary_test_cmd_aliases() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(
            TestCmd::new("primary").with_aliases(vec!["alt1", "alt2"]),
        ));

        // Each alias must route to the same command — and that command's
        // name() must be the primary one (not the alias).
        for needle in ["primary", "alt1", "alt2"] {
            let cmd = reg
                .find(needle)
                .unwrap_or_else(|| panic!("`{needle}` must resolve"));
            assert_eq!(cmd.name(), "primary", "alias `{needle}` returned wrong command");
        }

        // Unregistered alias still resolves to None (regression guard
        // against `.contains(&name)` accidentally becoming an open match).
        assert!(reg.find("alt3").is_none());
    }
}
