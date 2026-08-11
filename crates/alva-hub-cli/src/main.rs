// INPUT:  process argv/env, cli grammar, session_store, alva_hub_core registry, adapters
// OUTPUT: alva-hub binary for one-shot, multi-turn, and rehydrated Hub sessions
// POS:    Hub CLI dispatcher that validates engine selection before initializing only that engine.

mod cli;
mod handoff;
mod session_store;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory, SessionRepository};
use alva_engine_adapter_claude::{
    ClaudeAdapter, ClaudeAdapterConfig, ClaudePrintAdapter, ClaudePrintConfig,
    ClaudePrintPermissionMode,
};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeEvent, RuntimeOptions, RuntimeRequest,
};
use alva_hub_core::{HubRequest, HubRuntime, HubSessionStatus, ProductSessionId};
use tokio_stream::StreamExt;

use crate::cli::{parse_args, ClaudeMode, Engine, ParseOutcome, PermissionPolicy};
use crate::session_store::{now_unix, prompt_preview, SessionStore, StoredSession};

const USAGE_EXIT: i32 = 2;

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
        Ok(ParseOutcome::Run(invocation)) => invocation,
        Err(error) => {
            eprintln!("alva-hub: {error}");
            eprintln!("Try 'alva-hub --help'.");
            return USAGE_EXIT;
        }
    };

    let store = SessionStore::open_default();
    if let Some(ref s) = store {
        if invocation.verbose {
            eprintln!("alva-hub: session journal {}", s.path().display());
        }
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
    if let Some(session_id) = invocation.session.as_deref() {
        match rehydrate_session(
            store.as_ref(),
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
            }
            Err(error) => {
                eprintln!("alva-hub: {error}");
                return USAGE_EXIT;
            }
        }
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
        if let Some(ref store) = store {
            if let Ok(Some(rec)) = store.get(product_id.as_str()) {
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
                    product_id, rec.engine_id, rec.engine_session_id
                );

                if handoff_from.is_some() && rec.engine_id != target_engine_id {
                    match hub.switch_engine(product_id, target_engine_id) {
                        Ok(snap) => {
                            eprintln!(
                                "alva-hub: switched engine {} → {} (fresh native session; product session kept)",
                                rec.engine_id, target_engine_id
                            );
                            if let Some(prev) = snap.previous_engine_session_id.as_deref() {
                                eprintln!(
                                    "alva-hub: previous engine_session={prev}; handoff context will be injected once"
                                );
                            }
                        }
                        Err(error) => {
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
                Some(from_rec),
                Some(from_rec.engine_session_id.as_str()),
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
}

/// Load journal entry and align engine/workspace selection.
fn rehydrate_session(
    store: Option<&SessionStore>,
    session_id: &str,
    engine: &mut Engine,
    claude_mode: &mut ClaudeMode,
    workspace: &mut PathBuf,
    cwd_explicit: bool,
    engine_explicit: bool,
) -> Result<RehydratePlan, String> {
    let store = store.ok_or_else(|| {
        "no session journal available (set ALVA_HUB_HOME or ensure config dir exists)".to_string()
    })?;
    let rec = store
        .get(session_id)?
        .ok_or_else(|| format!("unknown product session '{session_id}' in journal"))?;

    let mut handoff_from = None;
    if !engine_explicit {
        *engine = parse_engine_id(&rec.engine_id)
            .ok_or_else(|| format!("journal engine '{}' is not supported", rec.engine_id))?;
    } else if engine.id() != rec.engine_id {
        handoff_from = Some(rec.clone());
        // keep *engine as user-requested target
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
    store: Option<SessionStore>,
    /// Consumed once on the next `run_turn` after `switch_engine`.
    handoff_context_next: Option<String>,
}

struct TurnOutcome {
    exit_code: i32,
    product_session_id: Option<ProductSessionId>,
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
    if let Some(ref store) = opts.store {
        eprintln!("alva-hub: session journal {}", store.path().display());
    }

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
        if let Some(id) = outcome.product_session_id {
            product_session = Some(id);
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
                            snap.engine_id, snap.engine_session_id
                        );
                        if let Some(prev) = snap.previous_engine_id {
                            eprintln!(
                                "alva-hub: previous_engine={prev} previous_engine_session={:?}",
                                snap.previous_engine_session_id
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
            list_sessions(opts.store.as_ref());
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
                match hub.switch_engine(pid, target.id()) {
                    Ok(snap) => {
                        let from = snap
                            .previous_engine_id
                            .as_ref()
                            .map(|e| e.as_str().to_string())
                            .unwrap_or_else(|| opts.engine_id.clone());
                        let prev_sid = snap.previous_engine_session_id.clone();
                        let journal = opts
                            .store
                            .as_ref()
                            .and_then(|s| s.get(pid.as_str()).ok().flatten());
                        opts.handoff_context_next = Some(crate::handoff::build_handoff_context(
                            &from,
                            target.id(),
                            journal.as_ref(),
                            prev_sid.as_deref(),
                        ));
                        opts.engine_id = target.id().to_string();
                        eprintln!(
                            "alva-hub: switched to engine={} (product session kept; next prompt gets handoff context once)",
                            snap.engine_id
                        );
                    }
                    Err(error) => eprintln!("alva-hub: switch failed: {error}"),
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
        last_exit = outcome.exit_code;
        if let Some(id) = outcome.product_session_id {
            product_session = Some(id);
        }
    }

    last_exit
}

fn list_sessions(store: Option<&SessionStore>) {
    let Some(store) = store else {
        eprintln!("alva-hub: no session journal configured");
        return;
    };
    match store.list_recent(20) {
        Ok(list) if list.is_empty() => eprintln!("alva-hub: journal is empty"),
        Ok(list) => {
            for s in list {
                eprintln!(
                    "alva-hub: {}  engine={}  workspace={}  {}",
                    s.product_session_id,
                    s.engine_id,
                    s.workspace,
                    s.last_prompt_preview.as_deref().unwrap_or("")
                );
            }
        }
        Err(e) => eprintln!("alva-hub: cannot list sessions: {e}"),
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
            ..Default::default()
        },
    };
    let mut hub_request = HubRequest::new(engine_id, request);
    if let Some(id) = resume {
        hub_request = hub_request.resume(id);
    }
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
            eprintln!("alva-hub: {error}");
            return TurnOutcome {
                exit_code: 1,
                product_session_id: None,
            };
        }
    };

    let mut failed = false;
    let mut emitted_text = false;
    let mut product_session_id = None;
    let mut last_result_text: Option<String> = None;
    while let Some(hub_event) = events.next().await {
        product_session_id = Some(hub_event.product_session_id.clone());
        let product_session_id = &hub_event.product_session_id;
        match hub_event.event {
            RuntimeEvent::Message { content, .. } => {
                for block in content {
                    if let Some(text) = block.as_text() {
                        print!("{text}");
                        emitted_text = true;
                        last_result_text = Some(match last_result_text.take() {
                            Some(mut acc) => {
                                acc.push_str(text);
                                acc
                            }
                            None => text.to_string(),
                        });
                    }
                }
            }
            RuntimeEvent::MessageDelta { .. } => {}
            RuntimeEvent::PermissionRequest {
                request_id,
                tool_name,
                tool_input,
                ..
            } => {
                let decision = resolve_permission(opts.permission, &tool_name, &tool_input);
                if let Err(error) = hub
                    .respond_permission(product_session_id, &request_id, decision)
                    .await
                {
                    failed = true;
                    eprintln!("alva-hub: could not answer permission request: {error}");
                }
            }
            RuntimeEvent::Error {
                message,
                recoverable,
            } => {
                if recoverable {
                    eprintln!("alva-hub: (retrying) {message}");
                } else {
                    failed = true;
                    eprintln!("alva-hub: {message}");
                }
            }
            RuntimeEvent::Completed { result, .. } => {
                if !emitted_text {
                    if let Some(result) = result {
                        print!("{result}");
                        emitted_text = true;
                        last_result_text = Some(result);
                    }
                } else if last_result_text.is_none() {
                    last_result_text = result;
                }
            }
            RuntimeEvent::SessionStarted {
                session_id,
                model,
                tools,
            } => {
                if opts.verbose {
                    eprintln!(
                        "alva-hub: session_started engine_session={session_id} model={:?} tools={}",
                        model,
                        tools.len()
                    );
                }
            }
            RuntimeEvent::ToolStart { id, name, input } => {
                if opts.verbose {
                    let input_preview = truncate_json(&input, 160);
                    eprintln!("alva-hub: tool_start {name} id={id} input={input_preview}");
                }
            }
            RuntimeEvent::ToolEnd {
                id,
                name,
                result,
                duration_ms,
            } => {
                if opts.verbose {
                    let dur = duration_ms.map(|ms| format!(" {ms}ms")).unwrap_or_default();
                    let ok = !result.is_error;
                    eprintln!("alva-hub: tool_end {name} id={id} ok={ok}{dur}");
                }
            }
        }
    }
    if emitted_text {
        println!();
    }

    if let Some(ref session_id) = product_session_id {
        eprintln!("alva-hub: product_session_id={session_id}");
        if let Some(snapshot) = hub.session(session_id) {
            if snapshot.stream_hard_violations > 0 {
                eprintln!(
                    "alva-hub: stream contract hard violations: {}{}",
                    snapshot.stream_hard_violations,
                    snapshot
                        .last_stream_violation
                        .as_ref()
                        .map(|d| format!(" (last: {d})"))
                        .unwrap_or_default()
                );
            } else if snapshot.stream_soft_warnings > 0 {
                eprintln!(
                    "alva-hub: stream contract soft warnings: {}",
                    snapshot.stream_soft_warnings
                );
            }

            // Persist journal when we have a native engine session id.
            if let Some(engine_session_id) = snapshot.engine_session_id.clone() {
                if let Some(ref store) = opts.store {
                    let rec = StoredSession {
                        product_session_id: session_id.as_str().to_string(),
                        engine_id: opts.engine_id.clone(),
                        engine_session_id,
                        workspace: opts.workspace.display().to_string(),
                        claude_mode: if opts.engine_id == "claude" {
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
                        updated_at: now_unix(),
                        last_prompt_preview: prompt_preview(prompt, 80),
                        last_result_preview: last_result_text
                            .as_deref()
                            .and_then(|t| prompt_preview(t, 240)),
                    };
                    if let Err(e) = store.upsert(rec) {
                        eprintln!("alva-hub: failed to write session journal: {e}");
                    } else if opts.verbose {
                        eprintln!("alva-hub: journal updated {}", store.path().display());
                    }
                }
            }
        }
    }

    TurnOutcome {
        exit_code: if failed { 1 } else { 0 },
        product_session_id,
    }
}

fn resolve_permission(
    policy: PermissionPolicy,
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> PermissionDecision {
    match policy {
        PermissionPolicy::Deny => {
            eprintln!("alva-hub: denied permission request for tool '{tool_name}'");
            PermissionDecision::Deny {
                message: "Alva Hub CLI permission policy is deny".into(),
            }
        }
        PermissionPolicy::Allow => {
            eprintln!("alva-hub: auto-allowed permission request for tool '{tool_name}'");
            PermissionDecision::Allow {
                updated_input: None,
            }
        }
        PermissionPolicy::Ask => ask_permission_interactive(tool_name, tool_input),
    }
}

fn ask_permission_interactive(
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> PermissionDecision {
    let preview = truncate_json(tool_input, 200);
    eprintln!("alva-hub: permission request for tool '{tool_name}'");
    eprintln!("alva-hub: input={preview}");
    eprint!("alva-hub: allow? [y/N] ");
    let _ = io::stderr().flush();

    if !stdin_is_tty() {
        eprintln!("alva-hub: stdin is not a TTY; denying");
        return PermissionDecision::Deny {
            message: "permission ask requires a TTY".into(),
        };
    }

    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
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
    }
}

fn stdin_is_tty() -> bool {
    use std::io::IsTerminal;
    io::stdin().is_terminal()
}

fn truncate_json(value: &serde_json::Value, max: usize) -> String {
    let s = value.to_string();
    if s.len() <= max {
        s
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
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
    fn truncate_json_short_and_long() {
        let short = serde_json::json!({"a": 1});
        assert_eq!(truncate_json(&short, 160), short.to_string());
        let long = serde_json::json!({"x": "y".repeat(200)});
        let out = truncate_json(&long, 40);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 40);
    }

    #[test]
    fn resolve_permission_deny_and_allow() {
        let input = serde_json::json!({});
        assert!(matches!(
            resolve_permission(PermissionPolicy::Deny, "shell", &input),
            PermissionDecision::Deny { .. }
        ));
        assert!(matches!(
            resolve_permission(PermissionPolicy::Allow, "shell", &input),
            PermissionDecision::Allow { .. }
        ));
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
