//! Engine readiness tests (moved out of the readiness module, ticket 17:
//! the module itself must stay free of engine-name literals; these exercise
//! the builtin blueprints by name through the public registry).
#![allow(unused_imports)]

use std::collections::HashMap;
use std::sync::Arc;

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use alva_hub_desktop::{
    blueprint, capability_labels, classify_engine, probe_readiness, read_cli_version,
    register_engine_blueprint, registered_engine_ids, stream_health_from_outcome, version_at_least,
    veto_missing_declared_binary, EngineProbe, EngineProbeFacts, EngineReadiness, ReadinessReason,
    ReadinessSpec, ReadinessStatus, StreamHealth, SystemProbe, ENGINE_CAPABILITIES,
};

/// The veto in run.rs filters `required_capabilities` through
/// ENGINE_CAPABILITIES before comparing against what an engine reports. If
/// the two ever drift, a real engine capability would stop being checkable
/// (dropped from the filter) or a word an engine can never provide would be
/// treated as vetoable again — the bug this vocabulary split fixed.
#[test]
fn engine_vocabulary_matches_exactly_what_capability_labels_reports() {
    let everything = RuntimeCapabilities {
        streaming: true,
        tool_control: true,
        permission_callback: true,
        resume: true,
        cancel: true,
    };
    assert_eq!(
        capability_labels(&everything),
        ENGINE_CAPABILITIES.to_vec(),
        "capability_labels and ENGINE_CAPABILITIES must stay in lockstep"
    );
}

struct FakeProbe {
    facts: HashMap<String, EngineProbeFacts>,
}

impl EngineProbe for FakeProbe {
    fn facts(&self, engine_id: &str) -> EngineProbeFacts {
        self.facts.get(engine_id).cloned().unwrap_or_default()
    }
}

fn healthy_facts(binary: &str, version: &str) -> EngineProbeFacts {
    EngineProbeFacts {
        binary_path: Some(binary.to_string()),
        dependency_error: None,
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
fn dashboard_discovers_all_four_engines_without_manual_paths() {
    let probe = FakeProbe {
        facts: HashMap::new(),
    };
    let report = probe_readiness(&probe);
    let ids: Vec<_> = report
        .iter()
        .map(|engine| engine.engine_id.as_str())
        .collect();
    // The registry is global to the process: concurrently-running acceptance
    // tests may register additional engines, so assert the builtin prefix in
    // order rather than an exact list.
    // Test-order tolerant: the registry is global and the acceptance test
    // registers a fifth engine in this same binary.
    for builtin in ["alva", "claude", "codex", "pi"] {
        assert!(
            ids.contains(&builtin),
            "builtin engine {builtin} must be discovered: {ids:?}"
        );
    }
}

#[test]
fn hub_catalog_override_is_the_alva_auth_authority() {
    let available = SystemProbe::new(None, Some(std::env::temp_dir()))
        .with_provider_authenticated("alva", true);
    assert_eq!(available.facts("alva").authenticated, Some(true));
    assert_eq!(available.quick_veto("alva"), None);

    let unavailable = SystemProbe::new(None, Some(std::env::temp_dir()))
        .with_provider_authenticated("alva", false);
    assert_eq!(unavailable.facts("alva").authenticated, Some(false));
    assert!(unavailable
        .quick_veto("alva")
        .is_some_and(|reason| reason.contains("Models and engines")));
}

/// Independent PATH lookup: the smoke test must not reuse SystemProbe's
/// own `locate` or it could agree with itself while both are wrong.
fn locate_on_path(command: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(command))
            .find(|candidate| alva_hub_desktop::is_executable_file(candidate))
    })
}

/// Real-machine smoke probe: exercises `SystemProbe` against the actual
/// environment (PATH, HOME, real CLI binaries). Passes on any machine.
#[test]
fn system_probe_matches_the_real_machine() {
    let probe = SystemProbe::new(None, None).with_provider_authenticated("alva", false);
    let report = probe_readiness(&probe);
    assert_eq!(report.len(), registered_engine_ids().len());

    for entry in &report {
        let spec = blueprint(&entry.engine_id)
            .expect("builtin engines are registered")
            .readiness
            .clone();
        match entry.engine_id.as_str() {
            "fifth-engine" => {} // registered by the acceptance test in this binary
            "alva" => {
                assert!(entry.binary_path.is_some(), "alva is built-in");
                assert_eq!(
                    entry.status,
                    ReadinessStatus::Unauthenticated,
                    "no provider config in the probe workspace: {:?}",
                    entry.reasons
                );
            }
            id => {
                let binary = blueprint(id).and_then(|entry| entry.descriptor.as_ref()
                    .and_then(|descriptor| descriptor.install_metadata.required_binary.clone()))
                    .unwrap_or_else(|| id.to_string());
                let located = locate_on_path(&binary);
                if entry.reasons.iter().any(|reason| matches!(reason, ReadinessReason::DependencyUnavailable { .. })) {
                    assert_eq!(entry.status, ReadinessStatus::Missing);
                    continue;
                }
                match (entry.binary_path.is_some(), located) {
                    (false, None) => {
                        assert_eq!(entry.status, ReadinessStatus::Missing);
                    }
                    (true, Some(path)) => {
                        let raw = read_cli_version(&path).unwrap_or_else(|error| {
                            panic!(
                                "real `{id} --version` failed for {}: {error}",
                                path.display()
                            )
                        });
                        let Some(version) = &entry.version else {
                            panic!("probe found {id} but reported no version");
                        };
                        assert_eq!(
                            version, &raw,
                            "probe version parser disagrees with the real `{id} --version`"
                        );
                        let minimum = &spec.minimum_version;
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
            minimum: "20.0.0".to_string(),
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
            remedy: "Configure a provider in Settings > Models and engines.".to_string(),
        }]
    );
}

#[test]
fn healthy_engine_reports_version_capabilities_and_stream_health() {
    let readiness = classify_engine("claude", healthy_facts("/usr/local/bin/node", "22.0.0"));
    assert_eq!(readiness.status, ReadinessStatus::Healthy);
    assert!(readiness.reasons.is_empty());
    assert_eq!(readiness.version.as_deref(), Some("22.0.0"));
    assert_eq!(readiness.authenticated, Some(true));
    assert_eq!(readiness.stream_health, StreamHealth::Ok);
    assert!(readiness.capabilities.contains(&"streaming".to_string()));
}

#[test]
fn failing_stream_marks_engine_unhealthy_with_last_failure() {
    let mut facts = healthy_facts("/usr/local/bin/node", "22.0.0");
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
        SystemProbe::new(None, Some(bin.to_path_buf()))
            .with_search_paths(vec![bin.to_path_buf()])
            .with_home_dir(home.to_path_buf())
    }

    fn install_fake_sdk(home: &Path) {
        let package = home.join(".local/share/alva-hub/runtimes/claude/node_modules/@anthropic-ai/claude-agent-sdk");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"1.0.0","main":"sdk.mjs"}"#).unwrap();
        fs::write(package.join("sdk.mjs"), "throw new Error('readiness must not import');").unwrap();
    }

    #[test]
    fn discovers_installed_clis_and_reports_missing_ones_with_locations() {
        let bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        install_fake_cli(bin.path(), "node", "v22.0.0");
        install_fake_sdk(home.path());
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(home.path().join(".claude/.credentials.json"), "{}").unwrap();

        let report = probe_readiness(&probe(bin.path(), home.path()));
        let claude = report
            .iter()
            .find(|engine| engine.engine_id == "claude")
            .unwrap();
        assert_eq!(claude.status, ReadinessStatus::Healthy);
        assert_eq!(claude.version.as_deref(), Some("v22.0.0"));
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
    fn skips_a_non_executable_path_candidate_for_a_later_working_cli() {
        let blocked_bin = tempfile::tempdir().unwrap();
        let working_bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        install_fake_cli(blocked_bin.path(), "node", "blocked");
        fs::set_permissions(
            blocked_bin.path().join("node"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        install_fake_cli(working_bin.path(), "node", "v22.0.0");

        let facts = SystemProbe::new(None, None)
            .with_search_paths(vec![
                blocked_bin.path().to_path_buf(),
                working_bin.path().to_path_buf(),
            ])
            .with_home_dir(home.path().to_path_buf())
            .facts("claude");

        assert_eq!(
            facts.binary_path.as_deref(),
            Some(working_bin.path().join("node").to_str().unwrap())
        );
        assert_eq!(facts.version, Some(Ok("v22.0.0".into())));
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
        install_fake_cli(bin.path(), "node", "v22.0.0");
        install_fake_sdk(home.path());
        assert_eq!(probe(bin.path(), home.path()).quick_veto("claude"), None);

        // A configured third-party provider replaces subscription auth,
        // but never replaces the runtime binary itself.
        assert_eq!(
            probe(bin.path(), home.path())
                .quick_veto_for_target("pi", true)
                .as_deref(),
            Some("engine binary not found")
        );
        install_fake_cli(bin.path(), "pi", "0.52.9");
        assert_eq!(
            probe(bin.path(), home.path()).quick_veto_for_target("pi", true),
            None
        );
        assert!(probe(bin.path(), home.path())
            .quick_veto("pi")
            .is_some_and(|veto| veto.contains("provider key missing")));
    }

    #[test]
    fn refreshing_readiness_never_writes_workspace_or_home_config() {
        let bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        install_fake_cli(bin.path(), "node", "v22.0.0");
        install_fake_sdk(home.path());
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

struct DummyEngine;

#[async_trait::async_trait]
impl EngineRuntime for DummyEngine {
    fn execute(
        &self,
        _request: RuntimeRequest,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_core::Stream<Item = RuntimeEvent> + Send>>,
        RuntimeError,
    > {
        Ok(Box::pin(tokio_stream::iter(vec![])))
    }
    async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
        Ok(())
    }
    async fn respond_permission(
        &self,
        _session_id: &str,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: false,
            tool_control: false,
            permission_callback: false,
            resume: false,
            cancel: false,
        }
    }
}

/// ticket 15 batch 3: a declared engine is vetted by ITS declared binary
/// (never rejected by id); the "not supported by Hub" gate only applies to
/// engines that never registered a descriptor.
#[test]
fn declared_engine_is_vetted_by_its_binary_not_its_name() {
    use alva_engine_runtime::{ConfigSurface, EngineDescriptor, InstallMetadata, ProviderProtocol};
    let declared = EngineDescriptor {
        runtime: Arc::new(DummyEngine),
        inspector: None,
        permission_plans: Vec::new(),
        config_surface: ConfigSurface {
            supports_max_turns: true,
            provider_protocol: ProviderProtocol::Native,
            supports_fast_mode: false,
            supports_custom_http_headers: false,
        },
        install_metadata: InstallMetadata {
            required_binary: Some("gemini-cli".into()),
            install_hint: None,
        },
    };
    assert_eq!(
        veto_missing_declared_binary(&declared, |b| b == "gemini-cli"),
        None
    );
    let veto = veto_missing_declared_binary(&declared, |_| false).unwrap();
    assert!(
        veto.contains("gemini-cli"),
        "veto must name the binary: {veto}"
    );
    assert!(
        !veto.contains("unsupported"),
        "declared engines are not 'unsupported': {veto}"
    );
    let in_process = EngineDescriptor {
        runtime: declared.runtime.clone(),
        inspector: None,
        permission_plans: Vec::new(),
        config_surface: declared.config_surface.clone(),
        install_metadata: InstallMetadata::default(),
    };
    assert_eq!(veto_missing_declared_binary(&in_process, |_| false), None);
}

/// Ticket 17 acceptance: registering a previously-unknown engine blueprint
/// makes it fully usable — readiness classifies it, the permission gate
/// plans for it, and the assembly constructs it — without touching the
/// readiness, permission, or assembly modules.
#[test]
fn a_newly_registered_engine_is_readied_negotiated_and_built() {
    use alva_engine_runtime::{ConfigSurface, EngineDescriptor, InstallMetadata, ProviderProtocol};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ID: &str = "fifth-engine";

    struct Probe;
    impl EngineProbe for Probe {
        fn facts(&self, engine_id: &str) -> EngineProbeFacts {
            if engine_id != ID {
                return EngineProbeFacts::default();
            }
            EngineProbeFacts {
                binary_path: Some("built-in".to_string()),
                version: Some(Ok("1.0.0".to_string())),
                capabilities: vec!["streaming".to_string()],
                authenticated: Some(true),
                stream_health: StreamHealth::Ok,
                ..EngineProbeFacts::default()
            }
        }
    }

    static BUILDS: AtomicUsize = AtomicUsize::new(0);

    register_engine_blueprint(alva_hub_desktop::EngineBlueprint {
        engine_id: ID.into(),
        descriptor: Some(EngineDescriptor {
            runtime: Arc::new(DummyEngine),
            inspector: None,
            permission_plans: Vec::new(),
            config_surface: ConfigSurface {
                supports_max_turns: false,
                provider_protocol: ProviderProtocol::OpenAiChat,
                supports_fast_mode: true,
                supports_custom_http_headers: true,
            },
            install_metadata: InstallMetadata::default(),
        }),
        readiness: ReadinessSpec {
            display_name: "Fifth Engine".into(),
            minimum_version: "1.0.0".into(),
            default_auth_remedy: "Configure the fifth engine.".into(),
            auth_kind: alva_hub_desktop::AuthKind::ProviderKey,
        },
        probe: Arc::new(|_probe: &SystemProbe| Probe.facts(ID)),
        quick_veto: Arc::new(|_probe, _uses_provider_api| None),
        enforcement: Some(Arc::new(|profile| {
            Ok(alva_hub_desktop::EnforcementPlan {
                relation: alva_hub_desktop::EnforcementRelation::Equivalent,
                enforcement: alva_hub_desktop::EngineEnforcement::DeclaredByEngine,
                summary: format!("fifth engine enforces {}", profile.as_str()),
            })
        })),
        build: Arc::new(|_args| {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(DummyEngine) as Arc<dyn EngineRuntime>)
        }),
    });

    // 1. Readiness: registered → probed → healthy.
    let facts = Probe.facts(ID);
    let readiness = classify_engine(ID, facts);
    assert_eq!(readiness.status, ReadinessStatus::Healthy);
    assert_eq!(readiness.display_name, "Fifth Engine");
    assert!(registered_engine_ids().contains(&ID.to_string()));

    // 2. Permission negotiation: the engine's own mapping answers.
    let plan = alva_hub_desktop::EnforcementPlan {
        relation: alva_hub_desktop::EnforcementRelation::Equivalent,
        enforcement: alva_hub_desktop::EngineEnforcement::DeclaredByEngine,
        summary: "fifth engine enforces workspace_write".into(),
    };
    let negotiated =
        alva_hub_desktop::enforcement_for(ID, alva_hub_desktop::PermissionProfile::WorkspaceWrite);
    assert_eq!(negotiated.unwrap().enforcement, plan.enforcement);

    // 3. Construction through the PUBLIC production entry (R61): the same
    // build_engine_for_target the products call. The test must not replicate
    // production decisions (negotiation, blueprint lookup, arg assembly) —
    // doing so would protect a copy, not the real path.
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = alva_hub_desktop::EngineTarget::for_engine(ID)
        .expect("a registered engine resolves a default target");
    let built = alva_hub_desktop::build_engine_for_target(
        &target,
        None,
        alva_hub_desktop::PermissionProfile::WorkspaceWrite,
        tmp.path(),
        tmp.path(),
    );
    assert!(built.is_ok(), "the registered engine must be constructible through build_engine_for_target: {:?}", built.err());
    assert_eq!(
        BUILDS.load(Ordering::SeqCst),
        1,
        "the production entry must route through the registered blueprint's build"
    );
}
