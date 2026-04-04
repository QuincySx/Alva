//! Hook execution engine — runs shell scripts at key lifecycle points.
//!
//! Hooks are configured in settings.json under the `hooks` key and executed
//! as shell subprocesses with structured JSON input on stdin.
//!
//! ## Exit code semantics
//!
//! | Code | Meaning                                                     |
//! |------|-------------------------------------------------------------|
//! | 0    | Success — JSON output parsed, operation continues           |
//! | 2    | Blocking — stderr fed back to the model, operation blocked  |
//! | other| Non-blocking error — stderr shown to user, operation continues|

mod executor;
mod matcher;

pub use executor::HookExecutor;
pub use matcher::matches_hook;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::settings::{HookConfig, HookEntry, HooksSettings};

// ---------------------------------------------------------------------------
// Hook events
// ---------------------------------------------------------------------------

/// Events at which hooks can fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    SessionStart,
    SessionEnd,
    Stop,
    UserPromptSubmit,
    Notification,
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreToolUse => write!(f, "PreToolUse"),
            Self::PostToolUse => write!(f, "PostToolUse"),
            Self::PostToolUseFailure => write!(f, "PostToolUseFailure"),
            Self::SessionStart => write!(f, "SessionStart"),
            Self::SessionEnd => write!(f, "SessionEnd"),
            Self::Stop => write!(f, "Stop"),
            Self::UserPromptSubmit => write!(f, "UserPromptSubmit"),
            Self::Notification => write!(f, "Notification"),
        }
    }
}

// ---------------------------------------------------------------------------
// Hook input (JSON on stdin to the hook subprocess)
// ---------------------------------------------------------------------------

/// Structured input passed to hook scripts via stdin as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// Which event triggered this hook.
    pub hook_event: String,
    /// Tool name (for tool-related events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Tool input arguments (for tool-related events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    /// Tool output / response (PostToolUse only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<String>,
    /// Error message (PostToolUseFailure only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Session ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Current working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

// ---------------------------------------------------------------------------
// Hook output (JSON parsed from stdout of the hook subprocess)
// ---------------------------------------------------------------------------

/// Structured JSON output a hook can produce on stdout.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HookOutput {
    /// Whether the operation should continue (default true).
    #[serde(rename = "continue")]
    pub should_continue: Option<bool>,
    /// Reason for stopping (when `continue` is false).
    pub stop_reason: Option<String>,
    /// Additional context to feed to the model.
    pub additional_context: Option<String>,
    /// Permission decision override (PreToolUse only).
    pub permission_decision: Option<String>,
    /// Modified tool input (PreToolUse only).
    pub updated_input: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Hook result (returned to callers after execution)
// ---------------------------------------------------------------------------

/// Outcome of running a single hook.
#[derive(Debug, Clone)]
pub enum HookOutcome {
    /// Hook succeeded (exit 0). May contain parsed JSON output.
    Success {
        stdout: String,
        output: Option<HookOutput>,
    },
    /// Hook produced a blocking error (exit 2). Stderr should be fed to the model.
    Blocked { stderr: String },
    /// Hook produced a non-blocking error. Stderr is shown to the user.
    NonBlockingError {
        exit_code: i32,
        stderr: String,
    },
    /// Hook timed out.
    Timeout,
    /// Hook could not be executed (e.g., command not found).
    ExecError { message: String },
}

impl HookOutcome {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

/// Aggregated result of all hooks for a single event.
#[derive(Debug, Clone)]
pub struct HookResult {
    pub event: HookEvent,
    pub outcomes: Vec<HookOutcome>,
}

impl HookResult {
    /// True if any hook blocked the operation.
    pub fn is_blocked(&self) -> bool {
        self.outcomes.iter().any(|o| o.is_blocked())
    }

    /// Collect all blocking stderr messages.
    pub fn blocking_messages(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter_map(|o| match o {
                HookOutcome::Blocked { stderr } => Some(stderr.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Collect additional context from successful hooks.
    pub fn additional_context(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter_map(|o| match o {
                HookOutcome::Success { output: Some(out), .. } => {
                    out.additional_context.as_deref()
                }
                _ => None,
            })
            .collect()
    }

    /// Get the first updated_input from successful PreToolUse hooks.
    pub fn updated_input(&self) -> Option<&serde_json::Value> {
        self.outcomes.iter().find_map(|o| match o {
            HookOutcome::Success { output: Some(out), .. } => out.updated_input.as_ref(),
            _ => None,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers to extract hooks from HooksSettings
// ---------------------------------------------------------------------------

impl HooksSettings {
    /// Get the hook configs for a given event.
    pub fn configs_for(&self, event: HookEvent) -> &[HookConfig] {
        match event {
            HookEvent::PreToolUse => &self.pre_tool_use,
            HookEvent::PostToolUse => &self.post_tool_use,
            // PostToolUseFailure currently reuses PostToolUse hooks.
            // TODO: add a dedicated `post_tool_use_failure` field to HooksSettings
            // when the settings schema is extended to support it.
            HookEvent::PostToolUseFailure => &self.post_tool_use,
            HookEvent::SessionStart => &self.session_start,
            HookEvent::SessionEnd => &self.session_end,
            HookEvent::Notification => &self.notification,
            // Events without dedicated config: Stop, UserPromptSubmit
            // These can be added to settings later. For now, empty.
            HookEvent::Stop | HookEvent::UserPromptSubmit => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_event_display() {
        assert_eq!(HookEvent::PreToolUse.to_string(), "PreToolUse");
        assert_eq!(HookEvent::SessionStart.to_string(), "SessionStart");
    }

    #[test]
    fn hook_result_blocked() {
        let result = HookResult {
            event: HookEvent::PreToolUse,
            outcomes: vec![
                HookOutcome::Success {
                    stdout: String::new(),
                    output: None,
                },
                HookOutcome::Blocked {
                    stderr: "dangerous command".to_string(),
                },
            ],
        };
        assert!(result.is_blocked());
        assert_eq!(result.blocking_messages(), vec!["dangerous command"]);
    }

    #[test]
    fn hook_result_not_blocked() {
        let result = HookResult {
            event: HookEvent::PostToolUse,
            outcomes: vec![HookOutcome::Success {
                stdout: "ok".to_string(),
                output: None,
            }],
        };
        assert!(!result.is_blocked());
    }

    #[test]
    fn hook_result_additional_context() {
        let result = HookResult {
            event: HookEvent::PreToolUse,
            outcomes: vec![HookOutcome::Success {
                stdout: String::new(),
                output: Some(HookOutput {
                    additional_context: Some("lint passed".to_string()),
                    ..Default::default()
                }),
            }],
        };
        assert_eq!(result.additional_context(), vec!["lint passed"]);
    }

    #[test]
    fn hook_input_serializes() {
        let input = HookInput {
            hook_event: "PreToolUse".to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input: Some(serde_json::json!({"command": "rm -rf /"})),
            tool_response: None,
            error: None,
            session_id: Some("abc".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("rm -rf"));
        // None fields should be absent
        assert!(!json.contains("tool_response"));
    }

    #[test]
    fn hook_output_deserializes_defaults() {
        let json = r#"{}"#;
        let output: HookOutput = serde_json::from_str(json).unwrap();
        assert!(output.should_continue.is_none());
        assert!(output.additional_context.is_none());
    }

    #[test]
    fn hook_output_deserializes_full() {
        let json = r#"{"continue": false, "stop_reason": "blocked", "additional_context": "extra info"}"#;
        let output: HookOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.should_continue, Some(false));
        assert_eq!(output.stop_reason.as_deref(), Some("blocked"));
        assert_eq!(output.additional_context.as_deref(), Some("extra info"));
    }
}
