#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use rusqlite::{params, Connection, TransactionBehavior};

    use crate::claims::ClaimMode;
    use crate::store::handoff::build_handoff_package;
    use crate::store::{
        default_account_source, default_billing_route, now_unix_ms, EngineTarget, HandoffKind,
        HubStore, SideEffectEvidence, StoredBranch, SwitchDisposition, TaskClaim, WorkerLifecycle,
        SCHEMA_VERSION, STATUS_COMPLETED, STATUS_INTERRUPTED, STATUS_RUNNING,
    };
    use crate::{PermissionProfile, ResourceClaim, WorkspaceKind};

    /// Reopen a database after dropping its handle. Under parallel test load
    /// the previous handle's instance-lock release can lag behind `drop`, so
    /// the non-blocking acquisition briefly sees the file as still held.
    /// Retries are bounded: a genuinely leaked handle still fails.
    fn reopen(database: &Path) -> HubStore {
        let mut last = String::new();
        for _ in 0..50 {
            match HubStore::open(database) {
                Ok(store) => return store,
                Err(error) => {
                    last = error;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        panic!("could not reopen {}: {last}", database.display());
    }

    fn assert_branch_target(branch: &StoredBranch, target: &EngineTarget) {
        assert_eq!(branch.engine_id, target.engine_id);
        assert_eq!(branch.model_id, target.model_id);
        assert_eq!(branch.provider_id, target.provider_id);
        assert_eq!(branch.account_source, target.account_source);
        assert_eq!(branch.billing_route, target.billing_route);
        assert_eq!(
            branch.automatic_use_authorized,
            target.automatic_use_authorized
        );
        assert_eq!(branch.required_capabilities, target.required_capabilities);
        assert_eq!(branch.reasoning_effort, target.reasoning_effort);
        assert_eq!(branch.fast_mode, target.fast_mode);
    }

    fn assert_redacted_handoff_target(actual: &EngineTarget, pinned: &EngineTarget) {
        let mut expected = pinned.clone();
        expected.account_source.clear();
        expected.billing_route.clear();
        assert_eq!(actual, &expected);
    }

    #[cfg(unix)]
    #[test]
    fn database_and_parent_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("hub-data");
        let database = data.join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();

        assert_eq!(
            fs::metadata(&data).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_database_and_parent() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let linked_parent = root.path().join("linked");
        symlink(outside.path(), &linked_parent).unwrap();
        let parent_error = HubStore::open(linked_parent.join("alva-hub.db"))
            .err()
            .expect("symlinked parent must be rejected");
        assert!(parent_error.contains("symlinked Hub data directory"));

        let data = root.path().join("data");
        fs::create_dir(&data).unwrap();
        let outside_file = outside.path().join("outside.db");
        fs::write(&outside_file, b"keep").unwrap();
        let database = data.join("alva-hub.db");
        symlink(&outside_file, &database).unwrap();
        let file_error = HubStore::open(&database)
            .err()
            .expect("symlinked database must be rejected");
        assert!(file_error.contains("symlinked Hub database"));
        assert_eq!(fs::read(outside_file).unwrap(), b"keep");
    }

    #[test]
    fn independent_connection_observes_atomic_turn_and_attempt_transaction() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let first = HubStore::open(&database).unwrap();
        let second = Connection::open(&database).unwrap();

        let prepared = first
            .prepare_new_turn(
                "product-1",
                workspace.path(),
                "hello",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let visible: (i64, i64) = second
            .query_row(
                "SELECT (SELECT COUNT(*) FROM turns WHERE product_session_id = ?1),
                        (SELECT COUNT(*) FROM engine_attempts WHERE product_session_id = ?1)",
                ["product-1"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(visible, (1, 1));
        let duplicate = second.execute(
            "INSERT INTO engine_attempts
               (attempt_id, turn_id, product_session_id, branch_id, engine_id,
                status, side_effects_json, started_at)
             VALUES ('overlap', ?1, 'product-1', ?2, 'alva', 'running', '[]', 1)",
            params![prepared.turn_id, prepared.branch_id],
        );
        assert!(
            duplicate.is_err(),
            "the database must reject two active attempts"
        );

        first
            .finish_turn(&prepared, Some("engine-1"), Some("done"), None, None)
            .unwrap();
        let completed = first.get_session("product-1").unwrap().unwrap();
        assert_eq!(completed.session.status, STATUS_COMPLETED);
        assert_eq!(
            completed.session.engine_session_id.as_deref(),
            Some("engine-1")
        );
        assert_eq!(completed.turns[0].result.as_deref(), Some("done"));

        let running = first.prepare_resumed_turn("product-1", "next").unwrap();
        let busy_error = first
            .prepare_resumed_turn("product-1", "overlap")
            .expect_err("the session must not start a second active attempt");
        assert!(busy_error.contains("already running"));
        first
            .finish_turn(&running, Some("engine-1"), Some("next done"), None, None)
            .unwrap();
    }

    /// `required_capabilities` carries two vocabularies (engine words that the
    /// fallback veto checks, model words that the provider assembly reads). A
    /// word in neither is a typo that would be stored happily and then never
    /// satisfied — `tool_calls` is the real one this ticket found in three
    /// fixtures (the engine word is `tool_control`, the model word is `tools`).
    #[test]
    fn an_unknown_required_capability_is_rejected_with_the_legal_vocabularies() {
        let mut target = EngineTarget::for_engine("alva").unwrap();
        target.required_capabilities = vec!["tool_calls".into()];
        let error = crate::store::validate_engine_target(&target)
            .expect_err("a word in neither vocabulary must be rejected");
        assert!(
            error.contains("tool_calls"),
            "names the offending word: {error}"
        );
        assert!(
            error.contains("tool_control"),
            "lists the engine vocabulary so the fix is obvious: {error}"
        );
        assert!(
            error.contains("images"),
            "lists the model vocabulary too: {error}"
        );

        // Both vocabularies are accepted.
        target.required_capabilities = vec!["tool_control".into(), "images".into()];
        crate::store::validate_engine_target(&target)
            .expect("engine and model words are both legal in this field");
    }

    #[test]
    fn new_session_branch_metadata_roundtrips_and_restores_after_reopen() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let target = EngineTarget {
            engine_id: "alva".into(),
            model_id: "synthetic-model".into(),
            provider_id: "synthetic-provider".into(),
            account_source: "Synthetic workspace account".into(),
            billing_route: "Synthetic test billing".into(),
            automatic_use_authorized: true,
            required_capabilities: vec!["streaming".into(), "tool_control".into()],
            reasoning_effort: Some("xhigh".into()),
            fast_mode: true,
        };

        let prepared = store
            .prepare_new_turn(
                "product-target-roundtrip",
                workspace.path(),
                "work",
                &target,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let live = store
            .get_session("product-target-roundtrip")
            .unwrap()
            .unwrap();
        assert_branch_target(&live.branches[0], &target);
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        drop(store);

        let reopened = reopen(&database);
        let recovered = reopened
            .get_session("product-target-roundtrip")
            .unwrap()
            .unwrap();
        assert_branch_target(&recovered.branches[0], &target);
        let resumed = reopened
            .prepare_resumed_turn("product-target-roundtrip", "continue")
            .unwrap();
        assert_eq!(resumed.target, target);
    }

    #[test]
    fn reopening_repairs_an_abandoned_attempt_and_preserves_evidence() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-crash",
                workspace.path(),
                "change the project",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&prepared, "native-crash")
            .unwrap();
        store
            .record_side_effect(
                &prepared,
                SideEffectEvidence {
                    tool_call_id: "tool-1".into(),
                    tool_name: "write_file".into(),
                    succeeded: None,
                    duration_ms: Some(12),
                    path: Some("changed.txt".into()),
                    tool_input: None,
                    tool_output: None,
                },
            )
            .unwrap();

        let running = store.get_session("product-crash").unwrap().unwrap();
        assert_eq!(running.turns[0].status, STATUS_RUNNING);
        assert_eq!(running.turns[0].attempts.len(), 1);
        assert_eq!(running.turns[0].attempts[0].status, STATUS_RUNNING);
        drop(store);

        let reopened = reopen(&database);
        let recovered = reopened.get_session("product-crash").unwrap().unwrap();
        assert_eq!(recovered.session.status, STATUS_INTERRUPTED);
        assert_eq!(recovered.turns[0].status, STATUS_INTERRUPTED);
        assert_eq!(recovered.turns[0].attempts[0].status, STATUS_INTERRUPTED);
        assert!(recovered.turns[0].attempts[0]
            .failure_reason
            .as_deref()
            .unwrap()
            .contains("restarted"));
        assert_eq!(recovered.turns[0].attempts[0].side_effects.len(), 1);
        assert_eq!(
            recovered.turns[0].attempts[0].side_effects[0].succeeded,
            None
        );

        let continued = reopened
            .prepare_resumed_turn("product-crash", "continue safely")
            .unwrap();
        assert_eq!(continued.engine_session_id.as_deref(), Some("native-crash"));
    }

    #[test]
    fn live_store_owns_the_recovery_lock() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let _store = HubStore::open(&database).unwrap();

        let error = HubStore::open(&database)
            .err()
            .expect("a second live Desktop store must not repair active attempts");
        assert!(error.contains("already open"));
    }

    #[test]
    fn migrates_schema_one_turns_into_attempts_without_losing_history() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE hub_sessions (
                   product_session_id TEXT PRIMARY KEY,
                   workspace TEXT NOT NULL,
                   active_branch_id TEXT NOT NULL UNIQUE,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE engine_branches (
                   branch_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   engine_id TEXT NOT NULL,
                   engine_session_id TEXT,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE turns (
                   turn_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   branch_id TEXT NOT NULL,
                   prompt TEXT NOT NULL,
                   final_result TEXT,
                   error_message TEXT,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   completed_at INTEGER
                 );
                 INSERT INTO hub_sessions VALUES
                   ('product-v1', '/tmp/workspace', 'branch-v1', 'completed', 1, 2);
                 INSERT INTO engine_branches VALUES
                   ('branch-v1', 'product-v1', 'alva', 'native-v1', 'active', 1, 2);
                 INSERT INTO turns VALUES
                   ('turn-v1', 'product-v1', 'branch-v1', 'old prompt',
                    'old result', NULL, 'completed', 1, 2);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let store = HubStore::open(&database).unwrap();
        let detail = store.get_session("product-v1").unwrap().unwrap();
        assert_eq!(detail.turns[0].result.as_deref(), Some("old result"));
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.turns[0].attempts[0].status, STATUS_COMPLETED);
        assert_eq!(detail.branches.len(), 1);
        assert_eq!(detail.branches[0].account_source, default_account_source());
        assert_eq!(detail.branches[0].billing_route, default_billing_route());
        assert!(!detail.branches[0].automatic_use_authorized);
        assert!(detail.branches[0].required_capabilities.is_empty());
        // Pre-profile sessions keep their de-facto contract after migration.
        assert_eq!(
            detail.session.permission_profile,
            PermissionProfile::WorkspaceWrite
        );
        assert_eq!(detail.session.workspace_kind, WorkspaceKind::Project);
        let version = store
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn session_workspace_kind_persists_across_reopen() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let target = EngineTarget::for_engine("alva").unwrap();

        let prepared = store
            .prepare_new_turn_with_workspace_kind(
                "product-session-workspace",
                workspace.path(),
                "work in isolation",
                &target,
                PermissionProfile::WorkspaceWrite,
                WorkspaceKind::Session,
            )
            .unwrap();
        store
            .finish_turn(&prepared, Some("native-session"), Some("done"), None, None)
            .unwrap();
        assert_eq!(
            store
                .get_session("product-session-workspace")
                .unwrap()
                .unwrap()
                .session
                .workspace_kind,
            WorkspaceKind::Session
        );
        drop(store);

        let reopened = HubStore::open(&database).unwrap();
        assert_eq!(
            reopened
                .get_session("product-session-workspace")
                .unwrap()
                .unwrap()
                .session
                .workspace_kind,
            WorkspaceKind::Session
        );
    }

    #[test]
    fn permission_profile_persists_and_survives_branch_switches() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let prepared = store
            .prepare_new_turn(
                "product-profile",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::Unrestricted,
            )
            .unwrap();
        assert_eq!(prepared.permission_profile, PermissionProfile::Unrestricted);
        store
            .finish_turn(&prepared, Some("alva-native"), Some("done"), None, None)
            .unwrap();

        // A manual engine switch replaces the active branch but must never
        // touch the session-stable permission profile.
        store
            .request_engine_switch("product-profile", &claude, "user chose claude")
            .unwrap();
        let switched = store.get_session("product-profile").unwrap().unwrap();
        assert_eq!(switched.session.engine_id, "claude");
        assert_eq!(
            switched.session.permission_profile,
            PermissionProfile::Unrestricted
        );

        // Resumed turns carry the stored profile, and it survives reopen.
        let resumed = store
            .prepare_resumed_turn("product-profile", "continue")
            .unwrap();
        assert_eq!(resumed.permission_profile, PermissionProfile::Unrestricted);
        store
            .finish_turn(&resumed, None, Some("ok"), None, None)
            .unwrap();
        drop(store);
        let reopened = reopen(&database);
        assert_eq!(
            reopened
                .get_session("product-profile")
                .unwrap()
                .unwrap()
                .session
                .permission_profile,
            PermissionProfile::Unrestricted
        );
    }

    #[test]
    fn same_route_option_changes_persist_without_creating_a_branch() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let initial = EngineTarget {
            reasoning_effort: Some("high".into()),
            ..EngineTarget::for_engine("codex").unwrap()
        };
        let prepared = store
            .prepare_new_turn(
                "product-options",
                workspace.path(),
                "work",
                &initial,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();

        let updated = EngineTarget {
            reasoning_effort: Some("xhigh".into()),
            fast_mode: true,
            ..initial
        };
        store
            .request_engine_switch("product-options", &updated, "change model options")
            .unwrap();
        let live = store.get_session("product-options").unwrap().unwrap();
        assert_eq!(live.branches.len(), 1);
        assert_branch_target(&live.branches[0], &updated);
        drop(store);

        let reopened = reopen(&database);
        let resumed = reopened
            .prepare_resumed_turn("product-options", "continue")
            .unwrap();
        assert_eq!(resumed.target, updated);
    }

    #[test]
    fn manual_switch_to_a_weaken_only_target_is_rejected_with_the_reason() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();

        let prepared = store
            .prepare_new_turn(
                "product-gated-switch",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::Interactive,
            )
            .unwrap();
        store
            .finish_turn(&prepared, Some("alva-native"), Some("done"), None, None)
            .unwrap();

        for engine_id in ["claude", "codex"] {
            let target = EngineTarget::for_engine(engine_id).unwrap();
            let error = store
                .request_engine_switch("product-gated-switch", &target, "user insisted")
                .expect_err("a weaken-only manual switch must be rejected");
            assert!(
                error.contains("cannot preserve or tighten") && error.contains("interactive"),
                "{engine_id}: {error}"
            );
        }
        // Re-selecting the current engine stays a permitted no-op.
        let same = store
            .request_engine_switch("product-gated-switch", &alva, "stay on alva")
            .unwrap();
        assert_eq!(same.disposition, SwitchDisposition::Applied);
        let detail = store.get_session("product-gated-switch").unwrap().unwrap();
        assert_eq!(detail.session.engine_id, "alva");
        assert!(detail.pending_switch.is_none());
    }

    #[test]
    fn dirty_permission_profile_text_is_rejected_loudly() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-dirty",
                workspace.path(),
                "work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE hub_sessions SET permission_profile = 'yolo'
                  WHERE product_session_id = 'product-dirty'",
                [],
            )
            .unwrap();

        let error = store
            .get_session("product-dirty")
            .expect_err("a corrupted profile must not silently default");
        assert!(
            error.contains("yolo"),
            "error should name the value: {error}"
        );
    }

    #[test]
    fn rejects_a_future_schema_without_downgrading_it() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);

        let error = HubStore::open(&database)
            .err()
            .expect("future schema must be rejected");
        assert!(error.contains("newer than supported"));

        let connection = Connection::open(&database).unwrap();
        let version = connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION + 1);
    }

    #[test]
    fn manual_switch_parks_source_and_persists_full_handoff() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let prepared = store
            .prepare_new_turn(
                "product-switch",
                workspace.path(),
                "build the feature",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&prepared, "alva-native-1")
            .unwrap();
        store
            .finish_turn(
                &prepared,
                Some("alva-native-1"),
                Some("feature complete"),
                None,
                None,
            )
            .unwrap();

        let switched = store
            .request_engine_switch("product-switch", &claude, "User explicitly selected Claude")
            .unwrap();
        assert_eq!(switched.disposition, SwitchDisposition::Applied);
        assert_eq!(switched.active_engine_id, "claude");

        let detail = store.get_session("product-switch").unwrap().unwrap();
        assert_eq!(detail.session.engine_id, "claude");
        assert_eq!(detail.branches.len(), 2);
        let source = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert_eq!(source.status, "parked");
        assert_eq!(source.engine_session_id.as_deref(), Some("alva-native-1"));
        let target = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "claude")
            .unwrap();
        assert_eq!(target.status, "active");
        assert_eq!(target.model_id, "engine-default");
        assert_eq!(target.provider_id, "claude-cli");
        assert_eq!(detail.handoffs.len(), 1);
        let handoff = &detail.handoffs[0];
        assert_eq!(handoff.source_branch_id, source.branch_id);
        assert_eq!(handoff.target_branch_id, target.branch_id);
        assert_eq!(handoff.reason, "User explicitly selected Claude");
        assert_eq!(handoff.user_goal.as_deref(), Some("build the feature"));
        assert_eq!(
            handoff.latest_instruction.as_deref(),
            Some("build the feature")
        );
        assert_eq!(handoff.completed_work.len(), 1);
        assert_eq!(handoff.completed_work[0].result, "feature complete");
        assert_eq!(detail.timeline_events.len(), 1);
        assert_eq!(detail.timeline_events[0].kind, "engine_switched");

        drop(store);
        let reopened = reopen(&database);
        let recovered = reopened.get_session("product-switch").unwrap().unwrap();
        assert_eq!(recovered.session.engine_id, "claude");
        assert_eq!(recovered.branches.len(), 2);
        assert_eq!(recovered.handoffs.len(), 1);
    }

    #[test]
    fn running_switch_is_queued_until_the_attempt_is_terminal() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let codex = EngineTarget::for_engine("codex").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-queued",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();

        let queued = store
            .request_engine_switch("product-queued", &codex, "User explicitly selected Codex")
            .unwrap();
        assert_eq!(queued.disposition, SwitchDisposition::Queued);
        assert_eq!(queued.active_engine_id, "alva");
        let running = store.get_session("product-queued").unwrap().unwrap();
        assert_eq!(running.session.engine_id, "alva");
        assert_eq!(
            running.pending_switch.as_ref().unwrap().target.engine_id,
            "codex"
        );
        assert_eq!(running.timeline_events[0].kind, "engine_switch_queued");

        let cancelled = store
            .request_engine_switch(
                "product-queued",
                &alva,
                "User returned to the active engine",
            )
            .unwrap();
        assert_eq!(cancelled.disposition, SwitchDisposition::Applied);
        assert!(store
            .get_session("product-queued")
            .unwrap()
            .unwrap()
            .pending_switch
            .is_none());
        store
            .request_engine_switch(
                "product-queued",
                &codex,
                "User explicitly selected Codex again",
            )
            .unwrap();

        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        let applied = store
            .apply_pending_switch("product-queued")
            .unwrap()
            .expect("queued switch should apply at the safe boundary");
        assert_eq!(applied.disposition, SwitchDisposition::Applied);
        assert_eq!(applied.active_engine_id, "codex");

        let detail = store.get_session("product-queued").unwrap().unwrap();
        assert!(detail.pending_switch.is_none());
        assert_eq!(detail.session.engine_id, "codex");
        assert_eq!(detail.timeline_events.len(), 4);
        assert_eq!(detail.timeline_events[1].kind, "engine_switch_cancelled");
        assert_eq!(detail.timeline_events[3].kind, "engine_switched");
    }

    #[test]
    fn reopening_applies_a_switch_queued_before_a_crash() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let source = EngineTarget {
            engine_id: "alva".into(),
            model_id: "synthetic-alva-model".into(),
            provider_id: "synthetic-alva-provider".into(),
            account_source: "Synthetic Alva account".into(),
            billing_route: "Synthetic workspace billing".into(),
            automatic_use_authorized: false,
            required_capabilities: vec!["tool_control".into()],
            reasoning_effort: Some("medium".into()),
            fast_mode: false,
        };
        let target = EngineTarget {
            engine_id: "claude".into(),
            model_id: "synthetic-claude-model".into(),
            provider_id: "synthetic-claude-provider".into(),
            account_source: "Synthetic Claude account".into(),
            billing_route: "Synthetic Anthropic billing".into(),
            automatic_use_authorized: false,
            required_capabilities: vec!["streaming".into(), "resume".into()],
            reasoning_effort: Some("high".into()),
            fast_mode: true,
        };
        store
            .prepare_new_turn(
                "product-crash-switch",
                workspace.path(),
                "work",
                &source,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .request_engine_switch(
                "product-crash-switch",
                &target,
                "User selected Claude before restart",
            )
            .unwrap();
        let queued = store.get_session("product-crash-switch").unwrap().unwrap();
        assert_eq!(queued.pending_switch.unwrap().target, target);
        drop(store);

        let reopened = reopen(&database);
        let detail = reopened
            .get_session("product-crash-switch")
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.status, STATUS_INTERRUPTED);
        assert_eq!(detail.session.engine_id, "claude");
        assert!(detail.pending_switch.is_none());
        assert_eq!(detail.handoffs.len(), 1);
        let target_branch = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "claude")
            .unwrap();
        assert_branch_target(target_branch, &target);
        let source_branch = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert_branch_target(source_branch, &source);
        assert_redacted_handoff_target(&detail.handoffs[0].source, &source);
        assert_redacted_handoff_target(&detail.handoffs[0].target, &target);
        let resumed = reopened
            .prepare_resumed_turn("product-crash-switch", "continue")
            .unwrap();
        assert_eq!(resumed.target, target);
    }

    #[test]
    fn parked_branch_records_watermark_and_builds_a_delta_handoff_on_return() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let first = store
            .prepare_new_turn(
                "product-delta",
                workspace.path(),
                "alva work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store.record_engine_session(&first, "alva-native").unwrap();
        store
            .finish_turn(
                &first,
                Some("alva-native"),
                Some("alva complete"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-delta", &claude, "switch to Claude")
            .unwrap();

        let claude_turn = store
            .prepare_resumed_turn("product-delta", "claude work")
            .unwrap();
        store
            .finish_turn(
                &claude_turn,
                Some("claude-native"),
                Some("claude complete"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-delta", &alva, "return to Alva")
            .unwrap();

        let detail = store.get_session("product-delta").unwrap().unwrap();
        let alva_branch = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert!(alva_branch.watermark_turn_rowid.is_some());

        let resumed = store
            .prepare_resumed_turn("product-delta", "resume Alva")
            .unwrap();
        assert_eq!(resumed.engine_session_id.as_deref(), Some("alva-native"));
        let handoff = resumed
            .handoff
            .expect("native resume must receive a handoff");
        assert_eq!(handoff.kind, HandoffKind::Delta);
        assert_eq!(handoff.completed_work.len(), 1);
        assert_eq!(handoff.completed_work[0].result, "claude complete");
        assert!(!handoff
            .completed_work
            .iter()
            .any(|work| work.result == "alva complete"));
    }

    #[test]
    fn native_resume_upgrade_is_crash_recoverable_with_full_handoff() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let first = store
            .prepare_new_turn(
                "product-upgrade",
                workspace.path(),
                "alva work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&first, "expired-native")
            .unwrap();
        store
            .finish_turn(
                &first,
                Some("expired-native"),
                Some("alva complete"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-upgrade", &claude, "switch to Claude")
            .unwrap();
        let claude_turn = store
            .prepare_resumed_turn("product-upgrade", "claude work")
            .unwrap();
        store
            .finish_turn(&claude_turn, None, Some("claude complete"), None, None)
            .unwrap();
        store
            .request_engine_switch("product-upgrade", &alva, "return to Alva")
            .unwrap();
        let resume = store
            .prepare_resumed_turn("product-upgrade", "resume Alva")
            .unwrap();
        assert!(resume.engine_session_id.is_some());

        let fresh = store
            .upgrade_failed_native_resume(&resume, "Session not found: expired-native")
            .unwrap();
        assert!(fresh.engine_session_id.is_none());
        assert_eq!(fresh.handoff.as_ref().unwrap().kind, HandoffKind::Full);
        drop(store);

        let reopened = reopen(&database);
        let detail = reopened.get_session("product-upgrade").unwrap().unwrap();
        assert_eq!(detail.session.engine_session_id, None);
        assert_eq!(detail.turns.last().unwrap().status, STATUS_INTERRUPTED);
        assert_eq!(detail.turns.last().unwrap().attempts.len(), 2);
        assert!(detail
            .handoffs
            .iter()
            .any(|handoff| handoff.kind == HandoffKind::Full));
    }

    #[test]
    fn new_session_snapshots_global_fallback_policy_without_later_drift() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        claude.required_capabilities = vec!["streaming".into(), "resume".into()];
        store
            .set_global_fallback_policy(vec![claude.clone()])
            .unwrap();

        store
            .prepare_new_turn(
                "product-policy-snapshot",
                workspace.path(),
                "start work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let first = store
            .get_session("product-policy-snapshot")
            .unwrap()
            .unwrap();
        assert_eq!(first.fallback_policy.targets, vec![claude]);

        let mut codex = EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![codex.clone()])
            .unwrap();
        let unchanged = store
            .get_session("product-policy-snapshot")
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.fallback_policy.targets[0].engine_id, "claude");

        store
            .prepare_new_turn(
                "product-policy-new",
                workspace.path(),
                "start another",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let later = store.get_session("product-policy-new").unwrap().unwrap();
        assert_eq!(later.fallback_policy.targets, vec![codex]);
    }

    #[test]
    fn automatic_fallback_needs_explicit_session_authorization_but_manual_switch_does_not() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-policy-auth",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        store
            .set_session_fallback_policy("product-policy-auth", vec![claude.clone(), alva.clone()])
            .unwrap();
        let policy = store
            .get_session("product-policy-auth")
            .unwrap()
            .unwrap()
            .fallback_policy;
        assert_eq!(
            policy
                .targets
                .iter()
                .map(|target| target.engine_id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude", "alva"]
        );
        assert!(!store
            .is_authorized_for_automatic_fallback("product-policy-auth", &claude)
            .unwrap());

        let manual = store
            .request_engine_switch(
                "product-policy-auth",
                &claude,
                "User explicitly selected Claude",
            )
            .unwrap();
        assert_eq!(manual.active_engine_id, "claude");

        let mut authorized = claude.clone();
        authorized.automatic_use_authorized = true;
        store
            .set_session_fallback_policy("product-policy-auth", vec![authorized.clone()])
            .unwrap();
        assert!(store
            .is_authorized_for_automatic_fallback("product-policy-auth", &authorized)
            .unwrap());
    }

    #[test]
    fn session_fallback_policy_and_target_account_metadata_survive_restart() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-policy-restart",
                workspace.path(),
                "work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        let mut codex = EngineTarget::for_engine("codex").unwrap();
        codex.account_source = "Personal Codex subscription".into();
        codex.billing_route = "OpenAI account · monthly plan".into();
        codex.automatic_use_authorized = true;
        codex.required_capabilities = vec!["streaming".into()];
        store
            .set_session_fallback_policy("product-policy-restart", vec![codex.clone()])
            .unwrap();
        drop(store);

        let reopened = reopen(&database);
        let detail = reopened
            .get_session("product-policy-restart")
            .unwrap()
            .unwrap();
        assert_eq!(detail.fallback_policy.targets, vec![codex]);
    }

    #[test]
    fn automatic_fallback_continues_the_turn_with_serial_attempt_and_persisted_handoff() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.model_id = "synthetic-fallback-model".into();
        claude.provider_id = "synthetic-fallback-provider".into();
        claude.account_source = "Synthetic fallback account".into();
        claude.billing_route = "Synthetic fallback billing".into();
        claude.automatic_use_authorized = true;
        claude.required_capabilities = vec!["streaming".into(), "tool_control".into()];
        let prepared = store
            .prepare_new_turn(
                "product-fallback",
                workspace.path(),
                "do the work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-fallback", vec![claude.clone()])
            .unwrap();

        let next = store
            .fallback_failed_attempt(&prepared, "engine process died", &claude)
            .unwrap();
        assert_eq!(next.turn_id, prepared.turn_id);
        assert_ne!(next.attempt_id, prepared.attempt_id);
        assert_eq!(next.target, claude);
        assert!(next.handoff.as_ref().is_some_and(|handoff| {
            handoff.kind == HandoffKind::Full
                && handoff.reason == "engine process died"
                && handoff.target.model_id == claude.model_id
                && handoff.target.provider_id == claude.provider_id
                && handoff.target.automatic_use_authorized == claude.automatic_use_authorized
                && handoff.target.required_capabilities == claude.required_capabilities
                && handoff.target.account_source.is_empty()
                && handoff.target.billing_route.is_empty()
        }));
        assert!(!next.native_resume);

        let detail = store.get_session("product-fallback").unwrap().unwrap();
        assert_eq!(detail.session.engine_id, "claude");
        assert_eq!(detail.session.status, "running");
        let source = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert_eq!(source.status, "parked");
        let target = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "claude")
            .unwrap();
        assert_eq!(target.status, "active");
        assert_branch_target(target, &claude);

        assert_eq!(detail.turns.len(), 1);
        let attempts = &detail.turns[0].attempts;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].engine_id, "alva");
        assert_eq!(attempts[0].status, "failed");
        assert_eq!(
            attempts[0].failure_reason.as_deref(),
            Some("engine process died")
        );
        assert_eq!(attempts[1].engine_id, "claude");
        assert_eq!(attempts[1].status, "running");

        assert!(detail.handoffs.iter().any(
            |handoff| handoff.kind == HandoffKind::Full && handoff.target.engine_id == "claude"
        ));
        let fallback_event = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "engine_fallback")
            .unwrap();
        assert_eq!(fallback_event.source_engine_id, "alva");
        assert_eq!(fallback_event.target_engine_id, "claude");
        assert!(fallback_event.reason.contains("engine process died"));
        assert!(fallback_event.reason.contains("handoff"));
    }

    #[test]
    fn fallback_requires_session_authorization_and_a_different_engine() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-fallback-auth",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-fallback-auth", vec![claude.clone()])
            .unwrap();

        let unauthorized = store
            .fallback_failed_attempt(&prepared, "engine process died", &claude)
            .unwrap_err();
        assert!(unauthorized.contains("not authorized"));

        let same_engine = store
            .fallback_failed_attempt(&prepared, "engine process died", &alva)
            .unwrap_err();
        assert!(same_engine.contains("different engine"));

        // The rejected fallback must leave the source attempt recoverable.
        let detail = store.get_session("product-fallback-auth").unwrap().unwrap();
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.turns[0].attempts[0].status, "running");
        assert_eq!(detail.session.engine_id, "alva");

        claude.automatic_use_authorized = true;
        store
            .set_session_fallback_policy("product-fallback-auth", vec![claude.clone()])
            .unwrap();
        assert!(store
            .fallback_failed_attempt(&prepared, "engine process died", &claude)
            .is_ok());
    }

    #[test]
    fn returning_to_the_fallback_source_branch_receives_the_fallback_turn_in_the_delta() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;

        let first = store
            .prepare_new_turn(
                "product-fallback-delta",
                workspace.path(),
                "first task",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store.record_engine_session(&first, "alva-native").unwrap();
        store
            .finish_turn(
                &first,
                Some("alva-native"),
                Some("alva did the first task"),
                None,
                None,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-fallback-delta", vec![claude.clone()])
            .unwrap();

        let second = store
            .prepare_resumed_turn("product-fallback-delta", "second task")
            .unwrap();
        let fallback = store
            .fallback_failed_attempt(&second, "alva stream died", &claude)
            .unwrap();
        store
            .finish_turn(
                &fallback,
                Some("claude-native"),
                Some("claude completed the second task"),
                None,
                None,
            )
            .unwrap();

        store
            .request_engine_switch(
                "product-fallback-delta",
                &EngineTarget::for_engine("alva").unwrap(),
                "return to Alva",
            )
            .unwrap();
        let resumed = store
            .prepare_resumed_turn("product-fallback-delta", "third task")
            .unwrap();
        assert_eq!(resumed.engine_session_id.as_deref(), Some("alva-native"));
        let handoff = resumed
            .handoff
            .expect("returning source receives a handoff");
        assert_eq!(handoff.kind, HandoffKind::Delta);
        // The turn the fallback engine completed happened after the source
        // engine died — it must be part of the delta.
        assert!(handoff
            .completed_work
            .iter()
            .any(|work| work.result == "claude completed the second task"));
        assert!(!handoff
            .completed_work
            .iter()
            .any(|work| work.result == "alva did the first task"));
    }

    #[test]
    fn next_authorized_fallback_target_follows_chain_order_and_skips_tried_engines() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let codex = EngineTarget::for_engine("codex").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        store
            .prepare_new_turn(
                "product-chain",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-chain", vec![codex, claude.clone()])
            .unwrap();

        // codex is first in the chain but unauthorized; claude is next.
        let selected = store
            .next_authorized_fallback_target("product-chain", std::slice::from_ref(&alva))
            .unwrap()
            .unwrap();
        assert_eq!(selected.engine_id, "claude");

        let exhausted = store
            .next_authorized_fallback_target("product-chain", &[alva, claude])
            .unwrap();
        assert!(exhausted.is_none());
    }

    #[test]
    fn same_engine_alternate_provider_targets_each_get_their_own_try() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude_a = EngineTarget::for_engine("claude").unwrap();
        claude_a.automatic_use_authorized = true;
        let mut claude_b = claude_a.clone();
        claude_b.provider_id = "claude-alt-provider".into();
        store
            .prepare_new_turn(
                "product-alt-provider",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy(
                "product-alt-provider",
                vec![claude_a.clone(), claude_b.clone()],
            )
            .unwrap();

        let first = store
            .next_authorized_fallback_target("product-alt-provider", std::slice::from_ref(&alva))
            .unwrap()
            .unwrap();
        assert_eq!(first.provider_id, "claude-cli");

        let second = store
            .next_authorized_fallback_target(
                "product-alt-provider",
                &[alva.clone(), claude_a.clone()],
            )
            .unwrap()
            .unwrap();
        assert_eq!(second.provider_id, "claude-alt-provider");

        let exhausted = store
            .next_authorized_fallback_target("product-alt-provider", &[alva, claude_a, claude_b])
            .unwrap();
        assert!(exhausted.is_none());
    }

    #[test]
    fn transcript_is_opt_in_and_passes_the_same_filter() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-transcript",
                workspace.path(),
                "set things up",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(
                &prepared,
                None,
                Some("done, used GITHUB_TOKEN=ghp_canarytoken99x"),
                None,
                None,
            )
            .unwrap();

        // Default: no transcript crosses engines.
        store
            .request_engine_switch("product-transcript", &claude, "try Claude")
            .unwrap();
        let detail = store.get_session("product-transcript").unwrap().unwrap();
        assert!(!detail.session.include_full_transcript);
        assert!(detail.handoffs.last().unwrap().transcript.is_empty());

        // Opt in: transcript attaches, but through the same filter.
        store
            .set_transcript_sharing("product-transcript", true)
            .unwrap();
        store
            .request_engine_switch(
                "product-transcript",
                &EngineTarget::for_engine("codex").unwrap(),
                "try Codex",
            )
            .unwrap();
        let detail = store.get_session("product-transcript").unwrap().unwrap();
        assert!(detail.session.include_full_transcript);
        let handoff = detail
            .handoffs
            .iter()
            .find(|handoff| handoff.target.engine_id == "codex")
            .unwrap();
        assert_eq!(handoff.transcript.len(), 1);
        let result = handoff.transcript[0].result.as_deref().unwrap();
        assert!(result.contains("[redacted]"));
        assert!(!result.contains("ghp_canarytoken99x"));
    }

    #[test]
    fn team_run_walking_skeleton_survives_reopen_with_all_associations() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        // No lead session — refused.
        assert!(store.create_team_run("missing-lead", &claude).is_err());

        let lead = store
            .prepare_new_turn(
                "lead-session",
                workspace.path(),
                "plan the work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();

        let run = store.create_team_run("lead-session", &claude).unwrap();
        assert_eq!(run.lead_session_id, "lead-session");
        assert_eq!(run.workers.len(), 1);
        assert_eq!(run.workers[0].engine_id, "claude");
        // The worker is a Hub-owned durable session with its own branch and
        // the lead's profile — not an engine-private subagent.
        let worker_session = store
            .get_session(&run.workers[0].worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(worker_session.session.engine_id, "claude");
        assert_eq!(
            worker_session.session.permission_profile,
            PermissionProfile::WorkspaceWrite
        );

        let queued = store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "implement the parser",
                &[],
            )
            .unwrap();
        assert_eq!(queued.status, "queued");
        let task = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();
        assert_eq!(task.status, "running");

        // Worker executes a turn with evidence, then the task closes.
        let worker_turn = store
            .prepare_resumed_turn(&run.workers[0].worker_session_id, "implement the parser")
            .unwrap();
        store
            .record_side_effect(
                &worker_turn,
                SideEffectEvidence {
                    tool_call_id: "t1".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(10),
                    path: Some("src/parser.rs".into()),
                    tool_input: None,
                    tool_output: None,
                },
            )
            .unwrap();
        store
            .finish_turn(&worker_turn, None, Some("parser implemented"), None, None)
            .unwrap();
        let finished = store
            .finish_worker_task(&task.task_id, true, Some("parser implemented"))
            .unwrap();
        assert_eq!(finished.status, "completed");
        assert_eq!(finished.changed_files, vec!["src/parser.rs".to_string()]);
        assert_eq!(finished.verification.len(), 1);

        // The outcome reached the lead's timeline.
        let lead_detail = store.get_session("lead-session").unwrap().unwrap();
        let outcome = lead_detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "worker_outcome")
            .expect("worker outcome fact on lead timeline");
        assert!(outcome.reason.contains("parser implemented"));
        assert_eq!(outcome.target_engine_id, "claude");

        // Multi-byte results must excerpt safely (chars, not bytes) — this
        // used to poison the store mutex via a byte-boundary panic.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "写中文文档",
                &[],
            )
            .unwrap();
        let second_task = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();
        let long_chinese =
            "完成了所有的中文文档编写工作，包括安装指南、配置说明和常见问题解答".repeat(8);
        let finished = store
            .finish_worker_task(&second_task.task_id, true, Some(&long_chinese))
            .unwrap();
        assert_eq!(finished.status, "completed");

        // Everything survives a database reopen, fully associated.
        drop(store);
        let reopened = reopen(&database);
        let run = reopened.team_run_for_lead("lead-session").unwrap().unwrap();
        assert_eq!(run.workers.len(), 1);
        assert_eq!(run.tasks.len(), 2);
        assert_eq!(run.tasks[0].status, "completed");
        assert_eq!(
            run.tasks[0].changed_files,
            vec!["src/parser.rs".to_string()]
        );
        assert_eq!(run.tasks[1].status, "completed");
        assert!(reopened
            .get_session(&run.workers[0].worker_session_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn worker_queues_are_fifo_editable_and_cancellable_until_claimed() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "queue-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("queue-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        // A second worker on a different engine — the batch's parallel intent.
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
                None,
            )
            .unwrap();
        assert_eq!(run.workers.len(), 2);
        let claude_worker = run.workers[0].worker_id.clone();
        let codex_worker = run.workers[1].worker_id.clone();

        let first = store
            .enqueue_worker_task(&run.team_run_id, &claude_worker, "first", &[])
            .unwrap();
        let second = store
            .enqueue_worker_task(&run.team_run_id, &claude_worker, "second", &[])
            .unwrap();
        let other = store
            .enqueue_worker_task(&run.team_run_id, &codex_worker, "codex work", &[])
            .unwrap();

        // Pending tasks are editable and cancellable before consumption.
        let edited = store
            .update_worker_task_scope(&second.task_id, "second, refined")
            .unwrap();
        assert_eq!(edited.scope, "second, refined");
        store.cancel_worker_task(&first.task_id).unwrap();

        // FIFO per worker; cancelled tasks are never claimed; queues are
        // independent between workers.
        let claimed = store.claim_for_test(&claude_worker).unwrap().unwrap();
        assert_eq!(claimed.task_id, edited.task_id);
        assert_eq!(claimed.status, "running");
        assert!(store.claim_for_test(&claude_worker).unwrap().is_none());
        let codex_claimed = store.claim_for_test(&codex_worker).unwrap().unwrap();
        assert_eq!(codex_claimed.task_id, other.task_id);

        // Consumed tasks are immutable and uncancellable.
        assert!(store
            .update_worker_task_scope(&claimed.task_id, "too late")
            .is_err());
        assert!(store.cancel_worker_task(&claimed.task_id).is_err());
        // Enqueue refuses foreign workers.
        assert!(store
            .enqueue_worker_task(&run.team_run_id, "not-a-worker", "x", &[])
            .is_err());
        assert!(!store.has_queued_task(&claude_worker).unwrap());
    }

    #[test]
    fn queued_task_evidence_is_scoped_to_its_own_claim_not_the_queue() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "scope-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("scope-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        let worker = run.workers[0].clone();

        // Both tasks are queued up front — the second's created_at predates
        // the first's execution, which used to sweep in the first's evidence.
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "first", &[])
            .unwrap();
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "second", &[])
            .unwrap();

        let first = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
        let first_turn = store
            .prepare_resumed_turn(&worker.worker_session_id, "first")
            .unwrap();
        store
            .record_side_effect(
                &first_turn,
                SideEffectEvidence {
                    tool_call_id: "t-first".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(5),
                    path: Some("first.rs".into()),
                    tool_input: None,
                    tool_output: None,
                },
            )
            .unwrap();
        store
            .finish_turn(&first_turn, None, Some("first done"), None, None)
            .unwrap();
        let first = store
            .finish_worker_task(&first.task_id, true, Some("first done"))
            .unwrap();
        assert_eq!(first.changed_files, vec!["first.rs".to_string()]);

        let second = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
        let second_turn = store
            .prepare_resumed_turn(&worker.worker_session_id, "second")
            .unwrap();
        store
            .record_side_effect(
                &second_turn,
                SideEffectEvidence {
                    tool_call_id: "t-second".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(5),
                    path: Some("second.rs".into()),
                    tool_input: None,
                    tool_output: None,
                },
            )
            .unwrap();
        store
            .finish_turn(&second_turn, None, Some("second done"), None, None)
            .unwrap();
        let second = store
            .finish_worker_task(&second.task_id, true, Some("second done"))
            .unwrap();
        // Only its own work — the first task's file must not appear here.
        assert_eq!(second.changed_files, vec!["second.rs".to_string()]);
        assert_eq!(second.verification.len(), 1);
    }

    #[test]
    fn resource_claims_let_compatible_tasks_run_and_queue_conflicting_ones() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "claims-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        // Three workers sharing ONE workspace: the in-product Alva worker and
        // two external engines all pass the same product-level gate.
        let run = store
            .create_team_run("claims-lead", &EngineTarget::for_engine("alva").unwrap())
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("claude").unwrap(),
                None,
            )
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
                None,
            )
            .unwrap();
        let (alva_worker, claude_worker, codex_worker) = (
            run.workers[0].clone(),
            run.workers[1].clone(),
            run.workers[2].clone(),
        );

        let write_lib = ResourceClaim::Path {
            path: "src/lib.rs".into(),
            mode: ClaimMode::Write,
        };
        let write_docs = ResourceClaim::Path {
            path: "docs/guide.md".into(),
            mode: ClaimMode::Write,
        };
        let write_src_tree = ResourceClaim::Path {
            path: "src".into(),
            mode: ClaimMode::Write,
        };

        store
            .enqueue_worker_task(
                &run.team_run_id,
                &alva_worker.worker_id,
                "edit lib",
                std::slice::from_ref(&write_lib),
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &claude_worker.worker_id,
                "edit docs",
                std::slice::from_ref(&write_docs),
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &codex_worker.worker_id,
                "refactor the whole src tree",
                std::slice::from_ref(&write_src_tree),
            )
            .unwrap();

        // The Alva worker takes src/lib.rs.
        let alva_task = match store
            .claim_next_worker_task(&alva_worker.worker_id)
            .unwrap()
        {
            TaskClaim::Claimed(task) => *task,
            other => panic!("expected a claim, got {other:?}"),
        };
        // Disjoint docs work runs concurrently — no waiting.
        assert!(matches!(
            store
                .claim_next_worker_task(&claude_worker.worker_id)
                .unwrap(),
            TaskClaim::Claimed(_)
        ));
        // The src-tree refactor overlaps src/lib.rs, so it waits and names
        // both the resource and its holder.
        let blocked = store
            .claim_next_worker_task(&codex_worker.worker_id)
            .unwrap();
        match blocked {
            TaskClaim::Blocked { blocked_by, .. } => {
                assert_eq!(blocked_by.len(), 1);
                assert_eq!(blocked_by[0].resource, "write src");
                assert_eq!(blocked_by[0].holder_worker_id, alva_worker.worker_id);
                assert_eq!(blocked_by[0].holder_task_id, alva_task.task_id);
                store
                    .record_resource_wait(&run.team_run_id, &codex_worker.worker_id, &blocked_by)
                    .unwrap();
            }
            other => panic!("expected a block, got {other:?}"),
        }

        // The wait and its holder are visible on the lead timeline.
        let lead_detail = store.get_session("claims-lead").unwrap().unwrap();
        let wait = lead_detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "resource_wait")
            .expect("resource wait recorded");
        assert!(wait.reason.contains("write src"));
        assert!(wait.reason.contains(&alva_worker.worker_id));

        // Once the holder finishes, the blocked task claims deterministically.
        store
            .finish_worker_task(&alva_task.task_id, true, Some("done"))
            .unwrap();
        let unblocked = store
            .claim_next_worker_task(&codex_worker.worker_id)
            .unwrap();
        assert!(matches!(unblocked, TaskClaim::Claimed(_)));
    }

    #[test]
    fn an_older_waiter_reserves_its_resources_against_a_stream_of_short_tasks() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "fair-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("fair-lead", &EngineTarget::for_engine("alva").unwrap())
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("claude").unwrap(),
                None,
            )
            .unwrap();
        let (holder, waiter) = (run.workers[0].clone(), run.workers[1].clone());

        let narrow = ResourceClaim::Path {
            path: "src/a.rs".into(),
            mode: ClaimMode::Write,
        };
        let broad = ResourceClaim::Path {
            path: "src".into(),
            mode: ClaimMode::Write,
        };
        // The holder is already running something narrow.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &holder.worker_id,
                "first narrow edit",
                std::slice::from_ref(&narrow),
            )
            .unwrap();
        let first = match store.claim_next_worker_task(&holder.worker_id).unwrap() {
            TaskClaim::Claimed(task) => *task,
            other => panic!("expected a claim, got {other:?}"),
        };
        // The broad waiter arrives and is blocked.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &waiter.worker_id,
                "refactor the tree",
                std::slice::from_ref(&broad),
            )
            .unwrap();
        assert!(matches!(
            store.claim_next_worker_task(&waiter.worker_id).unwrap(),
            TaskClaim::Blocked { .. }
        ));
        // A newer narrow task queues behind on the holder.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &holder.worker_id,
                "second narrow edit",
                std::slice::from_ref(&narrow),
            )
            .unwrap();
        store
            .finish_worker_task(&first.task_id, true, Some("done"))
            .unwrap();

        // Without a reservation the newer narrow task would grab src/a.rs
        // again and starve the waiter. The older waiter wins instead.
        assert!(matches!(
            store.claim_next_worker_task(&holder.worker_id).unwrap(),
            TaskClaim::Blocked { .. }
        ));
        assert!(matches!(
            store.claim_next_worker_task(&waiter.worker_id).unwrap(),
            TaskClaim::Claimed(_)
        ));
    }

    #[test]
    fn isolated_workers_carry_worktree_identity_and_a_separate_workspace() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "iso-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("iso-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        // The shared worker has no worktree identity.
        assert!(run.workers[0].worktree_path.is_none());

        let handle = crate::worktree::WorktreeHandle {
            path: worktree.path().to_string_lossy().into_owned(),
            branch: "hub/worker-abc".into(),
        };
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
                Some(&handle),
            )
            .unwrap();
        let isolated = run.workers[1].clone();
        assert_eq!(
            isolated.worktree_path.as_deref(),
            Some(handle.path.as_str())
        );
        assert_eq!(isolated.worktree_branch.as_deref(), Some("hub/worker-abc"));
        // Its durable session runs in the worktree, not the shared workspace.
        let session = store
            .get_session(&isolated.worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(session.session.workspace, handle.path);
        assert_ne!(
            session.session.workspace,
            workspace.path().to_string_lossy()
        );
        // An isolated worker in its own workspace never contends with the
        // shared workers' claims.
        let claim = ResourceClaim::Path {
            path: "src/lib.rs".into(),
            mode: ClaimMode::Write,
        };
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "shared edit",
                std::slice::from_ref(&claim),
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &isolated.worker_id,
                "isolated edit",
                std::slice::from_ref(&claim),
            )
            .unwrap();
        assert!(matches!(
            store
                .claim_next_worker_task(&run.workers[0].worker_id)
                .unwrap(),
            TaskClaim::Claimed(_)
        ));
        assert!(matches!(
            store.claim_next_worker_task(&isolated.worker_id).unwrap(),
            TaskClaim::Claimed(_)
        ));

        // Adoption and discard are recorded as lead-visible decisions.
        store
            .record_adoption(
                &run.team_run_id,
                &isolated.worker_id,
                true,
                "Adopted 2 files",
            )
            .unwrap();
        store
            .record_adoption(&run.team_run_id, &isolated.worker_id, false, "Discarded")
            .unwrap();
        let lead_detail = store.get_session("iso-lead").unwrap().unwrap();
        assert!(lead_detail
            .timeline_events
            .iter()
            .any(|event| event.kind == "worktree_adopted"));
        assert!(lead_detail
            .timeline_events
            .iter()
            .any(|event| event.kind == "worktree_discarded"));

        // Discarding the worktree keeps the durable worker session AND
        // repoints it at the shared workspace, so later runs still work.
        let shared = workspace.path().to_string_lossy().into_owned();
        store
            .clear_worker_worktree(&isolated.worker_id, &shared)
            .unwrap();
        let run = store.get_team_run(&run.team_run_id).unwrap().unwrap();
        assert!(run.workers[1].worktree_path.is_none());
        let session = store
            .get_session(&isolated.worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(session.session.workspace, shared);
        assert_ne!(session.session.workspace, handle.path);
    }

    #[test]
    fn worker_lifecycle_survives_restart_and_never_conflates_archive_with_delete() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "life-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("life-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        let worker = run.workers[0].clone();
        assert_eq!(worker.lifecycle, WorkerLifecycle::Active);

        // History and a pending queue entry to preserve across states.
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "done work", &[])
            .unwrap();
        let first = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
        store
            .finish_worker_task(&first.task_id, true, Some("finished"))
            .unwrap();
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "still queued", &[])
            .unwrap();

        // Hibernate: identity, history and queue all stay; nothing is
        // consumed while the runtime is released.
        store
            .set_worker_lifecycle(&worker.worker_id, WorkerLifecycle::Hibernated)
            .unwrap();
        // "Dormant", never "Empty": the pump must be able to tell a parked
        // queue from a drained one, or its respawn check spins forever.
        assert!(matches!(
            store.claim_next_worker_task(&worker.worker_id).unwrap(),
            TaskClaim::Dormant
        ));
        assert!(store.claim_for_test(&worker.worker_id).unwrap().is_none());
        // Restart resumes pumps only for ACTIVE workers with queued work.
        assert!(!store
            .active_workers_with_queued_tasks()
            .unwrap()
            .contains(&worker.worker_id));
        let hibernated = store.get_team_run(&run.team_run_id).unwrap().unwrap();
        assert_eq!(hibernated.workers[0].lifecycle, WorkerLifecycle::Hibernated);
        assert_eq!(hibernated.tasks.len(), 2);
        assert!(store
            .get_session(&worker.worker_session_id)
            .unwrap()
            .is_some());
        assert!(store.has_queued_task(&worker.worker_id).unwrap());

        // A new task wakes it, and the older queued task is still first.
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "new work", &[])
            .unwrap();
        assert!(store
            .active_workers_with_queued_tasks()
            .unwrap()
            .contains(&worker.worker_id));
        let resumed = store.team_worker(&worker.worker_id).unwrap().unwrap();
        assert_eq!(resumed.lifecycle, WorkerLifecycle::Active);
        let claimed = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
        assert_eq!(claimed.scope, "still queued");

        // Ending the run archives — never deletes.
        let completed = store.complete_team_run(&run.team_run_id).unwrap();
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.workers[0].lifecycle, WorkerLifecycle::Archived);
        assert_eq!(completed.tasks.len(), 3);
        // Archived workers need an explicit restore, not an implicit wake.
        let refused = store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "sneak in", &[])
            .unwrap_err();
        assert!(refused.contains("archived"));
        // An ended run neither accepts new workers nor re-completes (which
        // would silently re-archive anything the user restored).
        let refused = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
                None,
            )
            .unwrap_err();
        assert!(refused.contains("has ended"));
        assert!(store.complete_team_run(&run.team_run_id).is_err());

        // Everything survives a restart: links, queue, history, lifecycle.
        drop(store);
        let reopened = reopen(&database);
        let run = reopened.team_run_for_lead("life-lead").unwrap().unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(run.workers[0].lifecycle, WorkerLifecycle::Archived);
        assert_eq!(run.workers[0].worker_id, worker.worker_id);
        assert_eq!(run.tasks.len(), 3);
        assert!(reopened
            .get_session(&worker.worker_session_id)
            .unwrap()
            .is_some());

        // Delete is a separate, explicit act — and only from archived.
        reopened
            .set_worker_lifecycle(&worker.worker_id, WorkerLifecycle::Active)
            .unwrap();
        assert!(reopened.delete_worker(&worker.worker_id).is_err());
        reopened
            .set_worker_lifecycle(&worker.worker_id, WorkerLifecycle::Archived)
            .unwrap();
        reopened.delete_worker(&worker.worker_id).unwrap();
        assert!(reopened.team_worker(&worker.worker_id).unwrap().is_none());
    }

    #[test]
    fn fallback_is_local_to_one_session_and_its_handoff_carries_team_context() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        let lead = store
            .prepare_new_turn(
                "local-lead",
                workspace.path(),
                "plan the sprint",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("local-lead", &EngineTarget::for_engine("codex").unwrap())
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("claude").unwrap(),
                None,
            )
            .unwrap();
        let (first_worker, second_worker) = (run.workers[0].clone(), run.workers[1].clone());
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &first_worker.worker_id,
                "build parser",
                &[],
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &first_worker.worker_id,
                "then docs with API_KEY=sk-live-scopecanary",
                &[],
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &second_worker.worker_id,
                "sibling work",
                &[],
            )
            .unwrap();
        let first_task = store
            .claim_for_test(&first_worker.worker_id)
            .unwrap()
            .unwrap();
        // Evidence produced by the running task, before it finished.
        let evidence_turn = store
            .prepare_resumed_turn(&first_worker.worker_session_id, "build parser")
            .unwrap();
        store
            .record_side_effect(
                &evidence_turn,
                SideEffectEvidence {
                    tool_call_id: "t-live".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(3),
                    path: Some("src/parser.rs".into()),
                    tool_input: None,
                    tool_output: None,
                },
            )
            .unwrap();
        store
            .finish_turn(&evidence_turn, None, Some("partial"), None, None)
            .unwrap();
        let sibling_task = store
            .claim_for_test(&second_worker.worker_id)
            .unwrap()
            .unwrap();

        // The LEAD falls back: the run, both workers and their queues stay.
        store
            .set_session_fallback_policy("local-lead", vec![claude.clone()])
            .unwrap();
        let lead_turn = store
            .prepare_resumed_turn("local-lead", "keep planning")
            .unwrap();
        let lead_fallback = store
            .fallback_failed_attempt(&lead_turn, "lead engine died", &claude)
            .unwrap();
        let run_after = store.get_team_run(&run.team_run_id).unwrap().unwrap();
        assert_eq!(run_after.team_run_id, run.team_run_id);
        assert_eq!(run_after.workers.len(), 2);
        assert_eq!(run_after.tasks.len(), 3);
        assert_eq!(run_after.tasks[0].status, "running");
        assert!(store.has_queued_task(&first_worker.worker_id).unwrap());
        // The lead's handoff tells the new engine it leads a live team.
        let snapshot = lead_fallback
            .handoff
            .as_ref()
            .unwrap()
            .team_snapshot
            .clone()
            .unwrap();
        assert!(snapshot.contains("Lead of TeamRun"));
        assert!(snapshot.contains("2 worker(s)"));
        assert!(snapshot.contains("Workers keep running independently"));
        // The fallback fact belongs to the LEAD session, not a worker.
        let lead_detail = store.get_session("local-lead").unwrap().unwrap();
        assert!(lead_detail
            .timeline_events
            .iter()
            .any(|event| event.kind == "engine_fallback"));
        for worker in [&first_worker, &second_worker] {
            let detail = store
                .get_session(&worker.worker_session_id)
                .unwrap()
                .unwrap();
            assert!(detail
                .timeline_events
                .iter()
                .all(|event| event.kind != "engine_fallback"));
        }

        // Now a WORKER falls back: the lead and the sibling are untouched.
        store
            .set_session_fallback_policy(&first_worker.worker_session_id, vec![claude.clone()])
            .unwrap();
        let worker_turn = store
            .prepare_resumed_turn(&first_worker.worker_session_id, &first_task.scope)
            .unwrap();
        let worker_fallback = store
            .fallback_failed_attempt(&worker_turn, "worker engine died", &claude)
            .unwrap();
        let worker_snapshot = worker_fallback
            .handoff
            .as_ref()
            .unwrap()
            .team_snapshot
            .clone()
            .unwrap();
        assert!(worker_snapshot.contains("Worker (active) of TeamRun"));
        // Its own task and its remaining queue travel with the handoff.
        assert!(worker_snapshot.contains("build parser"));
        assert!(worker_snapshot.contains("1 queued: then docs"));
        assert!(worker_snapshot.contains("sibling workers keep running"));
        // In-flight evidence really travels (a running task has no
        // changed_files yet, so this must come from the live attempts).
        assert!(
            worker_snapshot.contains("files touched so far: src/parser.rs"),
            "{worker_snapshot}"
        );
        // A user-authored scope crossing an engine boundary is filtered.
        assert!(!worker_snapshot.contains("sk-live-scopecanary"));
        assert!(worker_snapshot.contains("[redacted]"));
        // The sibling's task is still running and its session untouched.
        let sibling = store
            .worker_task_by_id(&sibling_task.task_id)
            .unwrap()
            .unwrap();
        assert_eq!(sibling.status, "running");
        let sibling_detail = store
            .get_session(&second_worker.worker_session_id)
            .unwrap()
            .unwrap();
        assert!(sibling_detail
            .timeline_events
            .iter()
            .all(|event| event.kind != "engine_fallback"));
        assert_eq!(sibling_detail.session.engine_id, "claude");
        // Only the falling-back worker changed engine.
        let first_detail = store
            .get_session(&first_worker.worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(first_detail.session.engine_id, "claude");
        assert!(first_detail
            .timeline_events
            .iter()
            .any(|event| event.kind == "engine_fallback"));
    }

    #[test]
    fn team_creation_inherits_permissions_and_never_widens_them() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        // An interactive session: only alva can honour it (claude/codex can
        // merely weaken it), per the adapter mapping contract.
        let lead = store
            .prepare_new_turn(
                "perm-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::Interactive,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();

        // Creating a worker is NOT a way around the profile.
        let refused = store
            .create_team_run("perm-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap_err();
        assert!(refused.contains("cannot honour"), "{refused}");

        let run = store
            .create_team_run("perm-lead", &EngineTarget::for_engine("alva").unwrap())
            .unwrap();
        // Autonomy is off until the user says otherwise.
        assert!(!run.autonomous);
        // The worker inherited the lead's profile verbatim.
        let worker_session = store
            .get_session(&run.workers[0].worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            worker_session.session.permission_profile,
            PermissionProfile::Interactive
        );
        // Adding a weaker-only engine is refused too.
        let refused = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
                None,
            )
            .unwrap_err();
        assert!(refused.contains("cannot honour"), "{refused}");

        // Autonomy is an explicit, persisted, independently visible choice.
        let autonomous = store.set_team_autonomy(&run.team_run_id, true).unwrap();
        assert!(autonomous.autonomous);
        // ...and it does not touch anyone's PermissionProfile.
        assert_eq!(
            store
                .get_session(&run.workers[0].worker_session_id)
                .unwrap()
                .unwrap()
                .session
                .permission_profile,
            PermissionProfile::Interactive
        );
        assert!(
            !store
                .set_team_autonomy(&run.team_run_id, false)
                .unwrap()
                .autonomous
        );

        // Decisions are auditable on the session they covered.
        store
            .record_permission_decision("perm-lead", "Allowed 'write_file' on src/lib.rs")
            .unwrap();
        let detail = store.get_session("perm-lead").unwrap().unwrap();
        let audit = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "permission_decision")
            .expect("approval audit recorded");
        assert!(audit.reason.contains("write_file"));
    }

    #[test]
    fn reopening_repairs_a_worker_task_left_running_by_a_crash() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "crash-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("crash-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        store
            .enqueue_worker_task(&run.team_run_id, &run.workers[0].worker_id, "work", &[])
            .unwrap();
        let claimed = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.status, "running");
        // Simulate a crash: the process dies with the task still running.
        drop(store);

        let reopened = reopen(&database);
        let task = reopened
            .worker_task_by_id(&claimed.task_id)
            .unwrap()
            .unwrap();
        // A crashed task must not stay "running" forever — nothing would
        // ever finish it, and the worker's queue would stall behind it.
        assert_eq!(task.status, "interrupted");
        assert!(task.completed_at.is_some());
        assert!(task.result.is_some());
    }

    #[test]
    fn a_solo_session_handoff_carries_no_team_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let prepared = store
            .prepare_new_turn(
                "solo",
                workspace.path(),
                "just me",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        store
            .request_engine_switch("solo", &EngineTarget::for_engine("claude").unwrap(), "try")
            .unwrap();
        let detail = store.get_session("solo").unwrap().unwrap();
        assert!(detail.handoffs.last().unwrap().team_snapshot.is_none());
    }

    #[test]
    fn tasks_in_different_workspaces_never_contend() {
        let root = tempfile::tempdir().unwrap();
        let first_workspace = tempfile::tempdir().unwrap();
        let second_workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let same_file = ResourceClaim::Path {
            path: "src/lib.rs".into(),
            mode: ClaimMode::Write,
        };
        let mut workers = Vec::new();
        for (index, workspace) in [first_workspace.path(), second_workspace.path()]
            .into_iter()
            .enumerate()
        {
            let lead_id = format!("ws-lead-{index}");
            let lead = store
                .prepare_new_turn(
                    &lead_id,
                    workspace,
                    "plan",
                    &alva,
                    PermissionProfile::WorkspaceWrite,
                )
                .unwrap();
            store
                .finish_turn(&lead, None, Some("planned"), None, None)
                .unwrap();
            let run = store
                .create_team_run(&lead_id, &EngineTarget::for_engine("claude").unwrap())
                .unwrap();
            store
                .enqueue_worker_task(
                    &run.team_run_id,
                    &run.workers[0].worker_id,
                    "edit lib",
                    std::slice::from_ref(&same_file),
                )
                .unwrap();
            workers.push(run.workers[0].worker_id.clone());
        }
        // Identical claims, different workspaces — both run.
        for worker_id in workers {
            assert!(matches!(
                store.claim_next_worker_task(&worker_id).unwrap(),
                TaskClaim::Claimed(_)
            ));
        }
    }

    #[test]
    fn latest_attempt_outcome_reports_terminal_evidence_per_engine() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        assert_eq!(store.latest_attempt_outcome("alva").unwrap(), None);

        let failed = store
            .prepare_new_turn(
                "product-outcome",
                workspace.path(),
                "first",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&failed, None, None, Some("stream ended early"), None)
            .unwrap();
        assert_eq!(
            store.latest_attempt_outcome("alva").unwrap(),
            Some(("failed".to_string(), Some("stream ended early".to_string())))
        );

        let completed = store
            .prepare_resumed_turn("product-outcome", "second")
            .unwrap();
        store
            .finish_turn(&completed, None, Some("done"), None, None)
            .unwrap();
        assert_eq!(
            store.latest_attempt_outcome("alva").unwrap(),
            Some(("completed".to_string(), None))
        );
        assert_eq!(store.latest_attempt_outcome("claude").unwrap(), None);
    }

    #[test]
    fn session_memory_persists_across_reopen_and_never_crosses_sessions() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        store
            .prepare_new_turn(
                "memory-a",
                workspace.path(),
                "start",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .prepare_new_turn(
                "memory-b",
                workspace.path(),
                "start",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();

        // Empty until taught; entries land in insertion order.
        assert_eq!(
            store.get_session("memory-a").unwrap().unwrap().memory,
            Vec::<String>::new()
        );
        let updated = store
            .add_session_memory("memory-a", "always run cargo test before finishing")
            .unwrap();
        assert_eq!(
            updated,
            vec!["always run cargo test before finishing".to_string()]
        );
        store
            .add_session_memory("memory-a", "never force-push to main")
            .unwrap();

        // The sibling session stays untouched — memory is per-session.
        assert_eq!(
            store.get_session("memory-b").unwrap().unwrap().memory,
            Vec::<String>::new()
        );
        // Whitespace-only entries are rejected.
        assert!(store.add_session_memory("memory-a", "   ").is_err());
        // Unknown sessions are rejected, not silently created.
        assert!(store.add_session_memory("memory-ghost", "x").is_err());

        // Reopen: memory survives restart (it must, to ride future handoffs).
        drop(store);
        let reopened = reopen(&database);
        assert_eq!(
            reopened.get_session("memory-a").unwrap().unwrap().memory,
            vec![
                "always run cargo test before finishing".to_string(),
                "never force-push to main".to_string()
            ]
        );
    }

    #[test]
    fn handoff_snapshot_carries_session_memory_and_filters_it() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        store
            .prepare_new_turn(
                "memory-handoff",
                workspace.path(),
                "start",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .add_session_memory(
                "memory-handoff",
                "always run tests; never touch API_KEY=sk-live-memory999",
            )
            .unwrap();

        let mut connection = store.lock().unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let package = build_handoff_package(
            &transaction,
            "memory-handoff",
            "b-alva",
            "b-claude",
            alva,
            EngineTarget::for_engine("claude").unwrap(),
            "test reason",
            now_unix_ms(),
            HandoffKind::Full,
            None,
        )
        .unwrap();
        transaction.commit().unwrap();

        // The memory entry rode the handoff, secret filtered.
        assert_eq!(package.session_memory.len(), 1);
        assert!(package.session_memory[0].contains("always run tests"));
        assert!(!package.session_memory[0].contains("sk-live-memory999"));
        // And the persisted handoff is what the target adapter receives.
        let encoded = package.render_for_engine();
        assert!(encoded.contains("session_memory"));
        assert!(!encoded.contains("sk-live-memory999"));
    }

    #[test]
    fn handoff_lists_workspace_skills_without_shipping_bodies() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        // Two skills with frontmatter descriptions, one junk directory.
        let skills = workspace.path().join(".alva").join("skills");
        std::fs::create_dir_all(skills.join("review")).unwrap();
        std::fs::write(
            skills.join("review/SKILL.md"),
            "---\ndescription: Review every change before landing\n---\n# Review\n",
        )
        .unwrap();
        std::fs::create_dir_all(skills.join("wip")).unwrap();
        std::fs::write(
            skills.join("wip/SKILL.md"),
            "no frontmatter, first line wins\n",
        )
        .unwrap();
        std::fs::create_dir_all(skills.join("notes")).unwrap();
        std::fs::write(skills.join("notes/README.txt"), "not a skill").unwrap();

        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        store
            .prepare_new_turn(
                "skills-session",
                workspace.path(),
                "start",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();

        let mut connection = store.lock().unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let package = build_handoff_package(
            &transaction,
            "skills-session",
            "b-alva",
            "b-claude",
            alva,
            EngineTarget::for_engine("claude").unwrap(),
            "switch",
            now_unix_ms(),
            HandoffKind::Full,
            None,
        )
        .unwrap();
        transaction.commit().unwrap();

        let mut names: Vec<_> = package
            .available_skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["review", "wip"]);
        let review = package
            .available_skills
            .iter()
            .find(|skill| skill.name == "review")
            .unwrap();
        assert_eq!(review.description, "Review every change before landing");
        assert_eq!(review.path, ".alva/skills/review/SKILL.md");
        let wip = package
            .available_skills
            .iter()
            .find(|skill| skill.name == "wip")
            .unwrap();
        assert_eq!(wip.description, "no frontmatter, first line wins");
        // The handoff renders the pointers, never the bodies.
        let encoded = package.render_for_engine();
        assert!(encoded.contains(".alva/skills/review/SKILL.md"));
        assert!(!encoded.contains("Review every change before landing\n---"));
    }

    #[test]
    fn handoff_lists_workspace_mcp_servers_without_credentials() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        // Alva shape: {servers: {id: {transport: {type, command, args, env}}}}
        std::fs::write(
            workspace.path().join(".mcp.json"),
            r#"{
              "servers": {
                "filesystem": {
                  "display_name": "Filesystem",
                  "transport": {
                    "type": "stdio",
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem"],
                    "env": {"TOKEN": "sk-secret-never-shown"}
                  }
                },
                "remote": {
                  "transport": {
                    "type": "sse",
                    "url": "https://mcp.example.com/sse",
                    "headers": {"Authorization": "Bearer sk-secret-never-shown"}
                  }
                }
              }
            }"#,
        )
        .unwrap();
        // Ecosystem shape in mcp.json: entries are the transport directly.
        std::fs::write(
            workspace.path().join("mcp.json"),
            r#"{"mcpServers": {"github": {"command": "npx", "args": ["-y", "@x/github-mcp"]}}}"#,
        )
        .unwrap();

        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        store
            .prepare_new_turn(
                "mcp-session",
                workspace.path(),
                "start",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();

        let mut connection = store.lock().unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let package = build_handoff_package(
            &transaction,
            "mcp-session",
            "b-alva",
            "b-claude",
            alva,
            EngineTarget::for_engine("claude").unwrap(),
            "switch",
            now_unix_ms(),
            HandoffKind::Full,
            None,
        )
        .unwrap();
        transaction.commit().unwrap();

        let mut ids: Vec<_> = package
            .available_mcp
            .iter()
            .map(|server| server.id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["filesystem", "github", "remote"]);
        let filesystem = package
            .available_mcp
            .iter()
            .find(|server| server.id == "filesystem")
            .unwrap();
        assert_eq!(filesystem.display_name, "Filesystem");
        assert_eq!(
            filesystem.transport,
            "stdio: npx -y @modelcontextprotocol/server-filesystem"
        );
        // env/headers never cross: the rendered handoff has no secret, and
        // the transport lines carry no credential material.
        let encoded = package.render_for_engine();
        assert!(!encoded.contains("sk-secret-never-shown"));
        assert!(encoded.contains("sse: https://mcp.example.com/sse"));
    }

    #[test]
    fn session_list_is_ordered_most_recent_first() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();

        let first = store
            .prepare_new_turn(
                "list-a",
                workspace.path(),
                "oldest",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = store
            .prepare_new_turn(
                "list-b",
                workspace.path(),
                "middle",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let third = store
            .prepare_new_turn(
                "list-c",
                workspace.path(),
                "newest",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store.finish_turn(&third, None, None, None, None).unwrap();

        let sessions = store.list_sessions().unwrap();
        let ids: Vec<_> = sessions
            .iter()
            .map(|session| session.product_session_id.as_str())
            .collect();
        // The finished session is the newest by updated_at — it must lead.
        assert_eq!(ids[0], "list-c");
        assert_eq!(ids[1], "list-b");
        assert_eq!(ids[2], "list-a");
        let _ = (first, second);
    }
}
