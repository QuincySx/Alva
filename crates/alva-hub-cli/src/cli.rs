// INPUT:  process argument strings
// OUTPUT: Engine, ClaudeMode, PermissionPolicy, Invocation, ParseOutcome, ParseError, parse_args
// POS:    Fail-closed command-line grammar for selecting one Hub engine and one prompt.

use std::fmt;

/// Product-visible engine choices supported by this CLI build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Engine {
    Alva,
    Claude,
    Codex,
}

impl Engine {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Alva => "alva",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn parse(value: &str) -> Result<Self, ParseError> {
        match value {
            "alva" => Ok(Self::Alva),
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            _ => Err(ParseError::UnknownEngine(value.into())),
        }
    }
}

/// Claude transport for `--engine claude`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ClaudeMode {
    /// `claude -p --output-format stream-json` (default Hub one-shot).
    #[default]
    Light,
    /// Node bridge + `@anthropic-ai/claude-agent-sdk`.
    Full,
}

impl ClaudeMode {
    fn parse(value: &str) -> Result<Self, ParseError> {
        match value {
            "light" | "print" | "cli" | "p" => Ok(Self::Light),
            "full" | "sdk" => Ok(Self::Full),
            _ => Err(ParseError::UnknownClaudeMode(value.into())),
        }
    }
}

/// How this CLI answers engine `PermissionRequest`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionPolicy {
    /// Fail-closed: always deny (default; matches Claude light / Codex never).
    #[default]
    Deny,
    /// Auto-allow once per request (still non-interactive; use carefully).
    Allow,
    /// Prompt on stderr/stdin for y/N (falls back to deny if stdin is not a TTY).
    Ask,
}

impl PermissionPolicy {
    fn parse(value: &str) -> Result<Self, ParseError> {
        match value {
            "deny" | "dontAsk" | "dont-ask" | "never" => Ok(Self::Deny),
            "allow" | "auto" | "accept" => Ok(Self::Allow),
            "ask" | "prompt" | "interactive" => Ok(Self::Ask),
            _ => Err(ParseError::UnknownPermission(value.into())),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::Allow => "allow",
            Self::Ask => "ask",
        }
    }
}

/// A validated Hub invocation (one-shot and/or REPL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) engine: Engine,
    /// True when the user passed `--engine` (journal must not silently override).
    pub(crate) engine_explicit: bool,
    /// Only meaningful when `engine == Claude`; ignored otherwise.
    pub(crate) claude_mode: ClaudeMode,
    /// Optional workspace override (`--cwd`); engines use process cwd when None.
    pub(crate) cwd: Option<String>,
    /// Resume a product session id from the on-disk journal (and Hub restore).
    pub(crate) session: Option<String>,
    /// Multi-turn REPL in one process (session stays alive for resume).
    pub(crate) repl: bool,
    /// Print ToolStart/ToolEnd / session metadata on stderr.
    pub(crate) verbose: bool,
    pub(crate) permission: PermissionPolicy,
    /// Empty when `--repl` with no initial prompt.
    pub(crate) prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseOutcome {
    Help,
    Run(Invocation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseError {
    MissingEngineValue,
    DuplicateEngine,
    UnknownEngine(String),
    MissingClaudeModeValue,
    DuplicateClaudeMode,
    UnknownClaudeMode(String),
    ClaudeModeWithoutClaudeEngine,
    MissingCwdValue,
    DuplicateCwd,
    MissingSessionValue,
    DuplicateSession,
    DuplicateRepl,
    DuplicateVerbose,
    MissingPermissionValue,
    DuplicatePermission,
    UnknownPermission(String),
    UnknownOption(String),
    EmptyPrompt,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEngineValue => {
                write!(f, "--engine requires 'alva', 'claude', or 'codex'")
            }
            Self::DuplicateEngine => write!(f, "--engine may be supplied only once"),
            Self::UnknownEngine(engine) => {
                write!(
                    f,
                    "unknown engine '{engine}' (expected alva, claude, or codex)"
                )
            }
            Self::MissingClaudeModeValue => {
                write!(f, "--claude-mode requires 'light' or 'full'")
            }
            Self::DuplicateClaudeMode => write!(f, "--claude-mode may be supplied only once"),
            Self::UnknownClaudeMode(mode) => {
                write!(f, "unknown claude mode '{mode}' (expected light or full)")
            }
            Self::ClaudeModeWithoutClaudeEngine => {
                write!(f, "--claude-mode is only valid with --engine claude")
            }
            Self::MissingCwdValue => write!(f, "--cwd requires a directory path"),
            Self::DuplicateCwd => write!(f, "--cwd may be supplied only once"),
            Self::MissingSessionValue => write!(f, "--session requires a product session id"),
            Self::DuplicateSession => write!(f, "--session may be supplied only once"),
            Self::DuplicateRepl => write!(f, "--repl may be supplied only once"),
            Self::DuplicateVerbose => write!(f, "--verbose may be supplied only once"),
            Self::MissingPermissionValue => {
                write!(f, "--permission requires 'deny', 'allow', or 'ask'")
            }
            Self::DuplicatePermission => write!(f, "--permission may be supplied only once"),
            Self::UnknownPermission(value) => {
                write!(
                    f,
                    "unknown permission '{value}' (expected deny, allow, or ask)"
                )
            }
            Self::UnknownOption(option) => write!(f, "unknown option '{option}'"),
            Self::EmptyPrompt => write!(
                f,
                "a prompt is required (or pass --repl for multi-turn mode)"
            ),
        }
    }
}

/// Parse CLI arguments without inspecting environment or constructing engines.
pub(crate) fn parse_args(args: &[String]) -> Result<ParseOutcome, ParseError> {
    let mut engine = Engine::Alva;
    let mut engine_set = false;
    let mut claude_mode = ClaudeMode::Light;
    let mut claude_mode_set = false;
    let mut cwd: Option<String> = None;
    let mut session: Option<String> = None;
    let mut repl = false;
    let mut verbose = false;
    let mut permission = PermissionPolicy::Deny;
    let mut permission_set = false;
    let mut prompt_parts = Vec::new();
    let mut options_ended = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if options_ended {
            prompt_parts.push(argument.clone());
            index += 1;
            continue;
        }
        if matches!(argument.as_str(), "--help" | "-h") {
            return Ok(ParseOutcome::Help);
        }
        if argument == "--" {
            options_ended = true;
            index += 1;
            continue;
        }
        if argument == "--repl" {
            if repl {
                return Err(ParseError::DuplicateRepl);
            }
            repl = true;
            index += 1;
            continue;
        }
        if matches!(argument.as_str(), "--verbose" | "-v") {
            if verbose {
                return Err(ParseError::DuplicateVerbose);
            }
            verbose = true;
            index += 1;
            continue;
        }
        if argument == "--session" {
            if session.is_some() {
                return Err(ParseError::DuplicateSession);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingSessionValue)?;
            session = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--session=") {
            if session.is_some() {
                return Err(ParseError::DuplicateSession);
            }
            if value.is_empty() {
                return Err(ParseError::MissingSessionValue);
            }
            session = Some(value.to_string());
            index += 1;
            continue;
        }
        if argument == "--cwd" {
            if cwd.is_some() {
                return Err(ParseError::DuplicateCwd);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingCwdValue)?;
            cwd = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--cwd=") {
            if cwd.is_some() {
                return Err(ParseError::DuplicateCwd);
            }
            if value.is_empty() {
                return Err(ParseError::MissingCwdValue);
            }
            cwd = Some(value.to_string());
            index += 1;
            continue;
        }
        if argument == "--permission" {
            if permission_set {
                return Err(ParseError::DuplicatePermission);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingPermissionValue)?;
            permission = PermissionPolicy::parse(value)?;
            permission_set = true;
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--permission=") {
            if permission_set {
                return Err(ParseError::DuplicatePermission);
            }
            if value.is_empty() {
                return Err(ParseError::MissingPermissionValue);
            }
            permission = PermissionPolicy::parse(value)?;
            permission_set = true;
            index += 1;
            continue;
        }
        if argument == "--engine" {
            if engine_set {
                return Err(ParseError::DuplicateEngine);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingEngineValue)?;
            engine = Engine::parse(value)?;
            engine_set = true;
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--engine=") {
            if engine_set {
                return Err(ParseError::DuplicateEngine);
            }
            if value.is_empty() {
                return Err(ParseError::MissingEngineValue);
            }
            engine = Engine::parse(value)?;
            engine_set = true;
            index += 1;
            continue;
        }
        if argument == "--claude-mode" {
            if claude_mode_set {
                return Err(ParseError::DuplicateClaudeMode);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingClaudeModeValue)?;
            claude_mode = ClaudeMode::parse(value)?;
            claude_mode_set = true;
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--claude-mode=") {
            if claude_mode_set {
                return Err(ParseError::DuplicateClaudeMode);
            }
            if value.is_empty() {
                return Err(ParseError::MissingClaudeModeValue);
            }
            claude_mode = ClaudeMode::parse(value)?;
            claude_mode_set = true;
            index += 1;
            continue;
        }
        if argument.starts_with('-') {
            return Err(ParseError::UnknownOption(argument.clone()));
        }
        prompt_parts.push(argument.clone());
        index += 1;
    }

    if claude_mode_set && engine != Engine::Claude {
        return Err(ParseError::ClaudeModeWithoutClaudeEngine);
    }

    let prompt = prompt_parts.join(" ");
    // Resume with --session alone is allowed (empty prompt) when --repl, or
    // requires a prompt for one-shot continue.
    if prompt.trim().is_empty() && !repl {
        return Err(ParseError::EmptyPrompt);
    }
    Ok(ParseOutcome::Run(Invocation {
        engine,
        engine_explicit: engine_set,
        claude_mode,
        cwd,
        session,
        repl,
        verbose,
        permission,
        prompt,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn defaults_to_alva() {
        assert_eq!(
            parse_args(&args(&["hello", "world"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hello world".into(),
            }))
        );
    }

    #[test]
    fn parses_engine_space_and_equals_forms() {
        for values in [
            &["--engine", "claude", "hello"][..],
            &["--engine=claude", "hello"][..],
        ] {
            assert_eq!(
                parse_args(&args(values)),
                Ok(ParseOutcome::Run(Invocation {
                    engine: Engine::Claude,
                    engine_explicit: true,
                    claude_mode: ClaudeMode::Light,
                    cwd: None,
                    session: None,
                    repl: false,
                    verbose: false,
                    permission: PermissionPolicy::Deny,
                    prompt: "hello".into(),
                }))
            );
        }
    }

    #[test]
    fn engine_ids_match_registry_contract() {
        assert_eq!(Engine::Alva.id(), "alva");
        assert_eq!(Engine::Claude.id(), "claude");
        assert_eq!(Engine::Codex.id(), "codex");
    }

    #[test]
    fn parses_codex_engine() {
        assert_eq!(
            parse_args(&args(&["--engine", "codex", "hello"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Codex,
                engine_explicit: true,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hello".into(),
            }))
        );
    }

    #[test]
    fn parses_claude_mode_full_and_light() {
        assert_eq!(
            parse_args(&args(&[
                "--engine",
                "claude",
                "--claude-mode",
                "full",
                "hello"
            ])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                engine_explicit: true,
                claude_mode: ClaudeMode::Full,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hello".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--engine=claude", "--claude-mode=sdk", "hi"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                engine_explicit: true,
                claude_mode: ClaudeMode::Full,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hi".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--engine=claude", "--claude-mode=print", "hi"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                engine_explicit: true,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hi".into(),
            }))
        );
    }

    #[test]
    fn parses_cwd() {
        assert_eq!(
            parse_args(&args(&["--cwd", "/tmp/ws", "hello"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: Some("/tmp/ws".into()),
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hello".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--cwd=/tmp/ws", "hello"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: Some("/tmp/ws".into()),
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "hello".into(),
            }))
        );
    }

    #[test]
    fn parses_repl_verbose_permission() {
        assert_eq!(
            parse_args(&args(&[
                "--repl",
                "--verbose",
                "--permission",
                "allow",
                "first"
            ])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: true,
                verbose: true,
                permission: PermissionPolicy::Allow,
                prompt: "first".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--repl"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: true,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: String::new(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--permission=allow", "x"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Allow,
                prompt: "x".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--permission", "ask", "x"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Ask,
                prompt: "x".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--session", "abc-123", "--repl"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Alva,
                engine_explicit: false,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: Some("abc-123".into()),
                repl: true,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: String::new(),
            }))
        );
    }

    #[test]
    fn rejects_claude_mode_on_non_claude_engine() {
        assert_eq!(
            parse_args(&args(&["--engine", "alva", "--claude-mode", "light", "x"])),
            Err(ParseError::ClaudeModeWithoutClaudeEngine)
        );
    }

    #[test]
    fn rejects_unknown_or_missing_engine_values() {
        assert_eq!(
            parse_args(&args(&["--engine", "nope", "x"])),
            Err(ParseError::UnknownEngine("nope".into()))
        );
        assert_eq!(
            parse_args(&args(&["--engine"])),
            Err(ParseError::MissingEngineValue)
        );
    }

    #[test]
    fn rejects_unknown_options_and_empty_prompts() {
        assert_eq!(
            parse_args(&args(&["--wat", "x"])),
            Err(ParseError::UnknownOption("--wat".into()))
        );
        assert_eq!(parse_args(&args(&[])), Err(ParseError::EmptyPrompt));
    }

    #[test]
    fn double_dash_allows_prompt_parts_that_start_with_dash() {
        assert_eq!(
            parse_args(&args(&["--engine", "claude", "--", "--explain", "-v"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                engine_explicit: true,
                claude_mode: ClaudeMode::Light,
                cwd: None,
                session: None,
                repl: false,
                verbose: false,
                permission: PermissionPolicy::Deny,
                prompt: "--explain -v".into(),
            }))
        );
    }

    #[test]
    fn help_is_successful() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(ParseOutcome::Help));
        assert_eq!(parse_args(&args(&["-h"])), Ok(ParseOutcome::Help));
    }
}
