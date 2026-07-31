// INPUT:  tokio process I/O, CodexAdapterConfig, RuntimeRequest
// OUTPUT: CodexProcess (spawn + line reader + kill)
// POS:    Host for `codex exec --json` child process.

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tracing::warn;

use alva_engine_runtime::{RuntimeError, RuntimeRequest};

use crate::config::CodexAdapterConfig;

/// Running `codex exec --json` process with stdout JSONL lines.
pub(crate) struct CodexProcess {
    child: Child,
    stdout_lines: Lines<BufReader<ChildStdout>>,
}

impl CodexProcess {
    pub(crate) async fn spawn(
        config: &CodexAdapterConfig,
        request: &RuntimeRequest,
    ) -> Result<Self, RuntimeError> {
        let cwd = request
            .working_directory
            .clone()
            .or_else(|| config.default_cwd.clone())
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

        let mut cmd = Command::new(&config.codex_path);
        // Fresh: `codex exec --json … PROMPT`
        // Resume: `codex exec resume <SESSION_ID> --json … PROMPT`
        // (Codex CLI 0.14x+; session ids come from thread.started / Hub journal.)
        cmd.arg("exec");
        if let Some(session) = &request.resume_session {
            cmd.arg("resume").arg(session);
        }
        cmd.arg("--json")
            .arg("--skip-git-repo-check")
            .arg("-C")
            .arg(&cwd)
            .arg("-s")
            .arg(config.sandbox.as_cli_str())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(model) = &config.model {
            cmd.arg("-m").arg(model);
        }
        if config.bypass_approvals_and_sandbox {
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
        }
        for (key, value) in &config.config_overrides {
            cmd.arg("-c").arg(format!("{key}={value}"));
        }
        if let Some(system) = &request.system_prompt {
            // Codex has no first-class system_prompt flag on exec; pass as
            // a leading instruction block in the user prompt instead.
            let combined = format!("<system>\n{system}\n</system>\n\n{}", request.prompt);
            cmd.arg(combined);
        } else {
            cmd.arg(&request.prompt);
        }

        let mut child = cmd.spawn().map_err(|error| {
            RuntimeError::ProcessError(format!(
                "Failed to spawn Codex CLI ({}): {error}",
                config.codex_path
            ))
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::ProcessError("Failed to capture Codex stdout".into()))?;
        monitor_stderr(child.stderr.take());

        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
        })
    }

    pub(crate) async fn next_line(&mut self) -> Result<Option<String>, RuntimeError> {
        self.stdout_lines.next_line().await.map_err(|error| {
            RuntimeError::ProcessError(format!("Codex stdout read failed: {error}"))
        })
    }

    pub(crate) async fn kill(&mut self) -> Result<(), RuntimeError> {
        match self.child.start_kill() {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(RuntimeError::ProcessError(format!(
                "Failed to kill Codex process: {error}"
            ))),
        }
    }
}

fn monitor_stderr(stderr: Option<tokio::process::ChildStderr>) {
    let Some(stderr) = stderr else {
        return;
    };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                warn!(target: "codex-adapter", "{line}");
            }
        }
    });
}
