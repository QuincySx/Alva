// INPUT:  process argv/env, cli grammar, session_store, alva_hub_core registry, adapters
// OUTPUT: alva-hub binary with bounded turns, durable resume, and metadata-only handoff
// POS:    Production headless Hub dispatcher with fail-closed lifecycle and output contracts.

mod cli;
mod handoff;
mod session_store;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory, SessionRepository};
use alva_engine_adapter_claude::{
    ClaudeAdapter, ClaudeAdapterConfig, ClaudePrintAdapter, ClaudePrintConfig,
    ClaudePrintPermissionMode,
};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeEvent, RuntimeOptions, RuntimeRequest,
};
use alva_hub_core::{HubEventStream, HubRequest, HubRuntime, HubSessionStatus, ProductSessionId};
use tokio_stream::StreamExt;

use crate::cli::{parse_args, ClaudeMode, Engine, ParseOutcome, PermissionPolicy};
use crate::session_store::{now_unix, SessionStore, StoredSession};

const USAGE_EXIT: i32 = 2;
const INTERRUPTED_EXIT: i32 = 130;
const TIMEOUT_EXIT: i32 = 124;
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
const CANCEL_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FINAL_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to start Tokio runtime");
    let exit_code = runtime.block_on(run());
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

async fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match parse_args(&args) {
        Ok(ParseOutcome::Help) => {
            print_usage();
            return 0;
        }
        Ok(ParseOutcome::Version) => {
            println!("alva-hub {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        Ok(ParseOutcome::Run(invocation)) => invocation,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            eprintln!("Try 'alva-hub --help'.");
            return USAGE_EXIT;
        }
    };

    let Some(store) = SessionStore::open_default() else {
        eprintln!("alva-hub: no private session journal location is available; set ALVA_HUB_HOME");
        return 1;
    };
    if let Err(error) = store.prepare() {
        eprintln!("alva-hub: session journal is not ready: {error}");
        return 1;
    }
    if invocation.verbose {
        eprintln!("alva-hub: session journal {}", store.path().display());
    }

    let mut engine = invocation.engine;
    let mut claude_mode = invocation.claude_mode;
    let mut workspace = match resolve_workspace(invocation.cwd.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            return USAGE_EXIT;
        }
    };

    // Optional rehydrate from disk. Explicit `--engine` different from the
    // journal means handoff (switch_engine), not hard failure.
    let mut resume_product: Option<ProductSessionId> = None;
    let mut handoff_from: Option<StoredSession> = None;
    let mut resume_record: Option<StoredSession> = None;
    if let Some(session_id) = invocation.session.as_deref() {
        match rehydrate_session(
            &store,
            session_id,
            &mut engine,
            &mut claude_mode,
            &mut workspace,
            invocation.cwd.is_some(),
            invocation.engine_explicit,
        ) {
            Ok(plan) => {
                resume_product = Some(plan.product_id);
                handoff_from = plan.handoff_from;
                resume_record = Some(plan.record);
            }
            Err(error) => {
                eprintln!("alva-hub: {error}");
                return USAGE_EXIT;
            }
        }
    }
    if engine == Engine::Codex && invocation.max_turns.is_some() {
        eprintln!(
            "alva-hub: Codex exec does not expose a compatible --max-turns control; omit the flag or select another engine"
        );
        return USAGE_EXIT;
    }

    let hub = HubRuntime::new();
    let target_engine_id = engine.id();

    // Register target engine; if handoff, also register journal engine so we
    // can restore then switch.
    if let Some(ref rec) = handoff_from {
        if rec.engine_id != target_engine_id {
            let from = parse_engine_id(&rec.engine_id);
            if let Some(from_eng) = from {
                let from_mode = rec
                    .claude_mode
                    .as_deref()
                    .and_then(parse_claude_mode_str)
                    .unwrap_or(ClaudeMode::Light);
                if let Err(error) = ensure_engine_registered(
                    &hub,
                    from_eng,
                    from_mode,
                    PermissionPolicy::Deny,
                    &workspace,
                ) {
                    eprintln!("alva-hub: {error}");
                    return USAGE_EXIT;
                }
            }
        }
    }
    if let Err(error) =
        ensure_engine_registered(&hub, engine, claude_mode, invocation.permission, &workspace)
    {
        eprintln!("alva-hub: {error}");
        return USAGE_EXIT;
    }

    // Seed Hub routing map from the journal, then hand off if requested.
    if let Some(ref product_id) = resume_product {
        let Some(rec) = resume_record.as_ref() else {
            eprintln!("alva-hub: internal error: rehydrated session record is missing");
            return 1;
        };
        if let Err(error) = hub.restore_session(
            product_id.clone(),
            rec.engine_id.as_str(),
            rec.engine_session_id.clone(),
            HubSessionStatus::Completed,
        ) {
            eprintln!("alva-hub: cannot restore session: {error}");
            return 1;
        }
        eprintln!(
            "alva-hub: restored product_session_id={} engine={} engine_session={}",
            product_id,
            rec.engine_id,
            safe_diagnostic_text(&rec.engine_session_id, 160)
        );

        if handoff_from.is_some() && rec.engine_id != target_engine_id {
            let original = match persist_handoff_intent(&store, product_id, target_engine_id) {
                Ok(original) => original,
                Err(error) => {
                    eprintln!("alva-hub: cannot persist handoff intent: {error}");
                    return 1;
                }
            };
            match hub.switch_engine(product_id, target_engine_id) {
                Ok(_) => {
                    eprintln!(
                        "alva-hub: switched engine {} → {} (durable intent recorded; fresh native session)",
                        rec.engine_id, target_engine_id
                    );
                }
                Err(error) => {
                    if let Err(rollback_error) = store.upsert(original) {
                        eprintln!(
                            "alva-hub: cannot roll back failed handoff intent: {rollback_error}"
                        );
                    }
                    eprintln!("alva-hub: cannot switch engine: {error}");
                    return 1;
                }
            }
        } else if rec.engine_id == "alva" {
            eprintln!(
                "alva-hub: resuming Alva from history store (rebuilt agent + restored messages)"
            );
        }
    }

    let mut handoff_context_next = None;
    if let (Some(ref product_id), Some(ref from_rec)) = (&resume_product, &handoff_from) {
        if from_rec.engine_id != target_engine_id {
            // switch_engine already ran above when handoff_from was set.
            let _ = product_id;
            handoff_context_next = Some(crate::handoff::build_handoff_context(
                &from_rec.engine_id,
                target_engine_id,
            ));
        }
    }

    let mut opts = TurnOptions {
        verbose: invocation.verbose,
        permission: invocation.permission,
        workspace: workspace.clone(),
        engine_id: target_engine_id.to_string(),
        claude_mode,
        store,
        handoff_context_next,
        timeout: Duration::from_secs(
            invocation
                .timeout_seconds
                .unwrap_or(DEFAULT_TIMEOUT_SECONDS),
        ),
        max_turns: invocation.max_turns,
    };

    if invocation.repl {
        return run_repl(&hub, &mut opts, invocation.prompt.trim(), resume_product).await;
    }

    let engine_id = opts.engine_id.clone();
    let outcome = run_turn(
        &hub,
        &engine_id,
        resume_product,
        &invocation.prompt,
        &mut opts,
    )
    .await;
    outcome.exit_code
}

struct RehydratePlan {
    product_id: ProductSessionId,
    /// Set when the user requested a different engine than the journal.
    handoff_from: Option<StoredSession>,
    record: StoredSession,
}

/// Load journal entry and align engine/workspace selection.
fn rehydrate_session(
    store: &SessionStore,
    session_id: &str,
    engine: &mut Engine,
    claude_mode: &mut ClaudeMode,
    workspace: &mut PathBuf,
    cwd_explicit: bool,
    engine_explicit: bool,
) -> Result<RehydratePlan, String> {
    let rec = store
        .get(session_id)?
        .ok_or_else(|| format!("unknown product session '{session_id}' in journal"))?;

    let mut handoff_from = None;
    let source_engine = parse_engine_id(&rec.engine_id)
        .ok_or_else(|| format!("journal engine '{}' is not supported", rec.engine_id))?;
    let pending_target = rec
        .pending_handoff_target
        .as_deref()
        .map(|id| {
            parse_engine_id(id)
                .ok_or_else(|| format!("journal handoff target '{id}' is not supported"))
        })
        .transpose()?;

    if !engine_explicit {
        *engine = pending_target.unwrap_or(source_engine);
    }
    if engine.id() != rec.engine_id {
        handoff_from = Some(rec.clone());
    }

    if *engine == Engine::Claude {
        if let Some(mode) = rec.claude_mode.as_deref().and_then(parse_claude_mode_str) {
            *claude_mode = mode;
        }
    }

    if !cwd_explicit {
        let path = PathBuf::from(&rec.workspace);
        if path.is_dir() {
            *workspace = path.canonicalize().unwrap_or(path);
        } else {
            eprintln!(
                "alva-hub: journal workspace missing ({}), using current workspace",
                rec.workspace
            );
        }
    }

    if rec.engine_session_id.trim().is_empty() {
        return Err("journal entry has empty engine_session_id".into());
    }

    Ok(RehydratePlan {
        product_id: ProductSessionId::from_existing(session_id),
        handoff_from,
        record: rec,
    })
}

fn parse_engine_id(id: &str) -> Option<Engine> {
    match id {
        "alva" => Some(Engine::Alva),
        "claude" => Some(Engine::Claude),
        "codex" => Some(Engine::Codex),
        _ => None,
    }
}

fn parse_claude_mode_str(mode: &str) -> Option<ClaudeMode> {
    match mode {
        "light" | "print" | "cli" | "p" => Some(ClaudeMode::Light),
        "full" | "sdk" => Some(ClaudeMode::Full),
        _ => None,
    }
}

/// Persist the source route plus target intent before mutating Hub's in-memory
/// routing map. A crash can therefore replay the handoff instead of losing it.
fn persist_handoff_intent(
    store: &SessionStore,
    product_id: &ProductSessionId,
    target_engine_id: &str,
) -> Result<StoredSession, String> {
    let original = store
        .get(product_id.as_str())?
        .ok_or_else(|| format!("session '{}' is missing from the journal", product_id))?;
    if original.engine_session_id.trim().is_empty() {
        return Err("source engine session id is empty".into());
    }
    let mut pending = original.clone();
    pending.pending_handoff_target = Some(target_engine_id.to_string());
    pending.updated_at = now_unix();
    store.upsert(pending)?;
    Ok(original)
}

fn ensure_engine_registered(
    hub: &HubRuntime,
    engine: Engine,
    claude_mode: ClaudeMode,
    permission: PermissionPolicy,
    workspace: &std::path::Path,
) -> Result<(), String> {
    let id = engine.id();
    if hub.engine_ids().iter().any(|e| e.as_str() == id) {
        return Ok(());
    }
    let runtime = build_selected_engine(engine, claude_mode, permission, workspace)?;
    hub.register_engine(id, runtime).map_err(|e| e.to_string())
}

struct TurnOptions {
    verbose: bool,
    permission: PermissionPolicy,
    workspace: PathBuf,
    engine_id: String,
    claude_mode: ClaudeMode,
    store: SessionStore,
    /// Consumed once on the next `run_turn` after `switch_engine`.
    handoff_context_next: Option<String>,
    timeout: Duration,
    max_turns: Option<u32>,
}

struct TurnOutcome {
    exit_code: i32,
    product_session_id: Option<ProductSessionId>,
    /// A deadline, interrupt, or signal-handler failure may leave a detached
    /// terminal reader; the REPL must terminate instead of racing it for stdin.
    terminate_repl: bool,
}

async fn run_repl(
    hub: &HubRuntime,
    opts: &mut TurnOptions,
    initial_prompt: &str,
    mut product_session: Option<ProductSessionId>,
) -> i32 {
    eprintln!(
        "alva-hub: multi-turn REPL (engine={}). Commands: /quit /session /sessions /engine /help",
        opts.engine_id
    );
    eprintln!("alva-hub: session journal {}", opts.store.path().display());

    let mut last_exit = 0i32;

    if !initial_prompt.is_empty() {
        let engine_id = opts.engine_id.clone();
        let outcome = run_turn(
            hub,
            &engine_id,
            product_session.clone(),
            initial_prompt,
            opts,
        )
        .await;
        last_exit = outcome.exit_code;
        let terminate_repl = outcome.terminate_repl;
        if let Some(id) = outcome.product_session_id {
            product_session = Some(id);
        }
        if terminate_repl {
            return last_exit;
        }
    }

    loop {
        eprint!("alva-hub> ");
        let _ = io::stderr().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(error) => {
                eprintln!("alva-hub: stdin error: {error}");
                return 1;
            }
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "/quit" || line == "/exit" || line == "/q" {
            break;
        }
        if line == "/help" {
            eprintln!(
                "alva-hub: type a prompt to continue the session.\n  \
                 /session            print current product session id\n  \
                 /sessions           list recent journal sessions\n  \
                 /engine alva|claude|codex   hand off product session to another engine\n  \
                 /quit               leave REPL"
            );
            continue;
        }
        if line == "/session" {
            match &product_session {
                Some(id) => {
                    if let Some(snap) = hub.session(id) {
                        eprintln!(
                            "alva-hub: product_session_id={id} engine={} engine_session={:?}",
                            snap.engine_id,
                            snap.engine_session_id
                                .as_deref()
                                .map(|value| safe_diagnostic_text(value, 160))
                        );
                        if let Some(prev) = snap.previous_engine_id {
                            eprintln!(
                                "alva-hub: previous_engine={prev} previous_engine_session={:?}",
                                snap.previous_engine_session_id
                                    .as_deref()
                                    .map(|value| safe_diagnostic_text(value, 160))
                            );
                        }
                    } else {
                        eprintln!("alva-hub: product_session_id={id}");
                    }
                }
                None => eprintln!("alva-hub: no product session yet (send a prompt first)"),
            }
            continue;
        }
        if line == "/sessions" {
            if !list_sessions(&opts.store) {
                last_exit = 1;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("/engine") {
            let name = rest.trim();
            if name.is_empty() {
                eprintln!(
                    "alva-hub: current engine={} (usage: /engine alva|claude|codex)",
                    opts.engine_id
                );
                continue;
            }
            let Some(target) = parse_engine_id(name) else {
                eprintln!("alva-hub: unknown engine '{name}' (expected alva, claude, or codex)");
                continue;
            };
            if target.id() == opts.engine_id {
                eprintln!("alva-hub: engine is already {}", opts.engine_id);
                continue;
            }
            if target == Engine::Codex && opts.max_turns.is_some() {
                eprintln!(
                    "alva-hub: cannot switch to Codex while --max-turns is set; restart without that flag"
                );
                last_exit = USAGE_EXIT;
                continue;
            }
            if let Err(error) = ensure_engine_registered(
                hub,
                target,
                opts.claude_mode,
                opts.permission,
                &opts.workspace,
            ) {
                eprintln!("alva-hub: cannot register engine: {error}");
                continue;
            }
            if let Some(ref pid) = product_session {
                let original = match persist_handoff_intent(&opts.store, pid, target.id()) {
                    Ok(original) => original,
                    Err(error) => {
                        eprintln!("alva-hub: cannot persist handoff intent: {error}");
                        last_exit = 1;
                        continue;
                    }
                };
                match hub.switch_engine(pid, target.id()) {
                    Ok(snap) => {
                        let from = snap
                            .previous_engine_id
                            .as_ref()
                            .map(|e| e.as_str().to_string())
                            .unwrap_or_else(|| opts.engine_id.clone());
                        opts.handoff_context_next =
                            Some(crate::handoff::build_handoff_context(&from, target.id()));
                        opts.engine_id = target.id().to_string();
                        eprintln!(
                            "alva-hub: switched to engine={} (product session kept; next prompt gets handoff context once)",
                            snap.engine_id
                        );
                    }
                    Err(error) => {
                        if let Err(rollback_error) = opts.store.upsert(original) {
                            eprintln!(
                                "alva-hub: cannot roll back failed handoff intent: {rollback_error}"
                            );
                        }
                        eprintln!("alva-hub: switch failed: {error}");
                        last_exit = 1;
                    }
                }
            } else {
                opts.engine_id = target.id().to_string();
                opts.handoff_context_next = None;
                eprintln!(
                    "alva-hub: engine set to {} (no product session yet)",
                    opts.engine_id
                );
            }
            continue;
        }
        if line.starts_with('/') {
            eprintln!("alva-hub: unknown command '{line}' (try /help)");
            continue;
        }

        let engine_id = opts.engine_id.clone();
        let outcome = run_turn(hub, &engine_id, product_session.clone(), line, opts).await;
        if outcome.exit_code != 0 {
            last_exit = outcome.exit_code;
        }
        let terminate_repl = outcome.terminate_repl;
        if let Some(id) = outcome.product_session_id {
            product_session = Some(id);
        }
        if terminate_repl {
            return last_exit;
        }
    }

    last_exit
}

fn list_sessions(store: &SessionStore) -> bool {
    match store.list_recent(20) {
        Ok(list) if list.is_empty() => {
            eprintln!("alva-hub: journal is empty");
            true
        }
        Ok(list) => {
            for s in list {
                eprintln!(
                    "alva-hub: {}  engine={}  workspace={}{}",
                    safe_diagnostic_text(&s.product_session_id, 128),
                    safe_diagnostic_text(&s.engine_id, 64),
                    safe_diagnostic_text(&s.workspace, 500),
                    s.pending_handoff_target
                        .as_deref()
                        .map(|target| format!(
                            "  pending_handoff={}",
                            safe_diagnostic_text(target, 64)
                        ))
                        .unwrap_or_default()
                );
            }
            true
        }
        Err(e) => {
            eprintln!("alva-hub: cannot list sessions: {e}");
            false
        }
    }
}

async fn run_turn(
    hub: &HubRuntime,
    engine_id: &str,
    resume: Option<ProductSessionId>,
    prompt: &str,
    opts: &mut TurnOptions,
) -> TurnOutcome {
    let request = RuntimeRequest {
        prompt: prompt.to_string(),
        resume_session: None,
        system_prompt: None,
        working_directory: Some(opts.workspace.clone()),
        options: RuntimeOptions {
            streaming: false,
            max_turns: opts.max_turns,
            ..Default::default()
        },
    };
    // Choose the product id before execute so timeout / SIGINT can route a
    // cancellation even before the first engine event reaches the consumer.
    let requested_product_id = resume.unwrap_or_default();
    let mut hub_request = HubRequest::new(engine_id, request).resume(requested_product_id.clone());
    // Take one-shot handoff preamble (if this is the first turn after switch).
    // HubRuntime also gates on pending_handoff so a stale preamble is harmless.
    let handoff_context = opts.handoff_context_next.take();
    if let Some(ref ctx) = handoff_context {
        if opts.verbose {
            eprintln!("alva-hub: injecting handoff context ({} bytes)", ctx.len());
        }
        hub_request = hub_request.with_handoff_context(ctx.clone());
    }

    let mut events = match hub.execute(hub_request) {
        Ok(events) => events,
        Err(error) => {
            opts.handoff_context_next = handoff_context;
            eprintln!(
                "alva-hub: {}",
                safe_diagnostic_text(&error.to_string(), 1_000)
            );
            return TurnOutcome {
                exit_code: 1,
                product_session_id: None,
                terminate_repl: false,
            };
        }
    };

    let mut failed = false;
    let mut cancel_requested = false;
    let mut exit_override = None;
    let mut saw_completed = false;
    let mut product_session_id = Some(requested_product_id.clone());
    let mut last_message_text: Option<String> = None;
    let mut final_result_text: Option<String> = None;
    let deadline = tokio::time::sleep(opts.timeout);
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(deadline);
    tokio::pin!(interrupt);

    'events: loop {
        let hub_event = tokio::select! {
            interrupt_result = &mut interrupt => {
                failed = true;
                cancel_requested = true;
                exit_override = Some(match interrupt_result {
                    Ok(()) => {
                        eprintln!("alva-hub: interrupted; cancelling active engine session");
                        INTERRUPTED_EXIT
                    }
                    Err(error) => {
                        eprintln!("alva-hub: cannot install interrupt handler: {error}");
                        1
                    }
                });
                break;
            }
            _ = &mut deadline => {
                failed = true;
                cancel_requested = true;
                exit_override = Some(TIMEOUT_EXIT);
                eprintln!(
                    "alva-hub: turn exceeded {} seconds; cancelling active engine session",
                    opts.timeout.as_secs()
                );
                break;
            }
            event = events.next() => match event {
                Some(event) => event,
                None => break,
            }
        };

        product_session_id = Some(hub_event.product_session_id.clone());
        let product_session_id = &hub_event.product_session_id;
        match hub_event.event {
            RuntimeEvent::Message { content, .. } => {
                let mut message_text = String::new();
                for block in content {
                    if let Some(text) = block.as_text() {
                        if let Err(error) =
                            append_bounded_text(&mut message_text, text, MAX_FINAL_OUTPUT_BYTES)
                        {
                            failed = true;
                            cancel_requested = true;
                            eprintln!("alva-hub: {error}");
                            break 'events;
                        }
                    }
                }
                if !message_text.is_empty() {
                    last_message_text = Some(message_text);
                }
            }
            RuntimeEvent::MessageDelta { .. } => {}
            RuntimeEvent::PermissionRequest {
                request_id,
                tool_name,
                tool_input,
                ..
            } => {
                let decision = tokio::select! {
                    decision = resolve_permission(opts.permission, &tool_name, &tool_input) => decision,
                    interrupt_result = &mut interrupt => {
                        failed = true;
                        cancel_requested = true;
                        exit_override = Some(if interrupt_result.is_ok() { INTERRUPTED_EXIT } else { 1 });
                        eprintln!("alva-hub: permission prompt interrupted; cancelling session");
                        break 'events;
                    }
                    _ = &mut deadline => {
                        failed = true;
                        cancel_requested = true;
                        exit_override = Some(TIMEOUT_EXIT);
                        eprintln!("alva-hub: permission prompt timed out; cancelling session");
                        break 'events;
                    }
                };
                let response = hub.respond_permission(product_session_id, &request_id, decision);
                let response = tokio::select! {
                    result = response => result,
                    interrupt_result = &mut interrupt => {
                        failed = true;
                        cancel_requested = true;
                        exit_override = Some(if interrupt_result.is_ok() { INTERRUPTED_EXIT } else { 1 });
                        eprintln!("alva-hub: permission response interrupted; cancelling session");
                        break 'events;
                    }
                    _ = &mut deadline => {
                        failed = true;
                        cancel_requested = true;
                        exit_override = Some(TIMEOUT_EXIT);
                        eprintln!("alva-hub: permission response timed out; cancelling session");
                        break 'events;
                    }
                };
                match response {
                    Ok(()) => {}
                    // An approval that already expired on the engine side means
                    // this one tool was blocked — the middleware hands the agent
                    // a tool error it can recover from. Killing the whole run
                    // over it would turn a single blocked tool into a failed
                    // task, so report it and keep going.
                    Err(error)
                        if matches!(
                            error,
                            alva_hub_core::HubError::Runtime(
                                alva_engine_runtime::RuntimeError::PermissionNotFound(_)
                            )
                        ) =>
                    {
                        eprintln!(
                            "alva-hub: permission request already expired on the engine; the tool was blocked: {}",
                            safe_diagnostic_text(&error.to_string(), 1_000)
                        );
                    }
                    Err(error) => {
                        failed = true;
                        cancel_requested = true;
                        eprintln!(
                            "alva-hub: could not answer permission request; cancelling session: {}",
                            safe_diagnostic_text(&error.to_string(), 1_000)
                        );
                        break;
                    }
                }
            }
            RuntimeEvent::Error {
                message,
                recoverable,
            } => {
                let message = safe_diagnostic_text(&message, 1_000);
                if recoverable {
                    eprintln!("alva-hub: (retrying) {message}");
                } else {
                    failed = true;
                    eprintln!("alva-hub: {message}");
                }
            }
            RuntimeEvent::Completed { result, .. } => {
                saw_completed = true;
                let result = result.or_else(|| last_message_text.take());
                if let Some(result) = result {
                    if result.len() > MAX_FINAL_OUTPUT_BYTES {
                        failed = true;
                        eprintln!(
                            "alva-hub: final output exceeds {} bytes",
                            MAX_FINAL_OUTPUT_BYTES
                        );
                    } else {
                        final_result_text = Some(result);
                    }
                }
            }
            RuntimeEvent::SessionStarted {
                session_id,
                model,
                tools,
            } => {
                if opts.verbose {
                    let session_id = safe_diagnostic_text(&session_id, 160);
                    let model = model
                        .as_deref()
                        .map(|value| safe_diagnostic_text(value, 160));
                    eprintln!(
                        "alva-hub: session_started engine_session={session_id} model={:?} tools={}",
                        model,
                        tools.len()
                    );
                }
            }
            RuntimeEvent::ToolStart { id, name, input } => {
                if opts.verbose {
                    let id = safe_diagnostic_text(&id, 160);
                    let name = safe_diagnostic_text(&name, 160);
                    eprintln!(
                        "alva-hub: tool_start {name} id={id} {}",
                        json_field_summary(&input)
                    );
                }
            }
            RuntimeEvent::ToolEnd {
                id,
                name,
                result,
                duration_ms,
            } => {
                if opts.verbose {
                    let id = safe_diagnostic_text(&id, 160);
                    let name = safe_diagnostic_text(&name, 160);
                    let dur = duration_ms.map(|ms| format!(" {ms}ms")).unwrap_or_default();
                    let ok = !result.is_error;
                    eprintln!("alva-hub: tool_end {name} id={id} ok={ok}{dur}");
                }
            }
        }
    }

    if cancel_requested {
        cancel_and_drain(hub, &requested_product_id, &mut events).await;
    }
    if !saw_completed && !cancel_requested {
        failed = true;
        eprintln!("alva-hub: product event stream closed without Completed");
    }

    if let Some(ref session_id) = product_session_id {
        eprintln!("alva-hub: product_session_id={session_id}");
        if let Some(snapshot) = hub.session(session_id) {
            if snapshot.stream_hard_violations > 0 {
                failed = true;
                eprintln!(
                    "alva-hub: stream contract hard violations: {}{}",
                    snapshot.stream_hard_violations,
                    snapshot
                        .last_stream_violation
                        .as_ref()
                        .map(|detail| { format!(" (last: {})", safe_diagnostic_text(detail, 500)) })
                        .unwrap_or_default()
                );
            } else if snapshot.stream_soft_warnings > 0 {
                eprintln!(
                    "alva-hub: stream contract soft warnings: {}",
                    snapshot.stream_soft_warnings
                );
            }

            // Persist only terminal routes. Failed handoff attempts leave their
            // source-route + target intent intact for a safe retry.
            if snapshot.status == HubSessionStatus::Completed {
                if let Some(engine_session_id) = snapshot.engine_session_id.clone() {
                    let rec = StoredSession {
                        product_session_id: session_id.as_str().to_string(),
                        engine_id: snapshot.engine_id.as_str().to_string(),
                        engine_session_id,
                        workspace: opts.workspace.display().to_string(),
                        claude_mode: if snapshot.engine_id.as_str() == "claude" {
                            Some(match opts.claude_mode {
                                ClaudeMode::Light => "light".into(),
                                ClaudeMode::Full => "full".into(),
                            })
                        } else {
                            None
                        },
                        previous_engine_id: snapshot
                            .previous_engine_id
                            .as_ref()
                            .map(|e| e.as_str().to_string()),
                        previous_engine_session_id: snapshot.previous_engine_session_id.clone(),
                        pending_handoff_target: None,
                        updated_at: now_unix(),
                    };
                    if let Err(e) = opts.store.upsert(rec) {
                        failed = true;
                        eprintln!("alva-hub: failed to write session journal: {e}");
                    } else if opts.verbose {
                        eprintln!("alva-hub: journal updated {}", opts.store.path().display());
                    }
                } else {
                    failed = true;
                    eprintln!("alva-hub: completed session has no native engine session id");
                }
            }
        }
    }

    if failed {
        if let Some(context) = handoff_context {
            opts.handoff_context_next = Some(context);
        }
    } else if let Some(result) = final_result_text {
        println!("{result}");
    }

    let terminate_repl = exit_override.is_some();
    TurnOutcome {
        exit_code: exit_override.unwrap_or(if failed { 1 } else { 0 }),
        product_session_id,
        terminate_repl,
    }
}

async fn cancel_and_drain(
    hub: &HubRuntime,
    product_session_id: &ProductSessionId,
    events: &mut HubEventStream,
) {
    match tokio::time::timeout(CANCEL_TIMEOUT, hub.cancel(product_session_id)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!(
            "alva-hub: cancellation could not be routed: {}",
            safe_diagnostic_text(&error.to_string(), 1_000)
        ),
        Err(_) => eprintln!("alva-hub: cancellation request timed out"),
    }
    if tokio::time::timeout(CANCEL_TIMEOUT, async {
        while events.next().await.is_some() {}
    })
    .await
    .is_err()
    {
        eprintln!("alva-hub: engine stream did not close after cancellation");
    }
}

async fn resolve_permission(
    policy: PermissionPolicy,
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> PermissionDecision {
    match policy {
        PermissionPolicy::Deny => {
            eprintln!(
                "alva-hub: denied permission request for tool '{}'",
                safe_diagnostic_text(tool_name, 160)
            );
            PermissionDecision::Deny {
                message: "Alva Hub CLI permission policy is deny".into(),
            }
        }
        PermissionPolicy::Allow => {
            eprintln!(
                "alva-hub: auto-allowed permission request for tool '{}'",
                safe_diagnostic_text(tool_name, 160)
            );
            PermissionDecision::Allow {
                updated_input: None,
            }
        }
        PermissionPolicy::Ask => ask_permission_interactive(tool_name, tool_input).await,
    }
}

async fn ask_permission_interactive(
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> PermissionDecision {
    if !stdin_is_tty() {
        eprintln!(
            "alva-hub: permission request for tool '{}' denied because stdin is not a TTY",
            safe_diagnostic_text(tool_name, 160)
        );
        return PermissionDecision::Deny {
            message: "permission ask requires a TTY".into(),
        };
    }

    let tool_name = safe_diagnostic_text(tool_name, 160);
    let preview = redacted_json_preview(tool_input, 400);
    let (decision_tx, decision_rx) = tokio::sync::oneshot::channel();
    let spawn_result = std::thread::Builder::new()
        .name("alva-hub-permission-input".into())
        .spawn(move || {
            eprintln!("alva-hub: permission request for tool '{tool_name}'");
            eprintln!("alva-hub: redacted_input={preview}");
            eprint!("alva-hub: allow? [y/N] ");
            let _ = io::stderr().flush();

            let mut line = String::new();
            let decision = match io::stdin().read_line(&mut line) {
                Ok(0) => PermissionDecision::Deny {
                    message: "permission denied (EOF)".into(),
                },
                Ok(_) => {
                    let ans = line.trim().to_ascii_lowercase();
                    if matches!(ans.as_str(), "y" | "yes") {
                        PermissionDecision::Allow {
                            updated_input: None,
                        }
                    } else {
                        PermissionDecision::Deny {
                            message: "user denied permission".into(),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("alva-hub: failed to read permission answer: {e}");
                    PermissionDecision::Deny {
                        message: "permission denied (stdin error)".into(),
                    }
                }
            };
            let _ = decision_tx.send(decision);
        });
    if let Err(error) = spawn_result {
        return PermissionDecision::Deny {
            message: format!("permission prompt thread failed: {error}"),
        };
    }
    match decision_rx.await {
        Ok(decision) => decision,
        Err(_) => PermissionDecision::Deny {
            message: "permission prompt thread stopped unexpectedly".into(),
        },
    }
}

fn stdin_is_tty() -> bool {
    use std::io::IsTerminal;
    io::stdin().is_terminal()
}

fn append_bounded_text(target: &mut String, text: &str, max: usize) -> Result<(), String> {
    if target.len().saturating_add(text.len()) > max {
        return Err(format!("final output exceeds {max} bytes"));
    }
    target.push_str(text);
    Ok(())
}

fn json_field_summary(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(fields) => {
            let mut names = fields
                .keys()
                .map(|key| {
                    key.chars()
                        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                        .take(40)
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            names.sort();
            format!("input_fields=[{}]", names.join(","))
        }
        serde_json::Value::Array(values) => format!("input=array(len={})", values.len()),
        serde_json::Value::Null => "input=null".into(),
        _ => "input=scalar(redacted)".into(),
    }
}

fn safe_diagnostic_text(value: &str, max_chars: usize) -> String {
    let printable = value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars.saturating_add(1))
        .collect::<String>();
    if contains_inline_secret(&printable) {
        return "[REDACTED]".into();
    }
    truncate_chars(&printable, max_chars)
}

fn redacted_json_preview(value: &serde_json::Value, max_chars: usize) -> String {
    let mut output = String::new();
    write_redacted_json_preview(value, None, 0, &mut output, max_chars.saturating_add(1));
    truncate_chars(&output, max_chars)
}

fn write_redacted_json_preview(
    value: &serde_json::Value,
    key: Option<&str>,
    depth: usize,
    output: &mut String,
    max_chars: usize,
) {
    if output.chars().count() >= max_chars {
        return;
    }
    if key.is_some_and(is_secret_key) {
        push_preview_fragment(output, "\"[REDACTED]\"", max_chars);
        return;
    }
    if depth >= 4 {
        push_preview_fragment(output, "\"…\"", max_chars);
        return;
    }
    match value {
        serde_json::Value::Object(fields) => {
            push_preview_fragment(output, "{", max_chars);
            for (index, (field, value)) in fields.iter().take(16).enumerate() {
                if index > 0 {
                    push_preview_fragment(output, ",", max_chars);
                }
                let display_field = truncate_chars(field, 80);
                let encoded =
                    serde_json::to_string(&display_field).unwrap_or_else(|_| "\"?\"".into());
                push_preview_fragment(output, &encoded, max_chars);
                push_preview_fragment(output, ":", max_chars);
                write_redacted_json_preview(value, Some(field), depth + 1, output, max_chars);
            }
            if fields.len() > 16 {
                push_preview_fragment(output, ",\"…\":\"…\"", max_chars);
            }
            push_preview_fragment(output, "}", max_chars);
        }
        serde_json::Value::Array(values) => {
            push_preview_fragment(output, "[", max_chars);
            for (index, value) in values.iter().take(8).enumerate() {
                if index > 0 {
                    push_preview_fragment(output, ",", max_chars);
                }
                write_redacted_json_preview(value, None, depth + 1, output, max_chars);
            }
            if values.len() > 8 {
                push_preview_fragment(output, ",\"…\"", max_chars);
            }
            push_preview_fragment(output, "]", max_chars);
        }
        serde_json::Value::String(text) => {
            let text = if contains_inline_secret(&truncate_chars(text, 256)) {
                "[REDACTED]".into()
            } else {
                truncate_chars(text, 160)
            };
            let encoded = serde_json::to_string(&text).unwrap_or_else(|_| "\"?\"".into());
            push_preview_fragment(output, &encoded, max_chars);
        }
        scalar => push_preview_fragment(output, &scalar.to_string(), max_chars),
    }
}

fn push_preview_fragment(output: &mut String, fragment: &str, max_chars: usize) {
    let used = output.chars().count();
    output.extend(fragment.chars().take(max_chars.saturating_sub(used)));
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '.'], "_");
    normalized == "authorization"
        || normalized == "cookie"
        || normalized.contains("api_key")
        || normalized.contains("access_token")
        || normalized.contains("refresh_token")
        || normalized.contains("client_secret")
        || normalized.ends_with("_password")
        || normalized == "password"
        || normalized == "token"
}

fn contains_inline_secret(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    normalized.starts_with("sk-")
        || normalized.contains("bearer ")
        || normalized.contains("api_key=")
        || normalized.contains("api-key=")
        || normalized.contains("access_token=")
        || normalized.contains("password=")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let prefix = value.chars().take(max_chars - 1).collect::<String>();
    format!("{prefix}…")
}

fn resolve_workspace(cwd: Option<&str>) -> Result<PathBuf, String> {
    match cwd {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_dir() {
                return Err(format!("--cwd path is not a directory: {}", path.display()));
            }
            path.canonicalize()
                .map_err(|e| format!("cannot resolve --cwd {}: {e}", path.display()))
        }
        None => std::env::current_dir().map_err(|e| format!("cannot determine workspace: {e}")),
    }
}

fn build_selected_engine(
    engine: Engine,
    claude_mode: ClaudeMode,
    permission: PermissionPolicy,
    workspace: &std::path::Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    match engine {
        Engine::Alva => {
            // The shared config uses the providers{} map (see `alva providers
            // list`), which the SDK-level ProviderConfig::load cannot see —
            // it only reads top-level legacy fields and would report "no API
            // key" for a valid profile. Resolution mirrors the Alva CLI:
            // explicit env key first, then the shared active profile, then
            // project/default layers.
            let env_key = std::env::var("ALVA_API_KEY")
                .ok()
                .filter(|key| !key.is_empty());
            let config = if env_key.is_some() {
                alva_llm_provider::ProviderConfig::load(workspace)?
            } else if let Some(shared) = alva_app_core::config::load() {
                match shared.active_provider() {
                    Some((name, entry)) => {
                        let kind = entry.effective_kind(name);
                        alva_llm_provider::ProviderConfig {
                            api_key: entry.api_key.clone(),
                            model: entry.model.clone().unwrap_or_else(|| {
                                // Mirrors the Alva CLI default table until it
                                // moves into alva-llm-provider as the single
                                // canonical source.
                                match kind {
                                    "anthropic" => "claude-opus-4-7",
                                    "gemini" => "gemini-2.5-pro",
                                    _ => "gpt-4o",
                                }
                                .to_string()
                            }),
                            base_url: entry.base_url.clone().unwrap_or_else(|| {
                                alva_llm_provider::default_base_url(Some(kind)).to_string()
                            }),
                            max_tokens: 32_000,
                            custom_headers: Default::default(),
                            kind: Some(kind.to_string()),
                        }
                    }
                    None => alva_llm_provider::ProviderConfig::load(workspace)?,
                }
            } else {
                alva_llm_provider::ProviderConfig::load(workspace)?
            };
            if config.api_key.is_empty() {
                return Err("no Alva provider API key configured (set ALVA_API_KEY)".into());
            }
            let provider_kind = config.kind.clone();
            let provider_registry = alva_llm_provider::build_provider_registry(&config);
            let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
            let factory = CatalogBaseAgentFactory::new(model, workspace)
                .with_provider_registry(provider_registry);
            // Durable typed records next to the Hub session journal let
            // `--session` resume Alva across process restarts.
            let repository_path = crate::handoff::alva_repository_path();
            let mut adapter = AlvaAdapter::new(Arc::new(factory));
            if let Some(path) = repository_path {
                adapter = adapter.with_session_repository(SessionRepository::open(path)?);
            }
            Ok(Arc::new(adapter))
        }
        Engine::Claude => match claude_mode {
            ClaudeMode::Light => {
                let config = ClaudePrintConfig {
                    default_cwd: Some(workspace.to_path_buf()),
                    permission_mode: claude_light_permission_mode(permission)?,
                    ..Default::default()
                };
                Ok(Arc::new(ClaudePrintAdapter::new(config)))
            }
            ClaudeMode::Full => Ok(Arc::new(ClaudeAdapter::new(ClaudeAdapterConfig::default()))),
        },
        Engine::Codex => {
            if permission != PermissionPolicy::Deny {
                return Err(format!(
                    "Codex exec does not support '--permission {}': it has no interactive \
                     permission callback, and auto-allow would also disable its sandbox",
                    permission.as_str()
                ));
            }
            let config = CodexAdapterConfig {
                default_cwd: Some(workspace.to_path_buf()),
                ..Default::default()
            };
            Ok(Arc::new(CodexAdapter::new(config)))
        }
    }
}

fn claude_light_permission_mode(
    permission: PermissionPolicy,
) -> Result<ClaudePrintPermissionMode, String> {
    match permission {
        PermissionPolicy::Deny => Ok(ClaudePrintPermissionMode::DontAsk),
        PermissionPolicy::Allow => Ok(ClaudePrintPermissionMode::BypassPermissions),
        PermissionPolicy::Ask => Err(
            "Claude light mode does not support '--permission ask'; use '--claude-mode full'"
                .into(),
        ),
    }
}

fn print_usage() {
    println!(
        "Usage: alva-hub [options] [--] <prompt>\n\
         \n\
         Options:\n  \
         --engine alva|claude|codex   Engine (default: alva; journal may override with --session)\n  \
         --claude-mode light|full     Claude transport (default: light)\n  \
         --cwd <dir>                  Workspace (default: process cwd)\n  \
         --session <id>               Resume product session from journal\n  \
         --session <id> --engine X    Hand off product session to engine X (fresh native session)\n  \
         --repl                       Multi-turn REPL; session resumes in-process\n  \
         --verbose, -v                Tool/session events on stderr\n  \
         --permission deny|allow|ask  HITL policy (default: deny)\n  \
         --timeout <seconds>           Per-turn wall-clock limit, 1–604800 (default: 1800)\n  \
         --max-turns <count>           Engine agentic-turn ceiling, 1–10000, when supported\n  \
         --version, -V                 Show version\n  \
         --help, -h                   Show this help\n\
         \n\
         Engines:\n  \
         alva    Full Alva harness; typed records under ~/.config/alva/hub/alva-history for --session\n  \
         claude  Claude Code (light/full); journal + native --resume\n  \
         codex   `codex exec --json` / `codex exec resume <id> --json`\n\
         \n\
         Session journal: $ALVA_HUB_HOME/sessions.json or ~/.config/alva/hub/sessions.json\n\
         REPL commands: /session  /sessions  /engine <name>  /help  /quit"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn durable_session(workspace: &std::path::Path) -> StoredSession {
        StoredSession {
            product_session_id: "product-1".into(),
            engine_id: "alva".into(),
            engine_session_id: "native-alva-1".into(),
            workspace: workspace.display().to_string(),
            claude_mode: None,
            previous_engine_id: None,
            previous_engine_session_id: None,
            pending_handoff_target: None,
            updated_at: 1,
        }
    }

    #[test]
    fn selecting_claude_light_does_not_read_alva_provider_configuration() {
        let nonexistent_workspace =
            std::path::Path::new("/definitely/not/an/alva/provider/workspace");
        assert!(build_selected_engine(
            Engine::Claude,
            ClaudeMode::Light,
            PermissionPolicy::Deny,
            nonexistent_workspace
        )
        .is_ok());
    }

    #[test]
    fn selecting_claude_full_does_not_read_alva_provider_configuration() {
        let nonexistent_workspace =
            std::path::Path::new("/definitely/not/an/alva/provider/workspace");
        assert!(build_selected_engine(
            Engine::Claude,
            ClaudeMode::Full,
            PermissionPolicy::Ask,
            nonexistent_workspace
        )
        .is_ok());
    }

    #[test]
    fn selecting_codex_does_not_read_alva_provider_configuration() {
        let nonexistent_workspace =
            std::path::Path::new("/definitely/not/an/alva/provider/workspace");
        assert!(build_selected_engine(
            Engine::Codex,
            ClaudeMode::Light,
            PermissionPolicy::Deny,
            nonexistent_workspace
        )
        .is_ok());
    }

    #[test]
    fn noninteractive_engines_do_not_silently_ignore_permission_policy() {
        let workspace = std::path::Path::new("/definitely/not/an/alva/provider/workspace");
        assert_eq!(
            claude_light_permission_mode(PermissionPolicy::Deny).unwrap(),
            ClaudePrintPermissionMode::DontAsk
        );
        assert_eq!(
            claude_light_permission_mode(PermissionPolicy::Allow).unwrap(),
            ClaudePrintPermissionMode::BypassPermissions
        );
        assert!(claude_light_permission_mode(PermissionPolicy::Ask).is_err());
        assert!(build_selected_engine(
            Engine::Codex,
            ClaudeMode::Light,
            PermissionPolicy::Allow,
            workspace
        )
        .is_err());
        assert!(build_selected_engine(
            Engine::Codex,
            ClaudeMode::Light,
            PermissionPolicy::Ask,
            workspace
        )
        .is_err());
    }

    #[test]
    fn redacted_json_preview_is_unicode_safe_and_hides_secrets() {
        let input = serde_json::json!({
            "api_key": "sk-test-secret",
            "message": "中".repeat(200),
            "nested": {"authorization": "Bearer secret"}
        });
        let out = redacted_json_preview(&input, 40);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 40);
        assert!(!out.contains("sk-test-secret"));
        assert!(!out.contains("Bearer secret"));
    }

    #[test]
    fn final_output_accumulator_is_bounded_without_slicing_utf8() {
        let mut output = String::new();
        append_bounded_text(&mut output, "中文", 6).unwrap();
        assert_eq!(output, "中文");
        assert!(append_bounded_text(&mut output, "x", 6).is_err());
    }

    #[tokio::test]
    async fn resolve_permission_deny_and_allow() {
        let input = serde_json::json!({});
        assert!(matches!(
            resolve_permission(PermissionPolicy::Deny, "shell", &input).await,
            PermissionDecision::Deny { .. }
        ));
        assert!(matches!(
            resolve_permission(PermissionPolicy::Allow, "shell", &input).await,
            PermissionDecision::Allow { .. }
        ));
    }

    #[test]
    fn handoff_intent_is_durable_before_the_route_changes() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::open(root.path().join("sessions.json"));
        let source = durable_session(root.path());
        store.upsert(source.clone()).unwrap();

        let product_id = ProductSessionId::from_existing(&source.product_session_id);
        let rollback = persist_handoff_intent(&store, &product_id, "claude").unwrap();
        let pending = store.get(product_id.as_str()).unwrap().unwrap();

        assert_eq!(rollback, source);
        assert_eq!(pending.engine_id, "alva");
        assert_eq!(pending.engine_session_id, "native-alva-1");
        assert_eq!(pending.pending_handoff_target.as_deref(), Some("claude"));
    }

    #[test]
    fn restart_replays_a_pending_handoff_target() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::open(root.path().join("sessions.json"));
        let mut source = durable_session(root.path());
        source.pending_handoff_target = Some("codex".into());
        store.upsert(source).unwrap();

        let mut engine = Engine::Alva;
        let mut claude_mode = ClaudeMode::Light;
        let mut workspace = root.path().to_path_buf();
        let plan = rehydrate_session(
            &store,
            "product-1",
            &mut engine,
            &mut claude_mode,
            &mut workspace,
            false,
            false,
        )
        .unwrap();

        assert_eq!(engine, Engine::Codex);
        assert_eq!(plan.product_id.as_str(), "product-1");
        assert_eq!(
            plan.handoff_from
                .as_ref()
                .map(|record| record.engine_id.as_str()),
            Some("alva")
        );
        assert_eq!(plan.record.pending_handoff_target.as_deref(), Some("codex"));
    }

    /// The shared config uses the providers{} map, which the SDK-level
    /// ProviderConfig::load cannot see. Regression guard for the alva
    /// provider resolution fix: an active profile in ALVA_CONFIG_DIR must
    /// assemble, and a missing key must fail loudly.
    #[test]
    fn alva_assembles_from_shared_active_profile_and_fails_without_key() {
        // env vars are process-global; serialize so parallel tests cannot
        // race the ALVA_CONFIG_DIR override.
        use std::sync::OnceLock;
        static ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();

        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        // Profile config with the active marker the CLI expects.
        std::fs::write(
            root.path().join("config.json"),
            r#"{
              "providers": {
                "anthropic": {
                  "kind": "anthropic",
                  "api_key": "sk-test-not-real",
                  "model": "claude-opus-4-7",
                  "base_url": "https://api.anthropic.com"
                }
              },
              "active": "anthropic"
            }"#,
        )
        .unwrap();

        std::env::set_var("ALVA_CONFIG_DIR", root.path());
        std::env::set_var("ALVA_HUB_HOME", root.path().join("hub"));
        build_selected_engine(
            Engine::Alva,
            ClaudeMode::Light,
            PermissionPolicy::Deny,
            workspace.path(),
        )
        .expect("active shared profile must assemble Alva");
        std::env::remove_var("ALVA_CONFIG_DIR");

        // Same config without a usable key fails loudly with the classic
        // guidance, not a silent fall-through.
        std::fs::write(
            root.path().join("config.json"),
            r#"{"providers": {"anthropic": {"kind": "anthropic", "api_key": ""}}, "active": "anthropic"}"#,
        )
        .unwrap();
        std::env::set_var("ALVA_CONFIG_DIR", root.path());
        let error = match build_selected_engine(
            Engine::Alva,
            ClaudeMode::Light,
            PermissionPolicy::Deny,
            workspace.path(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("an empty key must fail loudly"),
        };
        std::env::remove_var("ALVA_CONFIG_DIR");
        std::env::remove_var("ALVA_HUB_HOME");
        assert!(
            error.contains("ALVA_API_KEY"),
            "guidance names the env var: {error}"
        );
    }
}
