// INPUT:  crate::engine_registry (blueprints), EngineProbe facts (binary discovery, version, auth, stream evidence)
// OUTPUT: structured EngineReadiness reports for the engine dashboard
// POS:    Read-only Hub engine health classification; probes never mutate engine config.
//         Ticket 17: the module carries ZERO engine names — every per-engine fact
//         (probe, veto, spec) lives in the engine's registered blueprint.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::engine_registry;

pub fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// Auth = a usable provider key / route (in-process engines).
    ProviderKey,
    /// Auth = the CLI's own login state (credential files, keychain).
    CliLogin,
}

/// Per-engine readiness self-description (display name, minimum version,
/// default remedy, and how "authenticated" is interpreted). Lives in the
/// engine's registered blueprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessSpec {
    pub display_name: String,
    pub minimum_version: String,
    pub default_auth_remedy: String,
    pub auth_kind: AuthKind,
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
    engine_registry::registered_engine_ids()
        .iter()
        .map(|engine_id| classify_engine(engine_id, probe.facts(engine_id)))
        .collect()
}

pub fn classify_engine(engine_id: &str, facts: EngineProbeFacts) -> EngineReadiness {
    let Some(spec) = engine_registry::blueprint(engine_id).map(|b| b.readiness.clone()) else {
        // Unregistered engines are not the dashboard's business.
        return finish(
            ReadinessSpec {
                display_name: "Unknown engine".into(),
                minimum_version: "0.0.0".into(),
                default_auth_remedy: "This engine is not supported by Hub.".into(),
                auth_kind: AuthKind::CliLogin,
            },
            engine_id,
            facts,
            ReadinessStatus::Missing,
            vec![ReadinessReason::BinaryNotFound {
                searched: Vec::new(),
            }],
        );
    };
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
        if !version_at_least(found, &spec.minimum_version) {
            reasons.push(ReadinessReason::VersionIncompatible {
                found: found.clone(),
                minimum: spec.minimum_version.clone(),
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
            .unwrap_or_else(|| spec.default_auth_remedy.clone());
        reasons.push(if spec.auth_kind == AuthKind::ProviderKey {
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

/// The engine-capability vocabulary — exactly the set `capability_labels` can
/// produce.
///
/// `EngineTarget::required_capabilities` historically carries words from **two**
/// vocabularies (see [`MODEL_CAPABILITIES`]), so the fallback veto must consider
/// only these. Anything else in that field describes something an engine cannot
/// "provide", and treating it as missing vetoes the candidate on every attempt.
pub const ENGINE_CAPABILITIES: [&str; 5] = [
    "streaming",
    "tool_control",
    "permission_callback",
    "resume",
    "cancel",
];

/// Model-capability words that legitimately share the same field: the
/// provider assembly reads them back (engine wiring) to decide whether the
/// model accepts images. They describe the **model**, not the engine.
pub const MODEL_CAPABILITIES: [&str; 3] = ["images", "vision", "tools"];

pub fn capability_labels(capabilities: &alva_engine_runtime::RuntimeCapabilities) -> Vec<String> {
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
    spec: ReadinessSpec,
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
        display_name: spec.display_name,
        status,
        binary_path: facts.binary_path,
        version,
        minimum_version: spec.minimum_version,
        capabilities: facts.capabilities,
        authenticated: facts.authenticated,
        stream_health: facts.stream_health,
        reasons,
    }
}

pub fn version_at_least(found: &str, minimum: &str) -> bool {
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
    /// Product-owned provider availability per runtime. Hub supplies these
    /// overrides so
    /// readiness and fallback consume the same catalog as Settings and run
    /// resolution instead of re-reading a parallel provider authority.
    provider_authenticated: std::collections::HashMap<String, bool>,
}

impl SystemProbe {
    pub fn new(store: Option<&HubStore>, workspace: Option<PathBuf>) -> Self {
        let search_paths = std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).collect())
            .unwrap_or_default();
        let stream_outcomes = engine_registry::registered_engine_ids()
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
            provider_authenticated: std::collections::HashMap::new(),
        }
    }

    pub fn with_provider_authenticated(
        mut self,
        engine_id: impl Into<String>,
        authenticated: bool,
    ) -> Self {
        self.provider_authenticated
            .insert(engine_id.into(), authenticated);
        self
    }

    pub fn with_search_paths(mut self, search_paths: Vec<PathBuf>) -> Self {
        self.search_paths = search_paths;
        self
    }

    pub fn with_home_dir(mut self, home_dir: PathBuf) -> Self {
        self.home_dir = Some(home_dir);
        self
    }

    pub fn locate(&self, command: &str) -> Option<PathBuf> {
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
            .find(|candidate| is_executable_file(candidate))
    }

    pub fn searched_locations(&self) -> Vec<String> {
        self.search_paths
            .iter()
            .map(|directory| directory.display().to_string())
            .collect()
    }

    pub(crate) fn stream_evidence(&self, engine_id: &str) -> (StreamHealth, Option<String>) {
        stream_health_from_outcome(self.stream_outcomes.get(engine_id).cloned().flatten())
    }

    pub(crate) fn provider_authenticated_for(&self, engine_id: &str) -> Option<bool> {
        self.provider_authenticated.get(engine_id).copied()
    }

    pub(crate) fn workspace_path(&self) -> PathBuf {
        self.workspace.clone().unwrap_or_else(std::env::temp_dir)
    }

    pub(crate) fn home_dir(&self) -> Option<&PathBuf> {
        self.home_dir.as_ref()
    }

    /// Generic CLI facts given the engine-specific capability labels and its
    /// own native-auth answer (both supplied by the engine's blueprint).
    pub fn cli_facts(
        &self,
        engine_id: &str,
        command: &str,
        capabilities: Vec<String>,
        native_authenticated: Option<bool>,
    ) -> EngineProbeFacts {
        let Some(binary) = self.locate(command) else {
            return EngineProbeFacts {
                searched_locations: self.searched_locations(),
                ..EngineProbeFacts::default()
            };
        };
        let authenticated = self
            .provider_authenticated_for(engine_id)
            .map(Some)
            .unwrap_or(native_authenticated);
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
        self.quick_veto_for_target(engine_id, false)
    }

    /// Target-aware fast veto. API-backed CLI routes still require their
    /// runtime binary, but they do not require that CLI's private subscription
    /// login because the provider catalog supplies credentials for this run.
    /// The veto rules live in the engine's blueprint — except the
    /// "declared but unregistered binary" check, which is descriptor-driven.
    pub fn quick_veto_for_target(
        &self,
        engine_id: &str,
        uses_provider_api: bool,
    ) -> Option<String> {
        match engine_registry::blueprint(engine_id) {
            Some(entry) => (entry.quick_veto)(self, uses_provider_api),
            // Unregistered engines are genuinely unsupported — they must
            // register a blueprint into the registry first.
            None => Some(format!("engine '{engine_id}' is not supported by Hub")),
        }
    }

    /// Claude-CLI-style auth: a credential file in HOME, or an OAuth marker
    /// inside the main config. A missing credential file is "unknown", not
    /// "signed out" (the keychain may hold the credentials).
    pub fn file_marker_auth(
        &self,
        credential_file: &str,
        config_file: &str,
        marker: &str,
    ) -> Option<bool> {
        let home = self.home_dir.as_deref()?;
        if home.join(credential_file).is_file() {
            return Some(true);
        }
        let config = std::fs::read_to_string(home.join(config_file)).ok()?;
        config.contains(marker).then_some(true)
    }

    /// Single-file auth: the engine is signed in iff the file exists.
    pub fn file_auth(&self, path: &str) -> Option<bool> {
        let home = self.home_dir.as_deref()?;
        Some(home.join(path).is_file())
    }
}

impl EngineProbe for SystemProbe {
    fn facts(&self, engine_id: &str) -> EngineProbeFacts {
        match engine_registry::blueprint(engine_id) {
            Some(entry) => (entry.probe)(self),
            None => EngineProbeFacts {
                searched_locations: self.searched_locations(),
                ..EngineProbeFacts::default()
            },
        }
    }
}

use crate::store::HubStore;

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

/// Veto a **declared** engine whose required binary is missing. The decision
/// is made from the descriptor's install metadata, never the engine name.
/// In-process engines (`required_binary: None`) pass.
pub fn veto_missing_declared_binary(
    descriptor: &alva_engine_runtime::EngineDescriptor,
    binary_exists: impl Fn(&str) -> bool,
) -> Option<String> {
    match &descriptor.install_metadata.required_binary {
        Some(binary) if !binary_exists(binary) => {
            Some(format!("engine binary '{binary}' not found"))
        }
        // Declared and (binary present or in-process): no veto.
        Some(_) | None => None,
    }
}

pub fn read_cli_version(binary: &Path) -> Result<String, String> {
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
