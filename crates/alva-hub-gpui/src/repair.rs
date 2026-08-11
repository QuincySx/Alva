// INPUT:  EngineReadiness reports and the official engine distribution matrix
// OUTPUT: structured RepairAction plans plus the allowlisted repair executor
// POS:    Owner of install/upgrade/auth-repair flows; never touches HubStore,
//         so pinned models, accounts, profiles, and policies survive by
//         construction.

use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};

use crate::readiness::{EngineReadiness, ReadinessStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairKind {
    Install,
    Upgrade,
    Authenticate,
    Configure,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairAction {
    pub engine_id: String,
    pub kind: RepairKind,
    pub title: String,
    /// Official command shown to the user (and, for one-click actions, the
    /// exact argv the executor runs — regenerated server-side, never taken
    /// from the client).
    pub command: Option<String>,
    pub url: Option<String>,
    /// True only for safely-distributed engines where Hub can run the
    /// official installer itself.
    pub one_click: bool,
}

/// Official distribution matrix. Claude Code, Codex CLI, and Pi ship through npm
/// under their vendors' scopes — safe to one-click. Auth flows are
/// interactive terminal sessions, so they stay guided.
fn npm_package(engine_id: &str) -> Option<&'static str> {
    match engine_id {
        "claude" => Some("@anthropic-ai/claude-code"),
        "codex" => Some("@openai/codex"),
        "pi" => Some("@earendil-works/pi-coding-agent"),
        _ => None,
    }
}

fn docs_url(engine_id: &str) -> Option<&'static str> {
    match engine_id {
        "claude" => Some("https://docs.anthropic.com/en/docs/claude-code/setup"),
        "codex" => Some("https://developers.openai.com/codex/cli"),
        "pi" => Some("https://github.com/earendil-works/pi"),
        _ => None,
    }
}

/// Map one engine's readiness to its repair path. Pure — probing stays in
/// `readiness`, execution in [`repair_argv`]/[`run_repair_command`].
pub fn repair_plan(readiness: &EngineReadiness) -> Vec<RepairAction> {
    let engine_id = readiness.engine_id.as_str();
    let mut actions = Vec::new();
    match readiness.status {
        ReadinessStatus::Healthy | ReadinessStatus::Unhealthy => {}
        ReadinessStatus::Missing => {
            if let Some(package) = npm_package(engine_id) {
                actions.push(RepairAction {
                    engine_id: engine_id.to_string(),
                    kind: RepairKind::Install,
                    title: format!("Install {}", readiness.display_name),
                    command: Some(format!("npm install -g {package}")),
                    url: docs_url(engine_id).map(String::from),
                    one_click: true,
                });
            }
        }
        ReadinessStatus::Incompatible => {
            if let Some(package) = npm_package(engine_id) {
                actions.push(RepairAction {
                    engine_id: engine_id.to_string(),
                    kind: RepairKind::Upgrade,
                    title: format!(
                        "Upgrade {} (requires ≥ {})",
                        readiness.display_name, readiness.minimum_version
                    ),
                    command: Some(format!("npm install -g {package}@latest")),
                    url: docs_url(engine_id).map(String::from),
                    one_click: true,
                });
            }
        }
        ReadinessStatus::Unauthenticated => match engine_id {
            "alva" => actions.push(RepairAction {
                engine_id: engine_id.to_string(),
                kind: RepairKind::Configure,
                title: "Configure a provider API key".to_string(),
                command: Some("alva settings set <profile> --api-key <key>".to_string()),
                url: None,
                one_click: false,
            }),
            "claude" => actions.push(RepairAction {
                engine_id: engine_id.to_string(),
                kind: RepairKind::Authenticate,
                title: "Sign in to Claude Code".to_string(),
                command: Some("claude".to_string()),
                url: docs_url(engine_id).map(String::from),
                one_click: false,
            }),
            "codex" => actions.push(RepairAction {
                engine_id: engine_id.to_string(),
                kind: RepairKind::Authenticate,
                title: "Sign in to Codex CLI".to_string(),
                command: Some("codex login".to_string()),
                url: docs_url(engine_id).map(String::from),
                one_click: false,
            }),
            // Unknown ids get nothing — defense in depth for the IPC path.
            _ => {}
        },
    }
    actions
}

/// Argv for a one-click action, regenerated from the engine's CURRENT
/// readiness — the client only names (engine, kind); commands never cross
/// the IPC boundary inbound.
pub fn repair_argv(readiness: &EngineReadiness, kind: RepairKind) -> Result<Vec<String>, String> {
    let action = repair_plan(readiness)
        .into_iter()
        .find(|action| action.kind == kind)
        .ok_or_else(|| {
            format!(
                "engine '{}' has no {kind:?} repair for its current state",
                readiness.engine_id
            )
        })?;
    if !action.one_click {
        return Err(format!(
            "{} requires the guided manual steps; Hub does not run it",
            action.title
        ));
    }
    let command = action
        .command
        .expect("one-click actions always carry a command");
    Ok(command.split(' ').map(String::from).collect())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepairOutcome {
    pub success: bool,
    pub log: String,
}

/// Hard bound for one repair run; a wedged installer must never leave the
/// repair button dead until app restart.
const REPAIR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Find a program on PATH plus the well-known install prefixes GUI-launched
/// macOS apps do not inherit (Homebrew, /usr/local).
fn locate_program(name: &str) -> Option<PathBuf> {
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    directories.push(PathBuf::from("/opt/homebrew/bin"));
    directories.push(PathBuf::from("/usr/local/bin"));
    directories
        .into_iter()
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// Run an official repair argv to completion, capturing output, with a hard
/// timeout and kill. Blocking — callers wrap in spawn_blocking. Touches
/// nothing but the child process; failure or cancellation leaves every Hub
/// fact exactly as it was.
pub fn run_repair_command(argv: &[String], program_override: Option<PathBuf>) -> RepairOutcome {
    let Some((program, arguments)) = argv.split_first() else {
        return RepairOutcome {
            success: false,
            log: "empty repair command".to_string(),
        };
    };
    let program: PathBuf = match program_override {
        Some(program) => program,
        None => match locate_program(program) {
            Some(program) => program,
            None => {
                return RepairOutcome {
                    success: false,
                    log: format!(
                        "'{program}' was not found on PATH or in /opt/homebrew/bin, \
                         /usr/local/bin — install Node.js first"
                    ),
                }
            }
        },
    };
    let mut child = match std::process::Command::new(&program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return RepairOutcome {
                success: false,
                log: format!("failed to start {}: {error}", program.display()),
            }
        }
    };
    // Drain pipes on threads so a chatty installer can't fill the pipe
    // buffer and deadlock against the wait loop.
    let stdout = child.stdout.take().map(drain_to_string);
    let stderr = child.stderr.take().map(drain_to_string);
    let deadline = std::time::Instant::now() + REPAIR_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => break None,
        }
    };
    let mut log = String::new();
    for handle in [stdout, stderr].into_iter().flatten() {
        let text = handle.join().unwrap_or_default();
        if !text.trim().is_empty() {
            if !log.is_empty() {
                log.push('\n');
            }
            log.push_str(&text);
        }
    }
    match status {
        Some(status) => RepairOutcome {
            success: status.success(),
            log,
        },
        None => RepairOutcome {
            success: false,
            log: format!(
                "repair timed out after {}s and was stopped\n{log}",
                REPAIR_TIMEOUT.as_secs()
            ),
        },
    }
}

fn drain_to_string<R: std::io::Read + Send + 'static>(
    mut reader: R,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = std::io::Read::read_to_string(&mut reader, &mut buffer);
        buffer
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readiness::{classify_engine, EngineProbeFacts, StreamHealth};

    fn readiness_for(engine_id: &str, facts: EngineProbeFacts) -> EngineReadiness {
        classify_engine(engine_id, facts)
    }

    fn missing() -> EngineProbeFacts {
        EngineProbeFacts::default()
    }

    fn installed(version: &str, authenticated: Option<bool>) -> EngineProbeFacts {
        EngineProbeFacts {
            binary_path: Some("/usr/local/bin/x".into()),
            version: Some(Ok(version.into())),
            capabilities: vec!["streaming".into()],
            authenticated,
            stream_health: StreamHealth::Ok,
            ..EngineProbeFacts::default()
        }
    }

    #[test]
    fn missing_npm_engines_get_a_one_click_install() {
        for (engine, package) in [
            ("claude", "@anthropic-ai/claude-code"),
            ("codex", "@openai/codex"),
        ] {
            let plan = repair_plan(&readiness_for(engine, missing()));
            assert_eq!(plan.len(), 1, "{engine}");
            assert_eq!(plan[0].kind, RepairKind::Install);
            assert!(plan[0].one_click);
            assert_eq!(
                plan[0].command.as_deref(),
                Some(format!("npm install -g {package}").as_str())
            );
        }
        // Alva is built-in: nothing to install.
        assert!(repair_plan(&readiness_for("alva", missing())).is_empty());
    }

    #[test]
    fn incompatible_engines_get_a_one_click_upgrade_with_minimum_named() {
        let plan = repair_plan(&readiness_for("claude", installed("0.2.0", Some(true))));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, RepairKind::Upgrade);
        assert!(plan[0].one_click);
        assert!(plan[0].title.contains("1.0.0"));
        assert!(plan[0]
            .command
            .as_deref()
            .is_some_and(|command| command.ends_with("@latest")));
    }

    #[test]
    fn unauthenticated_engines_get_guided_auth_never_one_click() {
        let claude = repair_plan(&readiness_for("claude", installed("2.0.0", Some(false))));
        assert_eq!(claude[0].kind, RepairKind::Authenticate);
        assert!(!claude[0].one_click);

        let codex = repair_plan(&readiness_for("codex", installed("0.44.0", Some(false))));
        assert_eq!(codex[0].command.as_deref(), Some("codex login"));
        assert!(!codex[0].one_click);

        let alva = repair_plan(&readiness_for("alva", installed("0.1.0", Some(false))));
        assert_eq!(alva[0].kind, RepairKind::Configure);
        assert!(!alva[0].one_click);
    }

    #[test]
    fn healthy_engines_need_no_repair() {
        assert!(repair_plan(&readiness_for("claude", installed("2.0.0", Some(true)))).is_empty());
    }

    #[test]
    fn repair_argv_refuses_guided_actions_and_mismatched_kinds() {
        let unauthenticated = readiness_for("codex", installed("0.44.0", Some(false)));
        let refused = repair_argv(&unauthenticated, RepairKind::Authenticate).unwrap_err();
        assert!(refused.contains("manual steps"));

        let healthy = readiness_for("codex", installed("0.44.0", Some(true)));
        assert!(repair_argv(&healthy, RepairKind::Install).is_err());

        let missing_codex = readiness_for("codex", missing());
        let argv = repair_argv(&missing_codex, RepairKind::Install).unwrap();
        assert_eq!(argv[0], "npm");
        assert!(argv.contains(&"@openai/codex".to_string()));
    }

    #[test]
    fn unknown_engine_ids_never_yield_runnable_repairs() {
        for facts in [missing(), installed("9.9.9", Some(false))] {
            let readiness = readiness_for("evil", facts);
            assert!(
                repair_plan(&readiness).is_empty(),
                "unknown engine produced actions for {:?}",
                readiness.status
            );
            for kind in [
                RepairKind::Install,
                RepairKind::Upgrade,
                RepairKind::Authenticate,
                RepairKind::Configure,
            ] {
                assert!(repair_argv(&readiness, kind).is_err());
            }
        }
    }

    #[test]
    fn repair_action_json_shape_is_pinned_for_the_typescript_bridge() {
        let plan = repair_plan(&readiness_for("codex", missing()));
        let json = serde_json::to_value(&plan[0]).unwrap();
        assert_eq!(json["kind"], "install");
        assert_eq!(json["one_click"], true);
        assert_eq!(json["engine_id"], "codex");
        assert_eq!(json["url"], "https://developers.openai.com/codex/cli");
        assert_eq!(
            serde_json::to_value(RepairKind::Authenticate).unwrap(),
            "authenticate"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_repair_command_reports_success_failure_and_spawn_errors() {
        let ok = run_repair_command(
            &["echo".into(), "installed".into()],
            Some(PathBuf::from("/bin/echo")),
        );
        assert!(ok.success);
        assert!(ok.log.contains("installed"));

        let failed = run_repair_command(&["false".into()], Some(PathBuf::from("/usr/bin/false")));
        assert!(!failed.success);

        let missing = run_repair_command(
            &["definitely-not-a-binary".into()],
            Some(PathBuf::from("/nonexistent/path/xyz")),
        );
        assert!(!missing.success);
        assert!(missing.log.contains("failed to start"));
    }
}
