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
        cmd.args(codex_cli_args(config, request, &cwd));
        // Resume carries no -C flag, so the child must start IN the workspace.
        cmd.current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd.envs(&config.env);

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

/// CLI arguments for `codex exec` / `codex exec resume`, excluding the
/// trailing prompt. Extracted so the fresh-vs-resume shape is testable:
/// `resume` has no `-C`/`-s` flags — the resumed session carries its own
/// cwd and sandbox, and passing `-C` makes the CLI exit with an argument
/// error (the empty-stream soft-warning bug this fixes).
fn codex_cli_args(
    config: &CodexAdapterConfig,
    request: &RuntimeRequest,
    cwd: &std::path::Path,
) -> Vec<String> {
    let mut args = vec!["exec".to_string()];
    if let Some(session) = &request.resume_session {
        args.push("resume".to_string());
        args.push(session.clone());
    }
    args.push("--json".to_string());
    args.push("--skip-git-repo-check".to_string());
    if request.resume_session.is_none() {
        args.push("-C".to_string());
        args.push(cwd.display().to_string());
        args.push("-s".to_string());
        args.push(config.sandbox.as_cli_str().to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use alva_engine_runtime::RuntimeOptions;

    fn config() -> CodexAdapterConfig {
        CodexAdapterConfig::default()
    }

    fn request(resume: Option<&str>) -> RuntimeRequest {
        RuntimeRequest {
            prompt: "do the thing".to_string(),
            resume_session: resume.map(str::to_string),
            system_prompt: None,
            working_directory: Some(PathBuf::from("/tmp/ws")),
            options: RuntimeOptions {
                streaming: false,
                max_turns: None,
                extra: HashMap::new(),
            },
        }
    }

    #[test]
    fn fresh_exec_keeps_cwd_and_sandbox_flags() {
        let args = codex_cli_args(&config(), &request(None), std::path::Path::new("/tmp/ws"));
        assert_eq!(args[0], "exec");
        assert!(args.contains(&"-C".to_string()));
        assert!(args.contains(&"/tmp/ws".to_string()));
        assert!(args.contains(&"-s".to_string()));
        assert!(!args.iter().any(|arg| arg == "resume"));
    }

    #[test]
    fn resume_drops_cwd_and_sandbox_flags_and_keeps_session_id() {
        let args = codex_cli_args(
            &config(),
            &request(Some("sess-123")),
            std::path::Path::new("/tmp/ws"),
        );
        assert_eq!(&args[0..3], &["exec", "resume", "sess-123"]);
        assert!(
            !args.contains(&"-C".to_string()),
            "resume has no -C flag: {args:?}"
        );
        assert!(
            !args.contains(&"-s".to_string()),
            "resume has no -s flag: {args:?}"
        );
        assert!(args.contains(&"--skip-git-repo-check".to_string()));
    }
}
