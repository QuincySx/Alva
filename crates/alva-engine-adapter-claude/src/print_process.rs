// INPUT:  ClaudePrintConfig, RuntimeRequest, tokio process I/O
// OUTPUT: ClaudePrintProcess (spawn + JSONL read + kill)
// POS:    Host for `claude -p --output-format stream-json` (light / exec-style mode).

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tracing::{debug, warn};

use alva_engine_runtime::{RuntimeError, RuntimeRequest};

use crate::print_config::ClaudePrintConfig;
use crate::protocol::SdkMessage;

/// Running `claude -p` process with stdout JSONL lines.
pub(crate) struct ClaudePrintProcess {
    child: Child,
    stdout_lines: Lines<BufReader<ChildStdout>>,
}

impl ClaudePrintProcess {
    pub(crate) async fn spawn(
        config: &ClaudePrintConfig,
        request: &RuntimeRequest,
    ) -> Result<Self, RuntimeError> {
        let cwd = request
            .working_directory
            .clone()
            .or_else(|| config.default_cwd.clone())
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

        let mut cmd = Command::new(&config.claude_path);
        // Light mode = Codex-exec analogue: non-interactive print + JSONL stream.
        cmd.arg("-p")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose") // required by some CLI builds for stream-json partials
            .arg("--include-partial-messages")
            .arg("--permission-mode")
            .arg(config.permission_mode.as_cli_str())
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Skip SessionStart hooks / CLAUDE.md auto-discovery noise for Hub one-shots.
        if config.bare {
            cmd.arg("--bare");
        }

        let model_override = config.model.clone().or_else(|| {
            request
                .options
                .extra
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
        if let Some(model) = model_override {
            cmd.arg("--model").arg(model);
        }

        if let Some(system) = &request.system_prompt {
            if !system.is_empty() {
                cmd.arg("--append-system-prompt").arg(system);
            }
        }

        if let Some(max_turns) = request.options.max_turns {
            cmd.arg("--max-turns").arg(max_turns.to_string());
        }

        if let Some(session) = &request.resume_session {
            cmd.arg("--resume").arg(session);
        }

        // Prompt last (positional).
        cmd.arg(&request.prompt);

        let mut child = cmd.spawn().map_err(|error| {
            RuntimeError::ProcessError(format!(
                "Failed to spawn Claude CLI ({}): {error}",
                config.claude_path
            ))
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::ProcessError("Failed to capture Claude CLI stdout".into()))?;
        monitor_stderr(child.stderr.take());

        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
        })
    }

    pub(crate) async fn next_line(&mut self) -> Result<Option<String>, RuntimeError> {
        match self.stdout_lines.next_line().await? {
            Some(line) => Ok(Some(line)),
            None => Ok(None),
        }
    }

    pub(crate) async fn kill(&mut self) -> Result<(), RuntimeError> {
        self.child.kill().await.map_err(|error| {
            RuntimeError::ProcessError(format!("Failed to kill Claude CLI process: {error}"))
        })
    }
}

/// Parse one stdout line as an SDK-shaped message (print mode wire = SDK wire).
pub(crate) fn parse_print_line(line: &str) -> Result<Option<SdkMessage>, RuntimeError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<SdkMessage>(line) {
        Ok(msg) => Ok(Some(msg)),
        Err(error) => {
            // Forward-compat: unknown top-level shapes are dropped (cindy style).
            debug!(%error, %line, "skipping unparseable claude -p JSONL line");
            Ok(None)
        }
    }
}

fn monitor_stderr(stderr: Option<tokio::process::ChildStderr>) {
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    warn!(target: "claude_print", "stderr: {line}");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_system_init_line() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s1","model":"m","tools":["Read"]}"#;
        let msg = parse_print_line(line).unwrap().unwrap();
        assert!(matches!(
            msg,
            SdkMessage::System {
                subtype: Some(ref s),
                session_id: Some(ref id),
                ..
            } if s == "init" && id == "s1"
        ));
    }

    #[test]
    fn parse_unknown_type_maps_to_unknown_variant() {
        let line = r#"{"type":"rate_limit_event","info":{}}"#;
        // SdkMessage::Unknown via #[serde(other)] — forward-compat drop later in mapper.
        let msg = parse_print_line(line).unwrap().unwrap();
        assert!(matches!(msg, SdkMessage::Unknown));
    }

    #[test]
    fn empty_line_skipped() {
        assert!(parse_print_line("   ").unwrap().is_none());
    }
}
