// INPUT:  tokio process/fs I/O, PiAdapterConfig, RuntimeRequest
// OUTPUT: PiProcess and testable Pi argv generation
// POS:    Host for Pi print-mode JSON child processes and ephemeral Provider config.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tracing::warn;

use alva_engine_runtime::{RuntimeError, RuntimeRequest};

use crate::config::{PiAdapterConfig, PiProviderConfig, PiToolPolicy};

pub(crate) struct PiProcess {
    child: Child,
    stdout_lines: Lines<BufReader<ChildStdout>>,
    _provider_dir: Option<tempfile::TempDir>,
}

impl PiProcess {
    pub(crate) async fn spawn(
        config: &PiAdapterConfig,
        request: &RuntimeRequest,
        session_id: &str,
    ) -> Result<Self, RuntimeError> {
        let cwd = request
            .working_directory
            .clone()
            .or_else(|| config.default_cwd.clone())
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
        tokio::fs::create_dir_all(&config.session_dir)
            .await
            .map_err(|error| RuntimeError::ProcessError(format!("Pi session dir: {error}")))?;

        let (provider_dir, provider_env) = prepare_provider_dir(config.custom_provider.as_ref())?;
        let mut command = Command::new(&config.pi_path);
        command.args(pi_cli_args(config, request, session_id));
        command
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .envs(&config.env);
        if let Some((agent_dir, api_key)) = provider_env {
            command.env("PI_CODING_AGENT_DIR", agent_dir);
            command.env("ALVA_HUB_PI_API_KEY", api_key);
        }
        let prompt = if let Some(system) = &request.system_prompt {
            format!("<system>\n{system}\n</system>\n\n{}", request.prompt)
        } else {
            request.prompt.clone()
        };
        command.arg(prompt);

        let mut child = command.spawn().map_err(|error| {
            RuntimeError::ProcessError(format!(
                "Failed to spawn Pi CLI ({}): {error}",
                config.pi_path
            ))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::ProcessError("Failed to capture Pi stdout".into()))?;
        monitor_stderr(child.stderr.take());
        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
            _provider_dir: provider_dir,
        })
    }

    pub(crate) async fn next_line(&mut self) -> Result<Option<String>, RuntimeError> {
        self.stdout_lines
            .next_line()
            .await
            .map_err(|error| RuntimeError::ProcessError(format!("Pi stdout read failed: {error}")))
    }

    pub(crate) async fn kill(&mut self) -> Result<(), RuntimeError> {
        match self.child.start_kill() {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(RuntimeError::ProcessError(format!(
                "Failed to kill Pi process: {error}"
            ))),
        }
    }
}

fn pi_cli_args(
    config: &PiAdapterConfig,
    _request: &RuntimeRequest,
    session_id: &str,
) -> Vec<String> {
    let mut args = vec![
        "--print".into(),
        "--mode".into(),
        "json".into(),
        "--session-id".into(),
        session_id.into(),
        "--session-dir".into(),
        config.session_dir.display().to_string(),
    ];
    if let Some(provider) = &config.provider {
        args.extend(["--provider".into(), provider.clone()]);
    }
    if let Some(model) = &config.model {
        args.extend(["--model".into(), model.clone()]);
    }
    if let Some(thinking) = &config.thinking {
        args.extend(["--thinking".into(), thinking.clone()]);
    }
    if config.tool_policy == PiToolPolicy::ReadOnly {
        args.extend(["--tools".into(), "read,grep,find,ls".into()]);
    }
    args
}

fn prepare_provider_dir(
    provider: Option<&PiProviderConfig>,
) -> Result<(Option<tempfile::TempDir>, Option<(String, String)>), RuntimeError> {
    let Some(provider) = provider else {
        return Ok((None, None));
    };
    let dir = tempfile::Builder::new()
        .prefix("alva-hub-pi-")
        .tempdir()
        .map_err(|error| RuntimeError::ProcessError(format!("Pi provider dir: {error}")))?;
    std::fs::set_permissions(dir.path(), Permissions::from_mode(0o700))?;
    let input = if provider.images {
        vec!["text", "image"]
    } else {
        vec!["text"]
    };
    let models = serde_json::json!({
        "providers": {
            provider.provider_id.clone(): {
                "baseUrl": provider.base_url,
                "api": provider.protocol.as_pi_str(),
                "apiKey": "$ALVA_HUB_PI_API_KEY",
                "models": [{
                    "id": provider.model_id,
                    "name": provider.model_name,
                    "reasoning": provider.reasoning,
                    "input": input,
                    "contextWindow": provider.context_window,
                    "maxTokens": provider.max_output_tokens,
                    "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
                }]
            }
        }
    });
    let path = dir.path().join("models.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&models)?)?;
    std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
    Ok((
        Some(dir),
        Some((
            path.parent().unwrap().display().to_string(),
            provider.api_key.clone(),
        )),
    ))
}

fn monitor_stderr(stderr: Option<tokio::process::ChildStderr>) {
    let Some(stderr) = stderr else {
        return;
    };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                warn!(target: "pi-adapter", "{line}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_engine_runtime::RuntimeRequest;

    #[test]
    fn args_keep_secret_out_and_use_stable_session() {
        let config = PiAdapterConfig {
            provider: Some("custom".into()),
            model: Some("model-a".into()),
            thinking: Some("high".into()),
            session_dir: "/tmp/pi-sessions".into(),
            ..PiAdapterConfig::default()
        };
        let args = pi_cli_args(&config, &RuntimeRequest::new("hello"), "session-1");
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--session-id", "session-1"]));
        assert!(args.windows(2).any(|pair| pair == ["--provider", "custom"]));
        assert!(!args.iter().any(|arg| arg.contains("api-key")));
    }

    #[test]
    fn temporary_models_json_references_env_not_secret() {
        let provider = PiProviderConfig {
            provider_id: "custom".into(),
            display_name: "Custom".into(),
            base_url: "https://example.com".into(),
            api_key: "secret-value".into(),
            protocol: crate::config::PiApiProtocol::OpenAiResponses,
            model_id: "model-a".into(),
            model_name: "Model A".into(),
            context_window: 128_000,
            max_output_tokens: 8_000,
            images: true,
            reasoning: true,
        };
        let (dir, env) = prepare_provider_dir(Some(&provider)).unwrap();
        let content = std::fs::read_to_string(dir.unwrap().path().join("models.json")).unwrap();
        assert!(content.contains("$ALVA_HUB_PI_API_KEY"));
        assert!(!content.contains("secret-value"));
        assert_eq!(env.unwrap().1, "secret-value");
    }
}
