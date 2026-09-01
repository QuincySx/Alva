// INPUT:  process argument strings
// OUTPUT: Engine, policies, bounded Invocation, version/help outcomes, parse_args
// POS:    Fail-closed command-line grammar for selecting and bounding one Hub execution.

use std::fmt;

const MAX_TIMEOUT_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_AGENT_TURNS: u32 = 10_000;
const MAX_SESSION_ID_BYTES: usize = 128;

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
    /// Total wall-clock budget for each turn. `None` uses the production
    /// default selected by the binary.
    pub(crate) timeout_seconds: Option<u64>,
    /// Engine-level agentic turn ceiling when the selected adapter supports it.
    pub(crate) max_turns: Option<u32>,
    /// Empty when `--repl` with no initial prompt.
    pub(crate) prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseOutcome {
    Help,
    Version,
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
    InvalidSession(String),
    DuplicateRepl,
    DuplicateVerbose,
    MissingPermissionValue,
    DuplicatePermission,
    UnknownPermission(String),
    MissingTimeoutValue,
    DuplicateTimeout,
    InvalidTimeout(String),
    MissingMaxTurnsValue,
    DuplicateMaxTurns,
    InvalidMaxTurns(String),
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
            Self::InvalidSession(value) => write!(
                f,
                "invalid --session '{}' (expected 1–128 ASCII letters, digits, '-' or '_')",
                printable_value(value)
            ),
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
            Self::MissingTimeoutValue => write!(f, "--timeout requires positive seconds"),
            Self::DuplicateTimeout => write!(f, "--timeout may be supplied only once"),
            Self::InvalidTimeout(value) => {
                write!(
                    f,
                    "invalid --timeout '{value}' (expected 1–{MAX_TIMEOUT_SECONDS} seconds)"
                )
            }
            Self::MissingMaxTurnsValue => write!(f, "--max-turns requires a positive integer"),
            Self::DuplicateMaxTurns => write!(f, "--max-turns may be supplied only once"),
            Self::InvalidMaxTurns(value) => {
                write!(
                    f,
                    "invalid --max-turns '{value}' (expected 1–{MAX_AGENT_TURNS})"
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
    let mut timeout_seconds = None;
    let mut max_turns = None;
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
        if matches!(argument.as_str(), "--version" | "-V") {
            return Ok(ParseOutcome::Version);
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
            validate_session_id(value)?;
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
            validate_session_id(value)?;
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
        if argument == "--timeout" {
            if timeout_seconds.is_some() {
                return Err(ParseError::DuplicateTimeout);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingTimeoutValue)?;
            timeout_seconds = Some(parse_timeout(value)?);
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--timeout=") {
            if timeout_seconds.is_some() {
                return Err(ParseError::DuplicateTimeout);
            }
            if value.is_empty() {
                return Err(ParseError::MissingTimeoutValue);
            }
            timeout_seconds = Some(parse_timeout(value)?);
            index += 1;
            continue;
        }
        if argument == "--max-turns" {
            if max_turns.is_some() {
                return Err(ParseError::DuplicateMaxTurns);
            }
            let value = args
                .get(index + 1)
                .filter(|value| !value.starts_with('-'))
                .ok_or(ParseError::MissingMaxTurnsValue)?;
            max_turns = Some(parse_max_turns(value)?);
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--max-turns=") {
            if max_turns.is_some() {
                return Err(ParseError::DuplicateMaxTurns);
            }
            if value.is_empty() {
                return Err(ParseError::MissingMaxTurnsValue);
            }
            max_turns = Some(parse_max_turns(value)?);
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
        timeout_seconds,
        max_turns,
        prompt,
    }))
}

fn parse_timeout(value: &str) -> Result<u64, ParseError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| (1..=MAX_TIMEOUT_SECONDS).contains(seconds))
        .ok_or_else(|| ParseError::InvalidTimeout(value.to_string()))
}

fn parse_max_turns(value: &str) -> Result<u32, ParseError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|turns| (1..=MAX_AGENT_TURNS).contains(turns))
        .ok_or_else(|| ParseError::InvalidMaxTurns(value.to_string()))
}

fn validate_session_id(value: &str) -> Result<(), ParseError> {
    if value.is_empty()
        || value.len() > MAX_SESSION_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ParseError::InvalidSession(value.into()));
    }
    Ok(())
}

fn printable_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(80)
        .collect()
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
                timeout_seconds: None,
                max_turns: None,
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
                    timeout_seconds: None,
                    max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
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
                timeout_seconds: None,
                max_turns: None,
                prompt: "--explain -v".into(),
            }))
        );
    }

    #[test]
    fn help_is_successful() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(ParseOutcome::Help));
        assert_eq!(parse_args(&args(&["-h"])), Ok(ParseOutcome::Help));
        assert_eq!(parse_args(&args(&["--version"])), Ok(ParseOutcome::Version));
        assert_eq!(parse_args(&args(&["-V"])), Ok(ParseOutcome::Version));
    }

    #[test]
    fn parses_and_validates_execution_bounds() {
        let ParseOutcome::Run(invocation) =
            parse_args(&args(&["--timeout=45", "--max-turns", "7", "do work"])).unwrap()
        else {
            panic!("expected runnable invocation");
        };
        assert_eq!(invocation.timeout_seconds, Some(45));
        assert_eq!(invocation.max_turns, Some(7));

        assert_eq!(
            parse_args(&args(&["--timeout", "0", "x"])),
            Err(ParseError::InvalidTimeout("0".into()))
        );
        assert_eq!(
            parse_args(&args(&["--max-turns=nope", "x"])),
            Err(ParseError::InvalidMaxTurns("nope".into()))
        );
        assert_eq!(
            parse_args(&args(&["--timeout=604801", "x"])),
            Err(ParseError::InvalidTimeout("604801".into()))
        );
        assert_eq!(
            parse_args(&args(&["--max-turns=10001", "x"])),
            Err(ParseError::InvalidMaxTurns("10001".into()))
        );
    }

    #[test]
    fn rejects_session_ids_that_can_inject_logs_or_exhaust_maps() {
        assert!(matches!(
            parse_args(&args(&["--session", "bad\nid", "x"])),
            Err(ParseError::InvalidSession(_))
        ));
        assert!(matches!(
            parse_args(&args(&["--session", &"a".repeat(129), "x"])),
            Err(ParseError::InvalidSession(_))
        ));
    }
}
