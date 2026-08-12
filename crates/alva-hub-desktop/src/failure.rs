// INPUT:  terminal failure messages from engine start/stream paths
// OUTPUT: stable FailureClass matrix and the fallback-eligibility rule
// POS:    Single owner of Hub V1 operational-continuity failure classification.

use serde::Serialize;

/// Stable classification of terminal engine failures. Categories are the
/// product vocabulary (timeline cards, exhaustion summaries) — messages feed
/// in, classes come out, and the mapping only grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    EngineMissing,
    EngineIncompatible,
    Auth,
    ModelUnavailable,
    RateLimit,
    Context,
    Crash,
    Transport,
    Stall,
    Other,
}

impl FailureClass {
    pub fn label(&self) -> &'static str {
        match self {
            FailureClass::EngineMissing => "engine missing",
            FailureClass::EngineIncompatible => "engine incompatible",
            FailureClass::Auth => "authentication",
            FailureClass::ModelUnavailable => "model unavailable",
            FailureClass::RateLimit => "rate/quota/billing",
            FailureClass::Context => "context overflow",
            FailureClass::Crash => "engine crash",
            FailureClass::Transport => "transport failure",
            FailureClass::Stall => "stall (no progress)",
            FailureClass::Other => "unclassified failure",
        }
    }
}

/// Only classified operational continuity failures may consume another
/// authorized engine's account. Unclassified errors stop the chain and are
/// shown to the user instead.
pub fn triggers_fallback(class: FailureClass) -> bool {
    class != FailureClass::Other
}

/// Message-based classification. Adapters surface terminal failures as text,
/// so the matrix matches indicative phrases; ordering is most-specific-first
/// and every arm is pinned by tests.
pub fn classify_failure(message: &str) -> FailureClass {
    let message = message.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| message.contains(needle));

    if has(&[
        "api key",
        "authentication",
        "unauthorized",
        "not signed in",
        "not logged in",
        "credential",
        "invalid_api_key",
        "please run /login",
    ]) || has_http_code(&message, "401")
        || has_http_code(&message, "403")
    {
        return FailureClass::Auth;
    }
    if has(&[
        "rate limit",
        "rate_limit",
        "quota",
        "billing",
        "insufficient credit",
        "payment required",
        "overloaded",
    ]) || has_http_code(&message, "429")
    {
        return FailureClass::RateLimit;
    }
    if has(&[
        "context length",
        "context window",
        "maximum context",
        "too many tokens",
        "prompt is too long",
        "input is too long",
        "context_length_exceeded",
    ]) {
        return FailureClass::Context;
    }
    if has(&[
        "model not found",
        "model_not_found",
        "unknown model",
        "unsupported model",
        "invalid model",
        "no such model",
        "model is not available",
        "model unavailable",
    ]) {
        return FailureClass::ModelUnavailable;
    }
    if has(&[
        "no such file",
        "enoent",
        "command not found",
        "not installed",
        "executable not found",
    ]) {
        return FailureClass::EngineMissing;
    }
    if has(&[
        "incompatible",
        "unsupported version",
        "unsupported protocol",
        "protocol error",
        "version mismatch",
    ]) {
        return FailureClass::EngineIncompatible;
    }
    if has(&[
        "failed to spawn",
        "process error",
        "process exited",
        "exited with",
        "killed",
        "panicked",
        "crashed",
        "core dumped",
        "engine process died",
    ]) {
        return FailureClass::Crash;
    }
    if has(&[
        "stalled",
        "no progress",
        "no activity",
        "not responding",
        "watchdog",
        "hung",
    ]) {
        return FailureClass::Stall;
    }
    if has(&[
        "connection",
        "econnrefused",
        "econnreset",
        "broken pipe",
        "timed out",
        "timeout",
        "stream ended",
        "stream closed",
        "stream died",
        "network",
        "dns",
        "socket",
    ]) {
        return FailureClass::Transport;
    }
    FailureClass::Other
}

/// A numeric HTTP status only counts when it stands alone — ports, byte
/// counts, and ids ("127.0.0.1:4013") must not classify as that status.
fn has_http_code(message: &str, code: &str) -> bool {
    message.match_indices(code).any(|(index, _)| {
        let before_is_digit = message[..index]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_digit());
        let after_is_digit = message[index + code.len()..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit());
        !before_is_digit && !after_is_digit
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_covers_all_v1_operational_failure_categories() {
        let cases = [
            (
                "Failed to spawn Claude CLI (claude): No such file or directory",
                FailureClass::EngineMissing,
            ),
            ("codex: command not found", FailureClass::EngineMissing),
            (
                "engine speaks an unsupported protocol version",
                FailureClass::EngineIncompatible,
            ),
            (
                "CLI version incompatible with Hub",
                FailureClass::EngineIncompatible,
            ),
            ("no Alva provider API key configured", FailureClass::Auth),
            (
                "authentication_error: invalid credentials",
                FailureClass::Auth,
            ),
            ("401 Unauthorized", FailureClass::Auth),
            ("model not found: gpt-x", FailureClass::ModelUnavailable),
            (
                "The requested model is not available",
                FailureClass::ModelUnavailable,
            ),
            ("rate limit exceeded, retry later", FailureClass::RateLimit),
            (
                "quota_exceeded: monthly cap reached",
                FailureClass::RateLimit,
            ),
            ("HTTP 429 Too Many Requests", FailureClass::RateLimit),
            ("billing hard limit reached", FailureClass::RateLimit),
            (
                "prompt exceeds maximum context window",
                FailureClass::Context,
            ),
            ("context_length_exceeded", FailureClass::Context),
            ("engine process died unexpectedly", FailureClass::Crash),
            ("child exited with signal 9 (killed)", FailureClass::Crash),
            ("connection refused (ECONNREFUSED)", FailureClass::Transport),
            (
                "stream ended without terminal Completed event",
                FailureClass::Transport,
            ),
            ("request timed out after 60s", FailureClass::Transport),
            (
                "the user asked for a purple background",
                FailureClass::Other,
            ),
            // Digit runs that merely contain a status code must not match it.
            (
                "connect ECONNREFUSED 127.0.0.1:4013",
                FailureClass::Transport,
            ),
            ("permission denied (401)", FailureClass::Auth),
        ];
        for (message, expected) in cases {
            assert_eq!(
                classify_failure(message),
                expected,
                "message {message:?} misclassified"
            );
        }
    }

    #[test]
    fn most_specific_category_wins_over_generic_phrases() {
        // Spawn failure wording plus a missing binary → missing, not crash.
        assert_eq!(
            classify_failure("Failed to spawn Codex CLI (codex): No such file or directory"),
            FailureClass::EngineMissing,
        );
        // Auth wording inside a connection error → auth wins (account issue,
        // not infrastructure).
        assert_eq!(
            classify_failure("connection rejected: invalid api key"),
            FailureClass::Auth,
        );
    }

    #[test]
    fn only_unclassified_failures_are_excluded_from_fallback() {
        for class in [
            FailureClass::EngineMissing,
            FailureClass::EngineIncompatible,
            FailureClass::Auth,
            FailureClass::ModelUnavailable,
            FailureClass::RateLimit,
            FailureClass::Context,
            FailureClass::Crash,
            FailureClass::Transport,
            FailureClass::Stall,
        ] {
            assert!(triggers_fallback(class), "{class:?} must trigger fallback");
        }
        assert!(!triggers_fallback(FailureClass::Other));
    }

    #[test]
    fn stall_messages_classify_stably() {
        assert_eq!(
            classify_failure("engine stalled: no activity for 180s"),
            FailureClass::Stall,
        );
        // Real-CLI phrasing variants across engines.
        assert_eq!(
            classify_failure("claude: no activity detected; process may be hung"),
            FailureClass::Stall,
        );
        assert_eq!(
            classify_failure("codex exec made no progress and is not responding"),
            FailureClass::Stall,
        );
    }
}
