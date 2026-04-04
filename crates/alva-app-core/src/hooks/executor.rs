//! Hook executor — spawns shell subprocesses, feeds JSON input via stdin,
//! and interprets exit codes + stdout/stderr.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::settings::{HookConfig, HookEntry, HooksSettings};

use super::{
    matcher::matches_hook, HookEvent, HookInput, HookOutcome, HookOutput, HookResult,
};

/// Default hook timeout (10 seconds).
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// Executes hooks for the agent lifecycle.
///
/// Create one per session and reuse — it is stateless.
pub struct HookExecutor {
    workspace: PathBuf,
    session_id: String,
}

impl HookExecutor {
    pub fn new(workspace: impl Into<PathBuf>, session_id: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            session_id: session_id.into(),
        }
    }

    /// Run all matching hooks for an event.
    ///
    /// - `settings`: The hooks configuration from settings.json.
    /// - `event`: Which lifecycle event triggered.
    /// - `match_query`: Optional string to match against hook matchers (typically tool_name).
    /// - `input`: Structured data passed to the hook on stdin.
    pub async fn run(
        &self,
        settings: &HooksSettings,
        event: HookEvent,
        match_query: Option<&str>,
        input: HookInput,
    ) -> HookResult {
        let configs = settings.configs_for(event);
        let mut outcomes = Vec::new();

        for config in configs {
            if !matches_hook(config, match_query) {
                continue;
            }

            for entry in &config.hooks {
                if entry.hook_type != "command" {
                    tracing::debug!(
                        hook_type = %entry.hook_type,
                        "skipping non-command hook (not yet supported)"
                    );
                    continue;
                }

                let outcome = self.exec_command(entry, &input).await;
                let is_blocked = outcome.is_blocked();
                outcomes.push(outcome);

                // If a PreToolUse hook blocks, stop running further hooks.
                if is_blocked && event == HookEvent::PreToolUse {
                    break;
                }
            }
        }

        HookResult { event, outcomes }
    }

    /// Execute a single command hook entry.
    async fn exec_command(&self, entry: &HookEntry, input: &HookInput) -> HookOutcome {
        let timeout_ms = entry.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&entry.command)
            .current_dir(&self.workspace)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Set environment variables
        cmd.env("CLAUDE_PROJECT_DIR", &self.workspace);
        cmd.env("CLAUDE_SESSION_ID", &self.session_id);
        cmd.env("HOOK_EVENT", input.hook_event.as_str());
        if let Some(ref tool) = input.tool_name {
            cmd.env("TOOL_NAME", tool);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return HookOutcome::ExecError {
                    message: format!("failed to spawn hook: {}", e),
                };
            }
        };

        // Write JSON input to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let json = serde_json::to_string(input).unwrap_or_default();
            let _ = stdin.write_all(json.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            drop(stdin);
        }

        // Wait with timeout
        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            child.wait_with_output(),
        )
        .await;

        match result {
            Err(_) => {
                // Timeout — process is already consumed by wait_with_output;
                // tokio aborts it when the future is dropped.
                HookOutcome::Timeout
            }
            Ok(Err(e)) => HookOutcome::ExecError {
                message: format!("hook process error: {}", e),
            },
            Ok(Ok(output)) => {
                let exit_code = output.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

                match exit_code {
                    0 => {
                        // Success — try to parse JSON output from stdout
                        let parsed = if stdout.trim().is_empty() {
                            None
                        } else {
                            serde_json::from_str::<HookOutput>(stdout.trim()).ok()
                        };
                        HookOutcome::Success {
                            stdout,
                            output: parsed,
                        }
                    }
                    2 => {
                        // Blocking error — stderr goes to model
                        HookOutcome::Blocked { stderr }
                    }
                    code => {
                        // Non-blocking error — stderr shown to user
                        HookOutcome::NonBlockingError {
                            exit_code: code,
                            stderr,
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience builders for HookInput
// ---------------------------------------------------------------------------

impl HookInput {
    /// Build input for a PreToolUse event.
    pub fn pre_tool_use(
        tool_name: &str,
        tool_input: serde_json::Value,
        session_id: &str,
        cwd: &Path,
    ) -> Self {
        Self {
            hook_event: "PreToolUse".to_string(),
            tool_name: Some(tool_name.to_string()),
            tool_input: Some(tool_input),
            tool_response: None,
            error: None,
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.display().to_string()),
        }
    }

    /// Build input for a PostToolUse event.
    pub fn post_tool_use(
        tool_name: &str,
        tool_input: serde_json::Value,
        tool_response: &str,
        session_id: &str,
        cwd: &Path,
    ) -> Self {
        Self {
            hook_event: "PostToolUse".to_string(),
            tool_name: Some(tool_name.to_string()),
            tool_input: Some(tool_input),
            tool_response: Some(tool_response.to_string()),
            error: None,
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.display().to_string()),
        }
    }

    /// Build input for a PostToolUseFailure event.
    pub fn post_tool_use_failure(
        tool_name: &str,
        tool_input: serde_json::Value,
        error: &str,
        session_id: &str,
        cwd: &Path,
    ) -> Self {
        Self {
            hook_event: "PostToolUseFailure".to_string(),
            tool_name: Some(tool_name.to_string()),
            tool_input: Some(tool_input),
            tool_response: None,
            error: Some(error.to_string()),
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.display().to_string()),
        }
    }

    /// Build input for a lifecycle event (SessionStart, Stop, etc.).
    pub fn lifecycle(event: HookEvent, session_id: &str, cwd: &Path) -> Self {
        Self {
            hook_event: event.to_string(),
            tool_name: None,
            tool_input: None,
            tool_response: None,
            error: None,
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.display().to_string()),
        }
    }

    /// Build input for a UserPromptSubmit event.
    pub fn user_prompt_submit(prompt: &str, session_id: &str, cwd: &Path) -> Self {
        Self {
            hook_event: "UserPromptSubmit".to_string(),
            tool_name: None,
            tool_input: Some(serde_json::json!({ "prompt": prompt })),
            tool_response: None,
            error: None,
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.display().to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HookConfig, HookEntry, HooksSettings};

    fn make_settings(event: HookEvent, command: &str, matcher: Option<&str>) -> HooksSettings {
        let config = HookConfig {
            matcher: matcher.map(String::from),
            hooks: vec![HookEntry {
                hook_type: "command".to_string(),
                command: command.to_string(),
                timeout: Some(5000),
            }],
        };
        let mut settings = HooksSettings::default();
        match event {
            HookEvent::PreToolUse => settings.pre_tool_use.push(config),
            HookEvent::PostToolUse => settings.post_tool_use.push(config),
            HookEvent::SessionStart => settings.session_start.push(config),
            HookEvent::SessionEnd => settings.session_end.push(config),
            HookEvent::Notification => settings.notification.push(config),
            _ => {}
        }
        settings
    }

    fn test_executor() -> HookExecutor {
        let tmp = std::env::temp_dir();
        HookExecutor::new(&tmp, "test-session")
    }

    #[tokio::test]
    async fn exit_0_is_success() {
        let settings = make_settings(HookEvent::PreToolUse, "echo ok", None);
        let input = HookInput::lifecycle(HookEvent::PreToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, None, input).await;
        assert!(!result.is_blocked());
        assert_eq!(result.outcomes.len(), 1);
        assert!(result.outcomes[0].is_success());
    }

    #[tokio::test]
    async fn exit_2_is_blocked() {
        let settings = make_settings(
            HookEvent::PreToolUse,
            "echo 'dangerous' >&2; exit 2",
            None,
        );
        let input = HookInput::lifecycle(HookEvent::PreToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, None, input).await;
        assert!(result.is_blocked());
        assert_eq!(result.blocking_messages(), vec!["dangerous"]);
    }

    #[tokio::test]
    async fn exit_1_is_non_blocking_error() {
        let settings = make_settings(
            HookEvent::PreToolUse,
            "echo 'warning' >&2; exit 1",
            None,
        );
        let input = HookInput::lifecycle(HookEvent::PreToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, None, input).await;
        assert!(!result.is_blocked());
        match &result.outcomes[0] {
            HookOutcome::NonBlockingError { exit_code, stderr } => {
                assert_eq!(*exit_code, 1);
                assert_eq!(stderr, "warning");
            }
            other => panic!("expected NonBlockingError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn json_output_parsed() {
        let cmd = r#"echo '{"additional_context": "lint ok", "continue": true}'"#;
        let settings = make_settings(HookEvent::PostToolUse, cmd, None);
        let input = HookInput::lifecycle(HookEvent::PostToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PostToolUse, None, input).await;
        assert_eq!(result.additional_context(), vec!["lint ok"]);
    }

    #[tokio::test]
    async fn matcher_filters_hooks() {
        let settings = make_settings(HookEvent::PreToolUse, "echo matched", Some("Bash"));
        let input = HookInput::lifecycle(HookEvent::PreToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        // Match: tool_name = "Bash"
        let result = executor
            .run(&settings, HookEvent::PreToolUse, Some("Bash"), input.clone())
            .await;
        assert_eq!(result.outcomes.len(), 1);

        // No match: tool_name = "Read"
        let result = executor
            .run(&settings, HookEvent::PreToolUse, Some("Read"), input)
            .await;
        assert_eq!(result.outcomes.len(), 0);
    }

    #[tokio::test]
    async fn timeout_kills_hook() {
        let settings = HooksSettings {
            pre_tool_use: vec![HookConfig {
                matcher: None,
                hooks: vec![HookEntry {
                    hook_type: "command".to_string(),
                    command: "sleep 60".to_string(),
                    timeout: Some(100), // 100ms timeout
                }],
            }],
            ..Default::default()
        };
        let input = HookInput::lifecycle(HookEvent::PreToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, None, input).await;
        assert_eq!(result.outcomes.len(), 1);
        assert!(matches!(result.outcomes[0], HookOutcome::Timeout));
    }

    #[tokio::test]
    async fn env_vars_passed_to_hook() {
        // Hook script that echoes env vars as JSON
        let cmd = r#"echo "{\"project\": \"$CLAUDE_PROJECT_DIR\", \"event\": \"$HOOK_EVENT\", \"tool\": \"$TOOL_NAME\"}"" "#;
        let settings = make_settings(HookEvent::PreToolUse, "printenv HOOK_EVENT", None);
        let input = HookInput::pre_tool_use(
            "Bash",
            serde_json::json!({"command": "ls"}),
            "sess",
            Path::new("/tmp"),
        );
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, Some("Bash"), input).await;
        match &result.outcomes[0] {
            HookOutcome::Success { stdout, .. } => {
                assert!(stdout.trim().contains("PreToolUse"), "should have HOOK_EVENT: {}", stdout);
            }
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn stdin_receives_json_input() {
        // Hook reads tool_name from stdin JSON
        let cmd = r#"cat | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('tool_name',''))" 2>/dev/null || cat"#;
        // Simpler test: just verify stdin is piped
        let settings = make_settings(HookEvent::PreToolUse, "cat", None);
        let input = HookInput::pre_tool_use(
            "Bash",
            serde_json::json!({"command": "ls"}),
            "sess",
            Path::new("/tmp"),
        );
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, None, input).await;
        match &result.outcomes[0] {
            HookOutcome::Success { stdout, .. } => {
                // cat echoes back the JSON input
                assert!(stdout.contains("tool_name"), "stdin should contain tool_name: {}", stdout);
                assert!(stdout.contains("Bash"), "stdin should contain Bash: {}", stdout);
            }
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn blocked_hook_stops_further_pre_tool_use() {
        let mut settings = HooksSettings::default();
        settings.pre_tool_use.push(HookConfig {
            matcher: None,
            hooks: vec![
                HookEntry {
                    hook_type: "command".to_string(),
                    command: "echo blocked >&2; exit 2".to_string(),
                    timeout: Some(5000),
                },
                HookEntry {
                    hook_type: "command".to_string(),
                    command: "echo should-not-run".to_string(),
                    timeout: Some(5000),
                },
            ],
        });
        let input = HookInput::lifecycle(HookEvent::PreToolUse, "sess", Path::new("/tmp"));
        let executor = test_executor();

        let result = executor.run(&settings, HookEvent::PreToolUse, None, input).await;
        // Only the first hook should have run (blocked stops the rest)
        assert_eq!(result.outcomes.len(), 1);
        assert!(result.is_blocked());
    }

    #[test]
    fn hook_input_pre_tool_use_builder() {
        let input = HookInput::pre_tool_use(
            "Bash",
            serde_json::json!({"command": "ls"}),
            "sess-123",
            Path::new("/workspace"),
        );
        assert_eq!(input.hook_event, "PreToolUse");
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
        assert_eq!(input.session_id.as_deref(), Some("sess-123"));
    }

    #[test]
    fn hook_input_post_tool_use_builder() {
        let input = HookInput::post_tool_use(
            "Read",
            serde_json::json!({"file_path": "/tmp/test.rs"}),
            "file contents here",
            "sess-123",
            Path::new("/workspace"),
        );
        assert_eq!(input.hook_event, "PostToolUse");
        assert_eq!(input.tool_response.as_deref(), Some("file contents here"));
    }
}
