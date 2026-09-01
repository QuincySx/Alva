// INPUT:  EngineProbe facts (binary discovery, version, auth, stream evidence)
// OUTPUT: structured EngineReadiness reports for the alva/claude/codex/pi dashboard
// POS:    Read-only Hub engine health classification; probes never mutate engine config.

use std::path::{Path, PathBuf};

use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_adapter_pi::{PiAdapter, PiAdapterConfig};
use alva_engine_runtime::{EngineRuntime, RuntimeCapabilities};
use serde::Serialize;

use crate::store::HubStore;

pub const HUB_ENGINE_IDS: [&str; 4] = ["alva", "claude", "codex", "pi"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Healthy,
    Missing,
    Incompatible,
    Unauthenticated,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamHealth {
    Ok,
    #[default]
    Untested,
    Failing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ReadinessReason {
    BinaryNotFound { searched: Vec<String> },
    VersionUnreadable { detail: String },
    VersionIncompatible { found: String, minimum: String },
    Unauthenticated { remedy: String },
    ProviderKeyMissing { remedy: String },
    StreamFailing { last_failure: String },
}

impl ReadinessReason {
    /// Human-readable one-liner for timeline facts (fallback skip reasons).
    pub fn describe(&self) -> String {
        match self {
            ReadinessReason::BinaryNotFound { .. } => "engine binary not found".to_string(),
            ReadinessReason::VersionUnreadable { detail } => {
                format!("engine version unreadable: {detail}")
            }
            ReadinessReason::VersionIncompatible { found, minimum } => {
                format!("engine version {found} is older than required {minimum}")
            }
            ReadinessReason::Unauthenticated { remedy } => {
                format!("not signed in — {remedy}")
            }
            ReadinessReason::ProviderKeyMissing { remedy } => {
                format!("provider key missing — {remedy}")
            }
            ReadinessReason::StreamFailing { last_failure } => {
                format!("stream failing: {last_failure}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineReadiness {
    pub engine_id: String,
    pub display_name: String,
    pub status: ReadinessStatus,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub minimum_version: String,
    pub capabilities: Vec<String>,
    pub authenticated: Option<bool>,
    pub stream_health: StreamHealth,
    pub reasons: Vec<ReadinessReason>,
}

/// Raw facts one probe pass gathered about a single engine. Probes only read
/// (PATH lookup, version output, credential presence, stored attempt history);
/// classification happens in [`classify_engine`].
#[derive(Debug, Clone, Default)]
pub struct EngineProbeFacts {
    pub binary_path: Option<String>,
    pub searched_locations: Vec<String>,
    pub version: Option<Result<String, String>>,
    pub capabilities: Vec<String>,
    pub authenticated: Option<bool>,
    pub auth_remedy: Option<String>,
    pub stream_health: StreamHealth,
    pub last_stream_failure: Option<String>,
}

pub trait EngineProbe: Send + Sync {
    fn facts(&self, engine_id: &str) -> EngineProbeFacts;
}

pub fn probe_readiness(probe: &dyn EngineProbe) -> Vec<EngineReadiness> {
    HUB_ENGINE_IDS
        .iter()
        .map(|engine_id| classify_engine(engine_id, probe.facts(engine_id)))
        .collect()
}

pub fn classify_engine(engine_id: &str, facts: EngineProbeFacts) -> EngineReadiness {
    let spec = engine_spec(engine_id);
    let mut reasons = Vec::new();

    if facts.binary_path.is_none() {
        reasons.push(ReadinessReason::BinaryNotFound {
            searched: facts.searched_locations.clone(),
        });
        return finish(spec, engine_id, facts, ReadinessStatus::Missing, reasons);
    }

    let version = match &facts.version {
        Some(Ok(version)) => Some(version.clone()),
        Some(Err(detail)) => {
            reasons.push(ReadinessReason::VersionUnreadable {
                detail: detail.clone(),
            });
            None
        }
        None => {
            reasons.push(ReadinessReason::VersionUnreadable {
                detail: "the engine version was not probed".to_string(),
            });
            None
        }
    };
    if let Some(found) = &version {
        if !version_at_least(found, spec.minimum_version) {
            reasons.push(ReadinessReason::VersionIncompatible {
                found: found.clone(),
                minimum: spec.minimum_version.to_string(),
            });
            return finish(
                spec,
                engine_id,
                facts,
                ReadinessStatus::Incompatible,
                reasons,
            );
        }
    }
    if reasons
        .iter()
        .any(|reason| matches!(reason, ReadinessReason::VersionUnreadable { .. }))
    {
        return finish(
            spec,
            engine_id,
            facts,
            ReadinessStatus::Incompatible,
            reasons,
        );
    }

    if facts.authenticated == Some(false) {
        let remedy = facts
            .auth_remedy
            .clone()
            .unwrap_or_else(|| spec.default_auth_remedy.to_string());
        reasons.push(if engine_id == "alva" {
            ReadinessReason::ProviderKeyMissing { remedy }
        } else {
            ReadinessReason::Unauthenticated { remedy }
        });
        return finish(
            spec,
            engine_id,
            facts,
            ReadinessStatus::Unauthenticated,
            reasons,
        );
    }

    if facts.stream_health == StreamHealth::Failing {
        reasons.push(ReadinessReason::StreamFailing {
            last_failure: facts
                .last_stream_failure
                .clone()
                .unwrap_or_else(|| "The last streamed run failed.".to_string()),
        });
        return finish(spec, engine_id, facts, ReadinessStatus::Unhealthy, reasons);
    }

    finish(spec, engine_id, facts, ReadinessStatus::Healthy, reasons)
}

struct EngineSpec {
    display_name: &'static str,
    minimum_version: &'static str,
    default_auth_remedy: &'static str,
}

fn engine_spec(engine_id: &str) -> EngineSpec {
    match engine_id {
        "alva" => EngineSpec {
            display_name: "Alva",
            minimum_version: "0.1.0",
            default_auth_remedy: "Configure a provider API key for this workspace.",
        },
        "claude" => EngineSpec {
            display_name: "Claude Code",
            minimum_version: "1.0.0",
            default_auth_remedy: "Run `claude` once in a terminal to sign in.",
        },
        "codex" => EngineSpec {
            display_name: "Codex CLI",
            minimum_version: "0.20.0",
            default_auth_remedy: "Run `codex login` in a terminal to sign in.",
        },
        "pi" => EngineSpec {
            display_name: "Pi",
            minimum_version: "0.84.0",
            default_auth_remedy: "Configure a Pi provider or use a Hub custom Provider.",
        },
        _ => EngineSpec {
            display_name: "Unknown engine",
            minimum_version: "0.0.0",
            default_auth_remedy: "This engine is not supported by Hub.",
        },
    }
}

pub fn capability_labels(capabilities: &RuntimeCapabilities) -> Vec<String> {
    let mut labels = Vec::new();
    if capabilities.streaming {
        labels.push("streaming");
    }
    if capabilities.tool_control {
        labels.push("tool_control");
    }
    if capabilities.permission_callback {
        labels.push("permission_callback");
    }
    if capabilities.resume {
        labels.push("resume");
    }
    if capabilities.cancel {
        labels.push("cancel");
    }
    labels.into_iter().map(String::from).collect()
}

fn finish(
    spec: EngineSpec,
    engine_id: &str,
    facts: EngineProbeFacts,
    status: ReadinessStatus,
    reasons: Vec<ReadinessReason>,
) -> EngineReadiness {
    let version = match facts.version {
        Some(Ok(version)) => Some(version),
        _ => None,
    };
    EngineReadiness {
        engine_id: engine_id.to_string(),
        display_name: spec.display_name.to_string(),
        status,
        binary_path: facts.binary_path,
        version,
        minimum_version: spec.minimum_version.to_string(),
        capabilities: facts.capabilities,
        authenticated: facts.authenticated,
        stream_health: facts.stream_health,
        reasons,
    }
}

fn version_at_least(found: &str, minimum: &str) -> bool {
    let parse = |raw: &str| -> Vec<u64> {
        raw.trim()
            .trim_start_matches(|character: char| !character.is_ascii_digit())
            .split('.')
            .map_while(|part| {
                let digits: String = part
                    .chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect();
                digits.parse::<u64>().ok()
            })
            .collect()
    };
    let found = parse(found);
    let minimum = parse(minimum);
    if found.is_empty() {
        return false;
    }
    for index in 0..minimum.len().max(found.len()) {
        let left = found.get(index).copied().unwrap_or(0);
        let right = minimum.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    true
}

/// Read-only system probe: PATH discovery, `--version` output, credential
/// presence, and stored attempt history. It never launches an engine run and
/// never writes binary, model, account, or permission configuration.
///
/// Stream evidence is copied out of the store at construction time so the
/// probe owns all of its data and the slow CLI probing can run on a blocking
/// worker without borrowing Desktop state.
pub struct SystemProbe {
    stream_outcomes: std::collections::HashMap<String, Option<(String, Option<String>)>>,
    workspace: Option<PathBuf>,
    search_paths: Vec<PathBuf>,
    home_dir: Option<PathBuf>,
}

impl SystemProbe {
    pub fn new(store: Option<&HubStore>, workspace: Option<PathBuf>) -> Self {
        let search_paths = std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).collect())
            .unwrap_or_default();
        let stream_outcomes = HUB_ENGINE_IDS
            .iter()
            .map(|engine_id| {
                let outcome = store
                    .and_then(|store| store.latest_attempt_outcome(engine_id).ok())
                    .flatten();
                (engine_id.to_string(), outcome)
            })
            .collect();
        Self {
            stream_outcomes,
            workspace,
            search_paths,
            home_dir: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    pub fn with_search_paths(mut self, search_paths: Vec<PathBuf>) -> Self {
        self.search_paths = search_paths;
        self
    }

    pub fn with_home_dir(mut self, home_dir: PathBuf) -> Self {
        self.home_dir = Some(home_dir);
        self
    }

    fn locate(&self, command: &str) -> Option<PathBuf> {
        let candidates: Vec<String> = if cfg!(windows) {
            vec![
                format!("{command}.cmd"),
                format!("{command}.exe"),
                command.to_string(),
            ]
        } else {
            vec![command.to_string()]
        };
        self.search_paths
            .iter()
            .flat_map(|directory| candidates.iter().map(move |name| directory.join(name)))
            .find(|candidate| candidate.is_file())
    }

    fn searched_locations(&self) -> Vec<String> {
        self.search_paths
            .iter()
            .map(|directory| directory.display().to_string())
            .collect()
    }

    fn stream_evidence(&self, engine_id: &str) -> (StreamHealth, Option<String>) {
        stream_health_from_outcome(self.stream_outcomes.get(engine_id).cloned().flatten())
    }

    fn alva_facts(&self) -> EngineProbeFacts {
        // The Alva engine runs in-process against the workspace provider
        // config, so "auth" means a usable API key or custom auth headers.
        let workspace = self.workspace.clone().unwrap_or_else(std::env::temp_dir);
        let authenticated = alva_llm_provider::ProviderConfig::load(&workspace)
            .map(|config| !config.api_key.is_empty() || !config.custom_headers.is_empty())
            .unwrap_or(false);
        let (stream_health, last_stream_failure) = self.stream_evidence("alva");
        EngineProbeFacts {
            binary_path: Some("built-in".to_string()),
            searched_locations: Vec::new(),
            version: Some(Ok(env!("CARGO_PKG_VERSION").to_string())),
            // Mirrors AlvaAdapter::capabilities(); building the real adapter
            // requires a configured provider, which an unauthenticated
            // dashboard must still be able to report on.
            capabilities: capability_labels(&RuntimeCapabilities {
                streaming: true,
                tool_control: true,
                permission_callback: true,
                resume: true,
                cancel: true,
            }),
            authenticated: Some(authenticated),
            auth_remedy: None,
            stream_health,
            last_stream_failure,
        }
    }

    fn cli_facts(&self, engine_id: &str) -> EngineProbeFacts {
        let command = engine_id;
        let Some(binary) = self.locate(command) else {
            return EngineProbeFacts {
                searched_locations: self.searched_locations(),
                ..EngineProbeFacts::default()
            };
        };
        let (capabilities, authenticated) = match engine_id {
            "claude" => (
                capability_labels(
                    &ClaudePrintAdapter::new(ClaudePrintConfig::default()).capabilities(),
                ),
                self.claude_authenticated(),
            ),
            "codex" => (
                capability_labels(&CodexAdapter::new(CodexAdapterConfig::default()).capabilities()),
                self.codex_authenticated(),
            ),
            "pi" => (
                capability_labels(&PiAdapter::new(PiAdapterConfig::default()).capabilities()),
                None,
            ),
            _ => (Vec::new(), None),
        };
        let (stream_health, last_stream_failure) = self.stream_evidence(engine_id);
        EngineProbeFacts {
            binary_path: Some(binary.display().to_string()),
            searched_locations: self.searched_locations(),
            version: Some(read_cli_version(&binary)),
            capabilities,
            authenticated,
            auth_remedy: None,
            stream_health,
            last_stream_failure,
        }
    }

    /// Fast readiness veto for fallback-candidate gating: binary presence,
    /// credential presence, and provider-key checks only — no subprocess
    /// calls, so it is safe on an async thread. Version incompatibility is
    /// deliberately not vetoed here (it needs `--version`); an incompatible
    /// engine fails its attempt and the failure matrix classifies it.
    /// Stream-failing history does not veto either — the fallback attempt is
    /// itself the recovery path.
    pub fn quick_veto(&self, engine_id: &str) -> Option<String> {
        match engine_id {
            "alva" => {
                let facts = self.alva_facts();
                (facts.authenticated == Some(false)).then(|| {
                    "provider key missing — configure a provider API key for this workspace."
                        .to_string()
                })
            }
            "claude" | "codex" => {
                if self.locate(engine_id).is_none() {
                    return Some("engine binary not found".to_string());
                }
                let authenticated = if engine_id == "claude" {
                    self.claude_authenticated()
                } else {
                    self.codex_authenticated()
                };
                (authenticated == Some(false)).then(|| {
                    format!("not signed in — run `{engine_id} login` in a terminal to sign in.")
                })
            }
            "pi" => self
                .locate("pi")
                .is_none()
                .then(|| "engine binary not found".to_string()),
            other => Some(format!("engine '{other}' is not supported by Hub")),
        }
    }

    fn claude_authenticated(&self) -> Option<bool> {
        // Claude Code may keep credentials in the macOS Keychain, so a missing
        // file is "unknown", not "signed out".
        let home = self.home_dir.as_deref()?;
        if home.join(".claude/.credentials.json").is_file() {
            return Some(true);
        }
        let config = std::fs::read_to_string(home.join(".claude.json")).ok()?;
        config.contains("oauthAccount").then_some(true)
    }

    fn codex_authenticated(&self) -> Option<bool> {
        let home = self.home_dir.as_deref()?;
        Some(home.join(".codex/auth.json").is_file())
    }
}

impl EngineProbe for SystemProbe {
    fn facts(&self, engine_id: &str) -> EngineProbeFacts {
        match engine_id {
            "alva" => self.alva_facts(),
            _ => self.cli_facts(engine_id),
        }
    }
}

/// `interrupted` attempts are what crash repair stamps when the user quits
/// mid-run — a supported flow, not engine failure — so only `failed` marks the
/// stream as failing.
pub fn stream_health_from_outcome(
    outcome: Option<(String, Option<String>)>,
) -> (StreamHealth, Option<String>) {
    match outcome {
        Some((status, _)) if status == "completed" => (StreamHealth::Ok, None),
        Some((status, reason)) if status == "failed" => (
            StreamHealth::Failing,
            reason.or_else(|| Some("The last run ended without a result.".to_string())),
        ),
        _ => (StreamHealth::Untested, None),
    }
}

const CLI_VERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn read_cli_version(binary: &Path) -> Result<String, String> {
    use std::process::Stdio;

    let mut child = std::process::Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to run --version: {error}"))?;
    let deadline = std::time::Instant::now() + CLI_VERSION_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "--version timed out after {}s",
                    CLI_VERSION_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(error) => return Err(format!("failed to wait for --version: {error}")),
        }
    };
    if !status.success() {
        return Err(format!("--version exited with {status}"));
    }
    let mut stdout = String::new();
    use std::io::Read as _;
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let line = stdout.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        Err("empty --version output".to_string())
    } else {
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct FakeProbe {
        facts: HashMap<&'static str, EngineProbeFacts>,
    }

    impl EngineProbe for FakeProbe {
        fn facts(&self, engine_id: &str) -> EngineProbeFacts {
            self.facts.get(engine_id).cloned().unwrap_or_default()
        }
    }

    fn healthy_facts(binary: &str, version: &str) -> EngineProbeFacts {
        EngineProbeFacts {
            binary_path: Some(binary.to_string()),
            searched_locations: vec!["PATH".to_string()],
            version: Some(Ok(version.to_string())),
            capabilities: vec!["streaming".to_string(), "cancel".to_string()],
            authenticated: Some(true),
            auth_remedy: None,
            stream_health: StreamHealth::Ok,
            last_stream_failure: None,
        }
    }

    #[test]
    fn dashboard_discovers_all_engines_without_manual_paths() {
        let probe = FakeProbe {
            facts: HashMap::new(),
        };
        let report = probe_readiness(&probe);
        let ids: Vec<_> = report
            .iter()
            .map(|engine| engine.engine_id.as_str())
            .collect();
        assert_eq!(ids, vec!["alva", "claude", "codex", "pi"]);
    }

    /// Independent PATH lookup: the smoke test must not reuse SystemProbe's
    /// own `locate` or it could agree with itself while both are wrong.
    fn locate_on_path(command: &str) -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|directory| directory.join(command))
                .find(|candidate| candidate.is_file())
        })
    }

    /// Real-machine smoke probe: exercises `SystemProbe` against the actual
    /// environment (PATH, HOME, real CLI binaries). Unlike the FakeProbe
    /// unit tests, this validates that binary discovery and version parsing
    /// agree with reality and that the classification follows the documented
    /// precedence. It passes on any machine: an installed engine is
    /// cross-checked against its real `--version` output, and an absent one
    /// must classify Missing. This closes the acceptance gap "the
    /// classifier is pinned, the live environment is not" for the probe.
    #[test]
    fn system_probe_matches_the_real_machine() {
        let probe = SystemProbe::new(None, None);
        let report = probe_readiness(&probe);
        assert_eq!(report.len(), HUB_ENGINE_IDS.len());

        for entry in &report {
            let spec = engine_spec(&entry.engine_id);
            match entry.engine_id.as_str() {
                "alva" => {
                    // Built-in engine: never "missing", and with no provider
                    // config in the temp workspace its auth (the provider
                    // key) must classify as unauthenticated.
                    assert!(entry.binary_path.is_some(), "alva is built-in");
                    assert_eq!(
                        entry.status,
                        ReadinessStatus::Unauthenticated,
                        "no provider config in the probe workspace: {:?}",
                        entry.reasons
                    );
                }
                id => {
                    let located = locate_on_path(id);
                    match (entry.binary_path.is_some(), located) {
                        (false, None) => {
                            assert_eq!(entry.status, ReadinessStatus::Missing);
                        }
                        (true, Some(path)) => {
                            // Cross-check the version parser against the raw
                            // CLI output: the probe must have run the same
                            // binary it claims to have found.
                            let raw = match read_cli_version(&path) {
                                Ok(raw) => raw,
                                Err(_) => {
                                    assert!(matches!(
                                        entry.status,
                                        ReadinessStatus::Unhealthy | ReadinessStatus::Incompatible
                                    ));
                                    continue;
                                }
                            };
                            let Some(version) = &entry.version else {
                                panic!("probe found {id} but reported no version");
                            };
                            assert_eq!(
                                version, &raw,
                                "probe version parser disagrees with the real `{id} --version`"
                            );
                            let minimum = spec.minimum_version;
                            if version_at_least(version, minimum) {
                                if entry.authenticated == Some(false) {
                                    assert_eq!(
                                        entry.status,
                                        ReadinessStatus::Unauthenticated,
                                        "{id}: signed-out must not look healthy"
                                    );
                                } else {
                                    assert_eq!(
                                        entry.status,
                                        ReadinessStatus::Healthy,
                                        "{id}: installed, readable, not signed out; reasons: {:?}",
                                        entry.reasons
                                    );
                                }
                            } else {
                                assert_eq!(
                                    entry.status,
                                    ReadinessStatus::Incompatible,
                                    "{id} v{version} is below the minimum {minimum}"
                                );
                            }
                        }
                        _ => panic!("probe and independent PATH lookup disagree about {id} "),
                    }
                }
            }
        }
    }

    /// The engine catalog is the single source for every surface that offers an
    /// engine: readiness, the permission profile table, the main engine picker
    /// and the fallback-chain catalog. A local copy in `ui/fallback.rs` once
    /// listed only three of them, so Pi could be selected as a main engine and
    /// appeared in readiness, but could not be added to a fallback chain at all.
    #[test]
    fn engine_catalog_covers_every_supported_engine() {
        assert_eq!(
            HUB_ENGINE_IDS.as_slice(),
            ["alva", "claude", "codex", "pi"].as_slice(),
            "shrinking this catalog silently drops an engine from every UI that reads it"
        );
    }

    #[test]
    fn missing_binary_reports_structured_search_locations() {
        let facts = EngineProbeFacts {
            searched_locations: vec!["PATH".to_string(), "~/.local/bin".to_string()],
            ..EngineProbeFacts::default()
        };
        let readiness = classify_engine("claude", facts);
        assert_eq!(readiness.status, ReadinessStatus::Missing);
        assert_eq!(
            readiness.reasons,
            vec![ReadinessReason::BinaryNotFound {
                searched: vec!["PATH".to_string(), "~/.local/bin".to_string()],
            }]
        );
        assert!(readiness.binary_path.is_none());
    }

    #[test]
    fn incompatible_version_reports_found_and_minimum() {
        let mut facts = healthy_facts("/usr/local/bin/claude", "0.2.9");
        facts.version = Some(Ok("0.2.9".to_string()));
        let readiness = classify_engine("claude", facts);
        assert_eq!(readiness.status, ReadinessStatus::Incompatible);
        assert_eq!(
            readiness.reasons,
            vec![ReadinessReason::VersionIncompatible {
                found: "0.2.9".to_string(),
                minimum: "1.0.0".to_string(),
            }]
        );
        assert_eq!(readiness.version.as_deref(), Some("0.2.9"));
    }

    #[test]
    fn unreadable_version_is_incompatible_with_structured_detail() {
        let mut facts = healthy_facts("/usr/local/bin/codex", "0.30.0");
        facts.version = Some(Err("exit status 2".to_string()));
        let readiness = classify_engine("codex", facts);
        assert_eq!(readiness.status, ReadinessStatus::Incompatible);
        assert_eq!(
            readiness.reasons,
            vec![ReadinessReason::VersionUnreadable {
                detail: "exit status 2".to_string(),
            }]
        );
        assert!(readiness.version.is_none());
    }

    #[test]
    fn unauthenticated_cli_engine_names_a_remedy_not_stderr() {
        let mut facts = healthy_facts("/usr/local/bin/codex", "0.30.0");
        facts.authenticated = Some(false);
        facts.auth_remedy = None;
        let readiness = classify_engine("codex", facts);
        assert_eq!(readiness.status, ReadinessStatus::Unauthenticated);
        assert_eq!(
            readiness.reasons,
            vec![ReadinessReason::Unauthenticated {
                remedy: "Run `codex login` in a terminal to sign in.".to_string(),
            }]
        );
    }

    #[test]
    fn alva_without_provider_key_reports_provider_key_missing() {
        let mut facts = healthy_facts("built-in", "0.1.0");
        facts.authenticated = Some(false);
        let readiness = classify_engine("alva", facts);
        assert_eq!(readiness.status, ReadinessStatus::Unauthenticated);
        assert_eq!(
            readiness.reasons,
            vec![ReadinessReason::ProviderKeyMissing {
                remedy: "Configure a provider API key for this workspace.".to_string(),
            }]
        );
    }

    #[test]
    fn healthy_engine_reports_version_capabilities_and_stream_health() {
        let readiness = classify_engine("claude", healthy_facts("/usr/local/bin/claude", "1.2.3"));
        assert_eq!(readiness.status, ReadinessStatus::Healthy);
        assert!(readiness.reasons.is_empty());
        assert_eq!(readiness.version.as_deref(), Some("1.2.3"));
        assert_eq!(readiness.authenticated, Some(true));
        assert_eq!(readiness.stream_health, StreamHealth::Ok);
        assert!(readiness.capabilities.contains(&"streaming".to_string()));
    }

    #[test]
    fn failing_stream_marks_engine_unhealthy_with_last_failure() {
        let mut facts = healthy_facts("/usr/local/bin/claude", "1.2.3");
        facts.stream_health = StreamHealth::Failing;
        facts.last_stream_failure = Some("stream ended before Completed".to_string());
        let readiness = classify_engine("claude", facts);
        assert_eq!(readiness.status, ReadinessStatus::Unhealthy);
        assert_eq!(
            readiness.reasons,
            vec![ReadinessReason::StreamFailing {
                last_failure: "stream ended before Completed".to_string(),
            }]
        );
    }

    #[test]
    fn version_comparison_tolerates_prefixes_and_depth() {
        assert!(version_at_least("claude 1.0.0", "1.0.0"));
        assert!(version_at_least("v2.1", "1.9.9"));
        assert!(!version_at_least("0.9", "1.0.0"));
        assert!(version_at_least("1.0", "1.0.0"));
        assert!(!version_at_least("garbage", "1.0.0"));
    }

    #[test]
    fn stream_health_maps_attempt_outcomes_to_structured_states() {
        assert_eq!(
            stream_health_from_outcome(None),
            (StreamHealth::Untested, None)
        );
        assert_eq!(
            stream_health_from_outcome(Some(("completed".to_string(), None))),
            (StreamHealth::Ok, None)
        );
        assert_eq!(
            stream_health_from_outcome(Some((
                "failed".to_string(),
                Some("stream ended early".to_string())
            ))),
            (
                StreamHealth::Failing,
                Some("stream ended early".to_string())
            )
        );
        assert_eq!(
            stream_health_from_outcome(Some(("failed".to_string(), None))),
            (
                StreamHealth::Failing,
                Some("The last run ended without a result.".to_string())
            )
        );
        // Quitting the app mid-run is a supported flow, not engine failure.
        assert_eq!(
            stream_health_from_outcome(Some(("interrupted".to_string(), None))),
            (StreamHealth::Untested, None)
        );
    }

    #[test]
    fn readiness_json_shape_is_pinned_for_the_typescript_bridge() {
        let readiness = classify_engine(
            "claude",
            EngineProbeFacts {
                searched_locations: vec!["/usr/local/bin".to_string()],
                ..EngineProbeFacts::default()
            },
        );
        let json = serde_json::to_value(&readiness).unwrap();
        assert_eq!(json["status"], "missing");
        assert_eq!(json["stream_health"], "untested");
        assert_eq!(json["binary_path"], serde_json::Value::Null);
        assert_eq!(
            json["reasons"][0],
            serde_json::json!({
                "code": "binary_not_found",
                "searched": ["/usr/local/bin"],
            })
        );

        let reason = ReadinessReason::VersionIncompatible {
            found: "0.2.9".to_string(),
            minimum: "1.0.0".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&reason).unwrap(),
            serde_json::json!({
                "code": "version_incompatible",
                "found": "0.2.9",
                "minimum": "1.0.0",
            })
        );
    }

    #[cfg(unix)]
    mod system_probe {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        use super::*;

        fn install_fake_cli(directory: &Path, name: &str, stdout: &str) {
            let path = directory.join(name);
            fs::write(&path, format!("#!/bin/sh\necho \"{stdout}\"\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn probe(bin: &Path, home: &Path) -> SystemProbe {
            SystemProbe::new(None, None)
                .with_search_paths(vec![bin.to_path_buf()])
                .with_home_dir(home.to_path_buf())
        }

        #[test]
        fn discovers_installed_clis_and_reports_missing_ones_with_locations() {
            let bin = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            install_fake_cli(bin.path(), "claude", "2.0.14 (Claude Code)");
            fs::create_dir_all(home.path().join(".claude")).unwrap();
            fs::write(home.path().join(".claude/.credentials.json"), "{}").unwrap();

            let report = probe_readiness(&probe(bin.path(), home.path()));
            let claude = report
                .iter()
                .find(|engine| engine.engine_id == "claude")
                .unwrap();
            assert_eq!(claude.status, ReadinessStatus::Healthy);
            assert_eq!(claude.version.as_deref(), Some("2.0.14 (Claude Code)"));
            assert_eq!(claude.authenticated, Some(true));
            assert!(claude.capabilities.contains(&"streaming".to_string()));

            let codex = report
                .iter()
                .find(|engine| engine.engine_id == "codex")
                .unwrap();
            assert_eq!(codex.status, ReadinessStatus::Missing);
            assert_eq!(
                codex.reasons,
                vec![ReadinessReason::BinaryNotFound {
                    searched: vec![bin.path().display().to_string()],
                }]
            );
        }

        #[test]
        fn outdated_cli_is_incompatible_and_missing_credentials_need_sign_in() {
            let bin = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            install_fake_cli(bin.path(), "codex", "codex-cli 0.4.0");

            let old = probe(bin.path(), home.path()).facts("codex");
            let readiness = classify_engine("codex", old);
            assert_eq!(readiness.status, ReadinessStatus::Incompatible);

            install_fake_cli(bin.path(), "codex", "codex-cli 0.44.0");
            let unauthenticated = probe(bin.path(), home.path()).facts("codex");
            let readiness = classify_engine("codex", unauthenticated);
            assert_eq!(readiness.status, ReadinessStatus::Unauthenticated);
            assert_eq!(
                readiness.reasons,
                vec![ReadinessReason::Unauthenticated {
                    remedy: "Run `codex login` in a terminal to sign in.".to_string(),
                }]
            );

            fs::create_dir_all(home.path().join(".codex")).unwrap();
            fs::write(home.path().join(".codex/auth.json"), "{}").unwrap();
            let healthy = probe(bin.path(), home.path()).facts("codex");
            assert_eq!(
                classify_engine("codex", healthy).status,
                ReadinessStatus::Healthy
            );
        }

        #[test]
        fn quick_veto_flags_missing_and_signed_out_engines_without_subprocesses() {
            let bin = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();

            // codex missing entirely.
            let empty = probe(bin.path(), home.path());
            assert_eq!(
                empty.quick_veto("codex").as_deref(),
                Some("engine binary not found")
            );

            // codex installed but signed out.
            install_fake_cli(bin.path(), "codex", "codex-cli 0.44.0");
            let signed_out = probe(bin.path(), home.path());
            assert!(signed_out
                .quick_veto("codex")
                .is_some_and(|veto| veto.contains("not signed in")));

            // codex signed in.
            fs::create_dir_all(home.path().join(".codex")).unwrap();
            fs::write(home.path().join(".codex/auth.json"), "{}").unwrap();
            assert_eq!(probe(bin.path(), home.path()).quick_veto("codex"), None);

            // claude present with unknown auth (keychain) is not vetoed.
            install_fake_cli(bin.path(), "claude", "2.0.14 (Claude Code)");
            assert_eq!(probe(bin.path(), home.path()).quick_veto("claude"), None);
        }

        #[test]
        fn refreshing_readiness_never_writes_workspace_or_home_config() {
            let bin = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            let workspace = tempfile::tempdir().unwrap();
            install_fake_cli(bin.path(), "claude", "2.0.14 (Claude Code)");
            install_fake_cli(bin.path(), "codex", "codex-cli 0.44.0");

            let list = |root: &Path| {
                let mut entries: Vec<String> = walk(root);
                entries.sort();
                entries
            };
            fn walk(root: &Path) -> Vec<String> {
                let mut entries = Vec::new();
                if let Ok(children) = fs::read_dir(root) {
                    for child in children.flatten() {
                        entries.push(child.path().display().to_string());
                        if child.path().is_dir() {
                            entries.extend(walk(&child.path()));
                        }
                    }
                }
                entries
            }
            let before_home = list(home.path());
            let before_workspace = list(workspace.path());

            let probe = SystemProbe::new(None, Some(workspace.path().to_path_buf()))
                .with_search_paths(vec![bin.path().to_path_buf()])
                .with_home_dir(home.path().to_path_buf());
            let _ = probe_readiness(&probe);

            assert_eq!(list(home.path()), before_home);
            assert_eq!(list(workspace.path()), before_workspace);
        }
    }
}
