// INPUT:  alva_app_core, alva_llm_provider, alva_host_native, checkpoint, session, output, event_handler, commands
// OUTPUT: run_repl
// POS:    Interactive REPL loop — session management, slash commands, and user input dispatch

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, PermissionMode, SkillRegistryError};
use alva_host_native::middleware::ApprovalRequest;
use alva_kernel_abi::agent_session::EventQuery;
use alva_kernel_abi::AgentSession;
use alva_llm_provider::ProviderConfig;
use tokio::sync::mpsc;

use crate::checkpoint;
use crate::commands::{CommandContext, CommandRegistry, CommandResult, TokenUsage};
use crate::event_handler;
use crate::output;
use crate::session::{JsonFileAgentSession, JsonFileSessionManager, SessionSummary};

// Session-level token accumulation uses TokenUsage directly.

/// Build a CommandContext from current REPL state.
fn build_command_context<'a>(
    workspace: &'a std::path::Path,
    config: &'a ProviderConfig,
    session_id: &'a str,
    message_count: usize,
    tokens: &TokenUsage,
    tool_names: Vec<String>,
    plugin_names: Vec<String>,
    middleware_names: Vec<String>,
    plan_mode: bool,
) -> CommandContext<'a> {
    CommandContext {
        workspace,
        model: &config.model,
        session_id,
        message_count,
        token_usage: tokens.clone(),
        tool_names,
        plugin_names,
        middleware_names,
        component_overrides: alva_app_core::config::load()
            .map(|cfg| cfg.components)
            .unwrap_or_default(),
        plan_mode,
    }
}

/// Run the interactive REPL loop with session management.
pub(crate) async fn run_repl(
    agent: &BaseAgent,
    config: &ProviderConfig,
    workspace: &std::path::Path,
    _paths: &AlvaPaths,
    session_manager: &JsonFileSessionManager,
    checkpoint_mgr: &checkpoint::CheckpointManager,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) {
    let registry = CommandRegistry::new();
    let mut tokens = TokenUsage::default();

    // Auto-resume latest or start new
    let (mut session_id, mut active_session) = match session_manager.latest() {
        Some(id) => match session_manager.load(&id).await {
            Some(sess) => {
                let sessions = session_manager.list();
                let meta = sessions.iter().find(|m| m.session_id == id);
                let (msg_count, summary) = meta
                    .map(|m| (m.event_count, m.preview.as_str()))
                    .unwrap_or((0, ""));
                output::print_session_resumed(&id, msg_count, summary);
                agent.swap_session(sess.clone()).await;
                (id, sess)
            }
            None => {
                let sess = session_manager.create("").await;
                let id = sess.session_id().to_string();
                agent.swap_session(sess.clone()).await;
                output::print_session_new(&id);
                (id, sess)
            }
        },
        None => {
            let sess = session_manager.create("").await;
            let id = sess.session_id().to_string();
            agent.swap_session(sess.clone()).await;
            output::print_session_new(&id);
            (id, sess)
        }
    };

    // Append eval_config_snapshot once per session — idempotent, so safe to
    // call here after every resume/create. Same record shape as Tauri's.
    session_manager
        .append_config_snapshot_if_needed(&active_session, agent, &config.model)
        .await;

    output::print_divider();

    // Analytics: SessionStart for the active session and a sticky timer
    // so we can emit SessionEnd with the right duration when the REPL
    // exits. `emit_session_*` is a no-op if no AnalyticsSink is on the bus.
    let session_started_at = std::time::Instant::now();
    emit_session_start(agent, &session_id, workspace);

    // reedline-driven input — slash autocomplete pops on keystroke (not
    // Tab). The completer pulls names from the registry plus REPL-side
    // hardcoded commands. History persists at ~/.alva/repl-history across
    // runs so frequent prompts stay reachable.
    let registry_names: Vec<String> = registry
        .list()
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect();

    let history_path = alva_app_core::config::alva_home_dir().map(|h| h.join("repl-history"));
    if let Some(p) = &history_path {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    use reedline::MenuBuilder; // for ColumnarMenu::with_name
    let history: Box<dyn reedline::History> = match &history_path {
        Some(p) => Box::new(
            reedline::FileBackedHistory::with_file(2000, p.clone()).unwrap_or_else(|_| {
                reedline::FileBackedHistory::new(2000).expect("in-memory history fallback")
            }),
        ),
        None => Box::new(reedline::FileBackedHistory::new(2000).expect("in-memory history")),
    };

    // Pageable single-column list — 6 entries per page, true paged loading
    // (completer.partial_complete called per page, not "load 25 then slice").
    //
    // To get the partial_complete path, ListMenu's `parsed.remainder` must
    // be empty. That's only the case when the input it sees is empty — which
    // is what `only_buffer_difference: true` gives us right when the menu
    // opens (the buffer state at open-time is the baseline; only the delta
    // since open is sent to the completer). So we keep the default true.
    //
    // The trade-off: the completer now sees the *post-`/` text* as `line`
    // (diff since open), not the full `/co...` buffer. SlashCompleter is
    // built to handle that — it filters command names by `line` directly,
    // no leading `/` required.
    let menu = Box::new(
        reedline::ListMenu::default()
            .with_name("completion_menu")
            .with_page_size(15),
    );

    let mut keybindings = reedline::default_emacs_keybindings();
    // Type `/` → insert it AND open the menu in the same event.
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Char('/'),
        reedline::ReedlineEvent::Multiple(vec![
            reedline::ReedlineEvent::Edit(vec![reedline::EditCommand::InsertChar('/')]),
            reedline::ReedlineEvent::Menu("completion_menu".to_string()),
        ]),
    );
    // Inside-menu navigation:
    //   ↑/↓        : row-by-row within the page
    //   PageUp/Dn  : page-by-page through the candidate list
    //   →/←        : also page-by-page (mirror PageUp/Dn for laptops without those keys)
    // Outside the menu these fall back to line/history navigation via UntilFound.
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Down,
        reedline::ReedlineEvent::UntilFound(vec![
            reedline::ReedlineEvent::MenuNext,
            reedline::ReedlineEvent::Down,
        ]),
    );
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Up,
        reedline::ReedlineEvent::UntilFound(vec![
            reedline::ReedlineEvent::MenuPrevious,
            reedline::ReedlineEvent::Up,
        ]),
    );
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::PageDown,
        reedline::ReedlineEvent::MenuPageNext,
    );
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::PageUp,
        reedline::ReedlineEvent::MenuPagePrevious,
    );
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Right,
        reedline::ReedlineEvent::UntilFound(vec![
            reedline::ReedlineEvent::MenuPageNext,
            reedline::ReedlineEvent::Right,
        ]),
    );
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Left,
        reedline::ReedlineEvent::UntilFound(vec![
            reedline::ReedlineEvent::MenuPagePrevious,
            reedline::ReedlineEvent::Left,
        ]),
    );
    // Tab still triggers the menu — useful when user already typed past `/`.
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Tab,
        reedline::ReedlineEvent::UntilFound(vec![
            reedline::ReedlineEvent::Menu("completion_menu".to_string()),
            reedline::ReedlineEvent::MenuNext,
        ]),
    );

    let edit_mode = Box::new(reedline::Emacs::new(keybindings));
    let mut line_editor = reedline::Reedline::create()
        .with_completer(Box::new(crate::repl_completer::SlashCompleter::new(
            registry_names,
        )))
        .with_menu(reedline::ReedlineMenu::EngineCompleter(menu))
        .with_edit_mode(edit_mode)
        .with_history(history);

    let prompt = ReplPrompt;

    loop {
        let line = match line_editor.read_line(&prompt) {
            Ok(reedline::Signal::Success(line)) => line,
            // Ctrl+C and Ctrl+D both exit immediately. Matches the user's
            // expectation that ^C kills the REPL (vs shell-like "clear line").
            Ok(reedline::Signal::CtrlC) | Ok(reedline::Signal::CtrlD) => break,
            Err(e) => {
                output::print_error(&format!("readline error: {e}"));
                break;
            }
        };
        // Inline-rewrap into the original `match read_line { Ok(_) => { ... } }`
        // body: the surrounding block uses `let trimmed = line.trim()` then a
        // `match trimmed { ... }`, all of which still works because `line`
        // shadows the rustyline-returned String here.
        match Ok::<usize, std::io::Error>(line.len()) {
            Ok(0) => continue, // empty line — original code did `continue` after trim
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let mut ctx = ReplCtx {
                    agent,
                    config,
                    workspace,
                    registry: &registry,
                    session_manager,
                    checkpoint_mgr,
                    session_id: &mut session_id,
                    active_session: &mut active_session,
                    tokens: &mut tokens,
                };
                match dispatch(trimmed, &mut ctx).await {
                    ReplAction::Quit => break,
                    ReplAction::Continue => continue,
                    ReplAction::Prompt { content, snapshot } => {
                        if snapshot {
                            // Idempotent — only writes the snapshot if this
                            // session doesn't have one yet. Covers /new +
                            // /fork paths that swapped in a fresh session.
                            session_manager
                                .append_config_snapshot_if_needed(
                                    &active_session,
                                    agent,
                                    &config.model,
                                )
                                .await;
                        }
                        let (in_tok, out_tok) =
                            event_handler::run_prompt(agent, &content, approval_rx).await;
                        tokens.input_tokens += in_tok;
                        tokens.output_tokens += out_tok;

                        // Persistence is automatic; refresh index summary +
                        // structured run record (same projection Tauri builds
                        // for Inspector).
                        let event_count = active_session.count(&EventQuery::default()).await;
                        session_manager.refresh_summary(&session_id, event_count, None);
                        session_manager.write_run_record(&active_session).await;
                    }
                }
            }
            Err(e) => {
                output::print_error(&format!("stdin error: {}", e));
                break;
            }
        }
    }

    // reedline persists incrementally via FileBackedHistory — no explicit
    // save needed on exit.

    // Final flush + analytics SessionEnd.
    emit_session_end(agent, &session_id, session_started_at);
    let _ = active_session.flush().await;
    eprintln!("Session saved: {}", session_id);
}

// === Line dispatch (CLI test layer 4) ===

/// What the REPL loop should do with one decoded line.
pub(crate) enum ReplAction {
    /// Exit the REPL.
    Quit,
    /// Line fully handled; read the next one.
    Continue,
    /// Run `content` as an agent prompt turn (loop persists the run record).
    /// `snapshot` = write the idempotent config snapshot first (plain user
    /// prompts do; registry-expanded prompts historically don't — preserved).
    Prompt { content: String, snapshot: bool },
}

/// Borrowed view of everything a REPL line can touch.
pub(crate) struct ReplCtx<'a> {
    pub agent: &'a BaseAgent,
    pub config: &'a ProviderConfig,
    pub workspace: &'a std::path::Path,
    pub registry: &'a CommandRegistry,
    pub session_manager: &'a JsonFileSessionManager,
    pub checkpoint_mgr: &'a checkpoint::CheckpointManager,
    pub session_id: &'a mut String,
    pub active_session: &'a mut Arc<JsonFileAgentSession>,
    pub tokens: &'a mut TokenUsage,
}

/// Decide one REPL line, terminal-free.
///
/// All stateful slash commands (session lifecycle, permission toggles,
/// checkpoint rewind, lock inspection, model switch, `!shell`) and the
/// `CommandRegistry` fallback live HERE; the read_line loop only executes
/// the returned action. Extracted so this decision layer is unit-testable
/// (diagnosis 2026-07-02 §5, layer 4).
pub(crate) async fn dispatch(trimmed: &str, ctx: &mut ReplCtx<'_>) -> ReplAction {
    let ReplCtx {
        agent,
        config,
        workspace,
        registry,
        session_manager,
        checkpoint_mgr,
        session_id,
        active_session,
        tokens,
    } = ctx;

    // === Commands handled directly by REPL (need mutable agent/state access) ===
    match trimmed {
        "/quit" | "/exit" => return ReplAction::Quit,
        "/clear" => {
            print!("\x1B[2J\x1B[1;1H");
            io::stdout().flush().ok();
            output::print_banner(&config.model, &workspace.display().to_string());
            output::print_git_status(workspace);
            output::print_banner_end();
            return ReplAction::Continue;
        }
        "/setup" => {
            eprintln!("  Reconfiguring requires restart.");
            if let Some(new_config) = crate::setup::run_setup_wizard(workspace) {
                eprintln!();
                eprintln!(
                    "  Configuration saved. Please restart alva-cli to use the new settings."
                );
                let _ = new_config;
            }
            return ReplAction::Continue;
        }
        "/plan" => {
            let current = agent.permission_mode();
            let new_mode = if current == PermissionMode::Plan {
                PermissionMode::Ask
            } else {
                PermissionMode::Plan
            };
            if !agent.set_permission_mode(new_mode) {
                eprintln!(
                    "  Cannot change permission mode — the `permission` component is disabled."
                );
            } else if new_mode == PermissionMode::Plan {
                eprintln!("  Plan mode ON — read-only, no file changes or commands");
            } else {
                eprintln!("  Plan mode OFF — tools can modify files");
            }
            return ReplAction::Continue;
        }
        "/auto" => {
            let current = agent.permission_mode();
            let new_mode = if current == PermissionMode::AcceptShell {
                PermissionMode::Ask
            } else {
                PermissionMode::AcceptShell
            };
            if !agent.set_permission_mode(new_mode) {
                eprintln!(
                    "  Cannot change permission mode — the `permission` component is disabled."
                );
            } else if new_mode == PermissionMode::AcceptShell {
                eprintln!("  Auto-shell ON — non-destructive shell commands run without prompting");
            } else {
                eprintln!("  Auto-shell OFF — shell commands ask for approval");
            }
            return ReplAction::Continue;
        }
        "/model" => {
            let current = agent.model_id().await;
            eprintln!("  Current model: {}", current);
            eprintln!("  Usage: /model <model_id>");
            return ReplAction::Continue;
        }
        "/rewind" => {
            handle_rewind(checkpoint_mgr);
            return ReplAction::Continue;
        }
        "/locks" => {
            if let Some(reg) = agent.bus().get::<alva_kernel_abi::ToolLockRegistry>() {
                let snap = reg.inspect();
                if snap.is_empty() {
                    eprintln!("  no active locks");
                } else {
                    eprintln!(
                        "  {:<32}  {:<5}  {:<24}  {}",
                        "key", "mode", "holder", "age"
                    );
                    for s in &snap {
                        let mode = match s.mode {
                            alva_kernel_abi::LockMode::Read => "read",
                            alva_kernel_abi::LockMode::Write => "write",
                        };
                        let holder = s.holder.as_deref().unwrap_or("-");
                        eprintln!(
                            "  {:<32}  {:<5}  {:<24}  {:.1?}",
                            truncate(&s.key, 32),
                            mode,
                            truncate(holder, 24),
                            s.age
                        );
                    }
                }
            } else {
                eprintln!("  ToolLockRegistry not available on bus");
            }
            return ReplAction::Continue;
        }
        "/new" => {
            let _ = active_session.flush().await;
            let new_session = session_manager.create("").await;
            **session_id = new_session.session_id().to_string();
            agent.swap_session(new_session.clone()).await;
            **active_session = new_session;
            **tokens = TokenUsage::default();
            output::print_session_new(&session_id);
            output::print_divider();
            return ReplAction::Continue;
        }
        "/fork" => {
            let _ = active_session.flush().await;
            let old_id = session_id.clone();
            // Load current messages before swapping
            let messages = agent.messages().await;
            let new_session = session_manager.create("").await;
            **session_id = new_session.session_id().to_string();
            // Copy messages into new session
            for msg in &messages {
                new_session.append_message(msg.clone(), None).await;
            }
            agent.swap_session(new_session.clone()).await;
            **active_session = new_session;
            eprintln!(
                "  Forked from {} → {}",
                &old_id[..8.min(old_id.len())],
                &session_id[..8.min(session_id.len())]
            );
            eprintln!(
                "  {} messages carried over. Try a different approach.",
                messages.len()
            );
            output::print_divider();
            return ReplAction::Continue;
        }
        "/resume" => {
            if let Some((new_id, new_session)) =
                handle_resume(agent, session_manager, active_session, session_id).await
            {
                **session_id = new_id;
                **active_session = new_session;
            }
            output::print_divider();
            return ReplAction::Continue;
        }
        "/sessions" => {
            handle_sessions(session_manager, &session_id);
            return ReplAction::Continue;
        }
        _ => {} // Fall through to registry or model/shell handling
    }

    // === /model <id> ===
    if let Some(model_id) = trimmed.strip_prefix("/model ") {
        let model_id = model_id.trim();
        if !model_id.is_empty() {
            let mut new_config = config.clone();
            new_config.model = model_id.to_string();
            // Canonical factory (PR-10) — this was a sixth copied
            // match, and worse: it hardcoded the chat provider
            // regardless of the configured kind.
            let kind = new_config.kind.clone();
            let new_model = alva_llm_provider::build_language_model(kind.as_deref(), new_config);
            agent.set_model(new_model).await;
            eprintln!("  Switched to model: {}", model_id);
        }
        return ReplAction::Continue;
    }

    // === !shell ===
    if trimmed.starts_with('!') {
        handle_shell(trimmed, workspace);
        return ReplAction::Continue;
    }

    // === Slash commands via registry ===
    if trimmed.starts_with('/') {
        let message_count = agent.messages().await.len();
        let tool_names = agent.tool_names();
        let plugin_names = agent.plugin_names();
        let middleware_names = agent.middleware_names();
        let plan_mode = agent.permission_mode() == PermissionMode::Plan;
        let ctx = build_command_context(
            workspace,
            config,
            &session_id,
            message_count,
            &tokens,
            tool_names,
            plugin_names,
            middleware_names,
            plan_mode,
        );

        if let Some(result) = registry.execute(trimmed, &ctx) {
            match result {
                CommandResult::Text(text) => {
                    eprintln!("{}", text);
                }
                CommandResult::Prompt {
                    content,
                    progress_message,
                    ..
                } => {
                    if let Some(msg) = &progress_message {
                        eprintln!("  {}", msg);
                    }
                    return ReplAction::Prompt {
                        content,
                        snapshot: false,
                    };
                }
                CommandResult::Compact { summary } => {
                    eprintln!("  {}", summary);
                    // Compaction runs as a prompt turn.
                    return ReplAction::Prompt {
                        content: summary,
                        snapshot: false,
                    };
                }
                CommandResult::Error(e) => {
                    if e.starts_with("Unknown command:") {
                        let invocation = trimmed
                            .strip_prefix('/')
                            .unwrap_or(trimmed)
                            .split_once(char::is_whitespace)
                            .map(|(name, args)| (name, Some(args.trim())))
                            .unwrap_or_else(|| {
                                (trimmed.strip_prefix('/').unwrap_or(trimmed), None)
                            });

                        match agent.invoke_skill(invocation.0, invocation.1).await {
                            Some(Ok(content)) => {
                                return ReplAction::Prompt {
                                    content,
                                    snapshot: false,
                                };
                            }
                            Some(Err(SkillRegistryError::Load(error))) => {
                                output::print_error(&format!(
                                    "Failed to load skill '{}': {error}",
                                    invocation.0
                                ));
                            }
                            Some(Err(SkillRegistryError::NotFound(_))) | None => {
                                // Preserve the registry's established unknown-command error
                                // when no skill with the exact slash name exists.
                                output::print_error(&e);
                            }
                        }
                    } else {
                        output::print_error(&e);
                    }
                }
            }
        }
        return ReplAction::Continue;
    }

    // === Regular prompt ===
    ReplAction::Prompt {
        content: trimmed.to_string(),
        snapshot: true,
    }
}

// === reedline Prompt — matches the previous ">" cyan look ===

struct ReplPrompt;

impl reedline::Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }
    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }
    fn render_prompt_indicator(
        &self,
        _mode: reedline::PromptEditMode,
    ) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("> ")
    }
    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("· ")
    }
    fn render_prompt_history_search_indicator(
        &self,
        history_search: reedline::PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(format!("(reverse-i-search '{}'): ", history_search.term))
    }
}

// === Extracted handlers for complex commands that need mutable state ===

/// Emit `AnalyticsEvent::SessionStart` if an `AnalyticsSink` is on the bus.
/// No-op if the analytics extension wasn't installed.
fn emit_session_start(agent: &BaseAgent, session_id: &str, workspace: &std::path::Path) {
    if let Some(sink) = agent.bus().get::<dyn alva_kernel_abi::AnalyticsSink>() {
        sink.record(alva_kernel_abi::AnalyticsEvent::SessionStart {
            session_id: session_id.to_string(),
            workspace: workspace.to_path_buf(),
            ts: std::time::SystemTime::now(),
        });
    }
}

/// Emit `AnalyticsEvent::SessionEnd`. Duration is wall-clock since
/// `started_at` (captured at the matching `SessionStart`).
fn emit_session_end(agent: &BaseAgent, session_id: &str, started_at: std::time::Instant) {
    if let Some(sink) = agent.bus().get::<dyn alva_kernel_abi::AnalyticsSink>() {
        sink.record(alva_kernel_abi::AnalyticsEvent::SessionEnd {
            session_id: session_id.to_string(),
            duration_ms: started_at.elapsed().as_millis() as u64,
            ts: std::time::SystemTime::now(),
        });
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn handle_rewind(checkpoint_mgr: &checkpoint::CheckpointManager) {
    let checkpoints = checkpoint_mgr.list();
    if checkpoints.is_empty() {
        eprintln!("  No checkpoints available.");
        return;
    }
    eprintln!("  Checkpoints:");
    for (i, cp) in checkpoints.iter().enumerate().take(10) {
        let date = chrono::DateTime::from_timestamp_millis(cp.created_at)
            .map(|d| d.format("%H:%M:%S").to_string())
            .unwrap_or_default();
        eprintln!(
            "  [{}] {} | {} files | {}",
            i + 1,
            date,
            cp.files.len(),
            cp.description
        );
    }
    eprint!(
        "  Rewind to [1-{}] or Enter to cancel: ",
        checkpoints.len().min(10)
    );
    io::stderr().flush().ok();

    let mut choice = String::new();
    if io::stdin().lock().read_line(&mut choice).is_ok() {
        let trimmed = choice.trim();
        if !trimmed.is_empty() {
            if let Ok(idx) = trimmed.parse::<usize>() {
                if idx >= 1 && idx <= checkpoints.len().min(10) {
                    let cp = &checkpoints[idx - 1];
                    match checkpoint_mgr.rewind(&cp.id) {
                        Ok(restored) => {
                            eprintln!(
                                "  Restored {} files from checkpoint {}",
                                restored.len(),
                                cp.id
                            );
                            for f in &restored {
                                eprintln!("    - {}", f);
                            }
                        }
                        Err(e) => {
                            output::print_error(&format!("rewind failed: {}", e));
                        }
                    }
                }
            }
        }
    }
}

async fn handle_resume(
    agent: &BaseAgent,
    session_manager: &JsonFileSessionManager,
    active_session: &Arc<JsonFileAgentSession>,
    current_session_id: &str,
) -> Option<(String, Arc<JsonFileAgentSession>)> {
    // Flush current session before switching.
    let _ = active_session.flush().await;

    let sessions = session_manager.list();
    if sessions.is_empty() {
        output::print_error("No sessions found.");
        return None;
    }

    eprintln!("Sessions:");
    for (i, s) in sessions.iter().enumerate().take(10) {
        let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
            .map(|d| d.format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let marker = if s.session_id == current_session_id {
            " ◀"
        } else {
            ""
        };
        eprintln!(
            "  [{}] {} | {} events | {}{}",
            i + 1,
            date,
            s.event_count,
            s.preview,
            marker,
        );
    }
    eprint!("Pick [1-{}] or Enter for latest: ", sessions.len().min(10));
    io::stderr().flush().ok();

    let mut choice = String::new();
    if io::stdin().lock().read_line(&mut choice).is_ok() {
        let idx: usize = choice.trim().parse().unwrap_or(1);
        if idx >= 1 && idx <= sessions.len().min(10) {
            let picked: &SessionSummary = &sessions[idx - 1];
            let new_id = picked.session_id.clone();

            let new_session = match session_manager.load(&new_id).await {
                Some(s) => s,
                None => {
                    output::print_error(&format!("session {} not found on disk", new_id));
                    return None;
                }
            };
            agent.swap_session(new_session.clone()).await;

            let msg_count = agent.messages().await.len();
            output::print_session_resumed(&new_id, msg_count, &picked.preview);
            return Some((new_id, new_session));
        }
    }
    None
}

fn handle_sessions(session_manager: &JsonFileSessionManager, current_session_id: &str) {
    let sessions = session_manager.list();
    if sessions.is_empty() {
        eprintln!("No sessions.");
    } else {
        for s in sessions.iter().take(20) {
            let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
                .map(|d| d.format("%m-%d %H:%M").to_string())
                .unwrap_or_default();
            let marker = if s.session_id == current_session_id {
                " ◀"
            } else {
                ""
            };
            eprintln!(
                "  {} | {} events | {}{}",
                date, s.event_count, s.preview, marker,
            );
        }
    }
}

fn handle_shell(cmd: &str, workspace: &std::path::Path) {
    let shell_cmd = cmd[1..].trim();
    if shell_cmd.is_empty() {
        output::print_error("Usage: !<command>");
        return;
    }
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(shell_cmd)
        .current_dir(workspace)
        .output()
    {
        Ok(out) => {
            if !out.stdout.is_empty() {
                print!("{}", String::from_utf8_lossy(&out.stdout));
            }
            if !out.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&out.stderr));
            }
            if !out.status.success() {
                output::print_error(&format!("exit code: {}", out.status.code().unwrap_or(-1)));
            }
        }
        Err(e) => output::print_error(&format!("failed to execute: {}", e)),
    }
}

// ---------------------------------------------------------------------------
// Tests — dispatch() decides each REPL line terminal-free. These drive the
// real dispatch against a real agent + real session manager (mock model,
// tempdir), asserting the returned ReplAction and the state mutations —
// the CLI's fourth interaction surface, previously untestable inside the
// read_line loop.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;

    async fn harness() -> (
        BaseAgent,
        ProviderConfig,
        JsonFileSessionManager,
        checkpoint::CheckpointManager,
        CommandRegistry,
        String,
        Arc<JsonFileAgentSession>,
        tempfile::TempDir,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        let skills = ws.join("skills");
        let repl_skill = skills.join("bundled").join("repl-fallback-skill");
        std::fs::create_dir_all(&repl_skill).unwrap();
        std::fs::write(
            repl_skill.join("SKILL.md"),
            "---\nname: repl-fallback-skill\ndescription: REPL fallback fixture\ninvocation: explicit\n---\n\nREPL_FALLBACK_BODY\n",
        )
        .unwrap();
        let agent = BaseAgent::builder()
            .workspace(ws)
            .system_prompt("t")
            .plugin(Box::new(alva_app_core::extension::PermissionPlugin::new()))
            .plugin(Box::new(
                alva_app_core::extension::SkillsPlugin::with_bundled(skills, None),
            ))
            .build(Arc::new(
                MockLanguageModel::new().with_response(make_assistant_message("x")),
            ))
            .await
            .unwrap();
        let config = ProviderConfig {
            api_key: "k".into(),
            model: "m".into(),
            base_url: "http://x".into(),
            max_tokens: 16,
            custom_headers: Default::default(),
            kind: None,
        };
        let mgr = JsonFileSessionManager::for_workspace(ws);
        let ckpt = checkpoint::CheckpointManager::new(ws);
        let registry = CommandRegistry::new();
        let session = mgr.create("").await;
        let sid = session.session_id().to_string();
        (agent, config, mgr, ckpt, registry, sid, session, tmp)
    }

    macro_rules! ctx {
        ($agent:expr, $config:expr, $ws:expr, $registry:expr, $mgr:expr, $ckpt:expr, $sid:expr, $sess:expr, $tok:expr) => {
            ReplCtx {
                agent: &$agent,
                config: &$config,
                workspace: $ws,
                registry: &$registry,
                session_manager: &$mgr,
                checkpoint_mgr: &$ckpt,
                session_id: &mut $sid,
                active_session: &mut $sess,
                tokens: &mut $tok,
            }
        };
    }

    #[tokio::test]
    async fn quit_and_exit_return_quit() {
        let (agent, config, mgr, ckpt, registry, mut sid, mut sess, tmp) = harness().await;
        let mut tok = TokenUsage::default();
        for line in ["/quit", "/exit"] {
            let mut c = ctx!(
                agent,
                config,
                tmp.path(),
                registry,
                mgr,
                ckpt,
                sid,
                sess,
                tok
            );
            assert!(
                matches!(dispatch(line, &mut c).await, ReplAction::Quit),
                "{line}"
            );
        }
    }

    #[tokio::test]
    async fn plain_text_becomes_a_prompt_with_snapshot() {
        let (agent, config, mgr, ckpt, registry, mut sid, mut sess, tmp) = harness().await;
        let mut tok = TokenUsage::default();
        let mut c = ctx!(
            agent,
            config,
            tmp.path(),
            registry,
            mgr,
            ckpt,
            sid,
            sess,
            tok
        );
        match dispatch("hello there", &mut c).await {
            ReplAction::Prompt { content, snapshot } => {
                assert_eq!(content, "hello there");
                assert!(snapshot, "plain prompts write the config snapshot");
            }
            other => panic!(
                "expected Prompt, got a different action: {}",
                matches!(other, ReplAction::Continue)
            ),
        }
    }

    #[tokio::test]
    async fn plan_toggles_permission_mode_and_continues() {
        let (agent, config, mgr, ckpt, registry, mut sid, mut sess, tmp) = harness().await;
        let mut tok = TokenUsage::default();
        assert_ne!(agent.permission_mode(), PermissionMode::Plan);
        {
            let mut c = ctx!(
                agent,
                config,
                tmp.path(),
                registry,
                mgr,
                ckpt,
                sid,
                sess,
                tok
            );
            assert!(matches!(
                dispatch("/plan", &mut c).await,
                ReplAction::Continue
            ));
        }
        assert_eq!(
            agent.permission_mode(),
            PermissionMode::Plan,
            "/plan turns plan mode ON"
        );
        {
            let mut c = ctx!(
                agent,
                config,
                tmp.path(),
                registry,
                mgr,
                ckpt,
                sid,
                sess,
                tok
            );
            assert!(matches!(
                dispatch("/plan", &mut c).await,
                ReplAction::Continue
            ));
        }
        assert_ne!(
            agent.permission_mode(),
            PermissionMode::Plan,
            "/plan again toggles OFF"
        );
    }

    #[tokio::test]
    async fn new_swaps_in_a_fresh_session_and_resets_tokens() {
        let (agent, config, mgr, ckpt, registry, mut sid, mut sess, tmp) = harness().await;
        let mut tok = TokenUsage {
            input_tokens: 5,
            output_tokens: 7,
        };
        let old_sid = sid.clone();
        let mut c = ctx!(
            agent,
            config,
            tmp.path(),
            registry,
            mgr,
            ckpt,
            sid,
            sess,
            tok
        );
        assert!(matches!(
            dispatch("/new", &mut c).await,
            ReplAction::Continue
        ));
        assert_ne!(sid, old_sid, "/new allocates a new session id");
        assert_eq!(tok.input_tokens, 0, "/new resets token accounting");
        assert_eq!(tok.output_tokens, 0);
    }

    #[tokio::test]
    async fn unknown_slash_is_handled_not_prompted() {
        // A "/…" line must never fall through to a prompt turn (that would
        // send the slash text to the model). Registry handles or reports it;
        // either way dispatch returns Continue.
        let (agent, config, mgr, ckpt, registry, mut sid, mut sess, tmp) = harness().await;
        let mut tok = TokenUsage::default();
        let mut c = ctx!(
            agent,
            config,
            tmp.path(),
            registry,
            mgr,
            ckpt,
            sid,
            sess,
            tok
        );
        assert!(matches!(
            dispatch("/definitely-not-a-command", &mut c).await,
            ReplAction::Continue
        ));
    }

    #[tokio::test]
    async fn unknown_slash_matching_skill_injects_body_without_model_tool_selection() {
        let (agent, config, mgr, ckpt, registry, mut sid, mut sess, tmp) = harness().await;
        let mut tok = TokenUsage::default();
        let mut c = ctx!(
            agent,
            config,
            tmp.path(),
            registry,
            mgr,
            ckpt,
            sid,
            sess,
            tok
        );

        match dispatch("/repl-fallback-skill focus here", &mut c).await {
            ReplAction::Prompt { content, snapshot } => {
                assert!(
                    content.contains("## Skill: repl-fallback-skill"),
                    "{content}"
                );
                assert!(content.contains("REPL_FALLBACK_BODY"), "{content}");
                assert!(content.contains("focus here"), "{content}");
                assert!(
                    !snapshot,
                    "slash skill fallback is already harness-expanded"
                );
            }
            _ => panic!("matching slash skill should become a direct injected prompt"),
        }
    }
}
