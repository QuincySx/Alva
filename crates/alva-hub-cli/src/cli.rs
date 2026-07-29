// INPUT:  process argument strings
// OUTPUT: Engine, ClaudeMode, Invocation, ParseOutcome, ParseError, parse_args
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

/// A validated one-shot Hub invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) engine: Engine,
    /// Only meaningful when `engine == Claude`; ignored otherwise.
    pub(crate) claude_mode: ClaudeMode,
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
                write!(
                    f,
                    "unknown claude mode '{mode}' (expected light or full)"
                )
            }
            Self::ClaudeModeWithoutClaudeEngine => {
                write!(f, "--claude-mode is only valid with --engine claude")
            }
            Self::UnknownOption(option) => write!(f, "unknown option '{option}'"),
            Self::EmptyPrompt => write!(f, "a prompt is required"),
        }
    }
}

/// Parse CLI arguments without inspecting environment or constructing engines.
pub(crate) fn parse_args(args: &[String]) -> Result<ParseOutcome, ParseError> {
    let mut engine = Engine::Alva;
    let mut engine_set = false;
    let mut claude_mode = ClaudeMode::Light;
    let mut claude_mode_set = false;
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
    if prompt.trim().is_empty() {
        return Err(ParseError::EmptyPrompt);
    }
    Ok(ParseOutcome::Run(Invocation {
        engine,
        claude_mode,
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
                claude_mode: ClaudeMode::Light,
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
                    claude_mode: ClaudeMode::Light,
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
                claude_mode: ClaudeMode::Light,
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
                claude_mode: ClaudeMode::Full,
                prompt: "hello".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--engine=claude", "--claude-mode=sdk", "hi"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                claude_mode: ClaudeMode::Full,
                prompt: "hi".into(),
            }))
        );
        assert_eq!(
            parse_args(&args(&["--engine=claude", "--claude-mode=print", "hi"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                claude_mode: ClaudeMode::Light,
                prompt: "hi".into(),
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
            parse_args(&args(&["--engine", "hello"])),
            Err(ParseError::UnknownEngine("hello".into()))
        );
        assert_eq!(
            parse_args(&args(&["--engine"])),
            Err(ParseError::MissingEngineValue)
        );
        assert_eq!(
            parse_args(&args(&["--engine="])),
            Err(ParseError::MissingEngineValue)
        );
        assert_eq!(
            parse_args(&args(&["--engine=gemini", "hello"])),
            Err(ParseError::UnknownEngine("gemini".into()))
        );
    }

    #[test]
    fn rejects_unknown_options_and_empty_prompts() {
        assert_eq!(
            parse_args(&args(&["--bogus", "hello"])),
            Err(ParseError::UnknownOption("--bogus".into()))
        );
        assert_eq!(parse_args(&[]), Err(ParseError::EmptyPrompt));
        assert_eq!(parse_args(&args(&[""])), Err(ParseError::EmptyPrompt));
        assert_eq!(
            parse_args(&args(&["--engine", "alva"])),
            Err(ParseError::EmptyPrompt)
        );
    }

    #[test]
    fn help_is_successful() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(ParseOutcome::Help));
        assert_eq!(parse_args(&args(&["-h"])), Ok(ParseOutcome::Help));
        assert_eq!(
            parse_args(&args(&["--engine", "claude", "--help"])),
            Ok(ParseOutcome::Help)
        );
    }

    #[test]
    fn double_dash_allows_prompt_parts_that_start_with_dash() {
        assert_eq!(
            parse_args(&args(&["--engine=claude", "--", "--explain", "-v"])),
            Ok(ParseOutcome::Run(Invocation {
                engine: Engine::Claude,
                claude_mode: ClaudeMode::Light,
                prompt: "--explain -v".into(),
            }))
        );
    }
}
