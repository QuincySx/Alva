//! alva-cli — Minimal CLI agent with session management.
//!
//! Usage:
//!   export ALVA_API_KEY=sk-...
//!   cargo run -p alva-app-core --bin alva-cli
//!
//! Commands:
//!   /new             Start a fresh session
//!   /resume          Resume latest session (or pick from list)
//!   /sessions        List all sessions in current directory
//!   /help            Show available commands
//!   /clear           Clear terminal
//!   /config          Show current config
//!   /quit /exit      Exit
//!   !<cmd>           Run shell command directly
//!
//! Sessions are stored under `.alva/sessions/` in the working directory.

mod agent_setup;
mod bundled_skills;
mod checkpoint;
mod commands;
mod context;
mod event_handler;
mod jobs_cmd;
mod output;
mod plugins;
mod providers_cmd;
mod repl;
mod repl_completer;
pub mod services;
mod session;
mod settings_cmd;
mod setup;
mod tools_cmd;
pub mod ui;

use std::io::{self, IsTerminal as _, Read as _};

use alva_app_core::AlvaPaths;
use alva_kernel_abi::agent_session::EventQuery;
use alva_kernel_abi::AgentSession;
use alva_llm_provider::ProviderConfig;

use session::JsonFileSessionManager;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    // run() returns an exit code; all destructors for its stack locals
    // (agent, extensions, subprocess loader, plugin proxies...) run
    // inside block_on() before we drop the runtime. Previously we used
    // `std::process::exit()` inside run(), which bypassed all Drop
    // impls and caused loaded subprocess plugins to be reparented
    // instead of cleanly shut down.
    let exit_code = rt.block_on(run());

    // Give in-flight tokio tasks a bounded window to finish (reader /
    // writer loops on subprocess stdio shutting down cleanly, etc.).
    rt.shutdown_timeout(std::time::Duration::from_secs(5));

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

async fn run() -> i32 {
    // Early dispatch: `alva plugins ...` and `alva context ...`
    // short-circuit before any agent setup — pure debugging commands
    // that need no LLM config or session init.
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(1).map(|s| s.as_str()) {
        Some("--help") | Some("-h") | Some("help") => {
            print!("{}", usage_text());
            return 0;
        }
        Some("plugins") => return plugins::run(&argv[2..]).await,
        Some("tools") => return tools_cmd::run(&argv[2..]).await,
        Some("providers") => return providers_cmd::run(&argv[2..]).await,
        Some("jobs") => return jobs_cmd::run(&argv[2..]).await,
        Some("context") => return context::run(&argv[2..]).await,
        Some("settings") => return settings_cmd::run(&argv[2..]).await,
        _ => {}
    }

    // Cross-process recursion gate — the sibling of the in-process
    // `agent`-tool depth limit, governed by the SAME `subagent_depth` config
    // knob. Workers with shell access can run `alva -p ...` themselves;
    // every agent run therefore carries its nesting depth in
    // ALVA_AGENT_DEPTH: refuse at the limit, export depth+1 so any alva a
    // tool shells out inherits it. Placed BEFORE provider/key resolution so
    // runaway recursion is stopped without spending a token. Non-agent
    // subcommands (tools/providers/jobs/settings/…) returned above and are
    // usable at any depth.
    let depth_limit = alva_app_core::config::load()
        .and_then(|c| c.subagent_depth)
        .unwrap_or(alva_app_core::components::DEFAULT_SUBAGENT_DEPTH);
    match recursion_gate(
        std::env::var("ALVA_AGENT_DEPTH").ok().as_deref(),
        depth_limit,
    ) {
        Ok(next_depth) => std::env::set_var("ALVA_AGENT_DEPTH", next_depth.to_string()),
        Err(e) => {
            output::print_error(&e);
            return 1;
        }
    }

    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let paths = AlvaPaths::new(&workspace);

    // --system-prompt must be known BEFORE the agent is built (the prompt
    // is a builder-time input); parsed here, consumed by build_agent, and
    // skipped again by the -p prompt collector below.
    let system_prompt_override: Option<String> = argv
        .iter()
        .position(|a| a == "--system-prompt")
        .and_then(|i| argv.get(i + 1).cloned());

    // `--max-turns <N>` — per-invocation turn budget (a builder-time input,
    // like --system-prompt). Garbage fails loudly: silently keeping the
    // default would hand a mis-typed budget to an expensive run.
    let max_turns: Option<u32> = match argv.iter().position(|a| a == "--max-turns") {
        Some(i) => match argv.get(i + 1).map(|v| v.parse::<u32>()) {
            Some(Ok(n)) if n >= 1 => Some(n),
            _ => {
                output::print_error(
                    "--max-turns expects a positive integer (the turn budget for this run)",
                );
                return 1;
            }
        },
        None => None,
    };

    // `--provider <name>` — pick a NAMED profile from the shared config for
    // this invocation. Parsed here (config resolution is pre-agent), skipped
    // again by the -p prompt collector below.
    let provider_flag: Option<String> = match argv.iter().position(|a| a == "--provider") {
        Some(i) => match argv.get(i + 1) {
            Some(name) if !name.is_empty() && !name.starts_with('-') => Some(name.clone()),
            _ => {
                output::print_error(
                    "--provider expects a profile name (see `alva providers list`)",
                );
                return 1;
            }
        },
        None => None,
    };

    // 1. Config — resolution order:
    //    0. `--provider <name>` — explicit per-invocation profile pick; only
    //       env vars still override its individual fields
    //    a. env vars (ALVA_*) — `ProviderConfig::load` will pick these up
    //    b. project / global flat-format files (legacy CLI paths)
    //    c. shared `~/.alva/config.json` active provider — same file Tauri reads
    //    d. setup wizard
    //
    // The shared config is checked AFTER env vars + legacy files so old setups
    // keep working unchanged. New users go through the wizard or `alva settings set`.
    let config = if let Some(name) = &provider_flag {
        match settings_cmd::load_provider_named(name) {
            Ok(mut c) => {
                apply_env_overrides(&mut c);
                c
            }
            Err(e) => {
                output::print_error(&e);
                return 1;
            }
        }
    } else {
        match ProviderConfig::load(&workspace) {
            Ok(c) if !c.api_key.is_empty() => c,
            _ => {
                match settings_cmd::try_load_provider_from_shared() {
                    Some(mut shared) => {
                        apply_env_overrides(&mut shared);
                        shared
                    }
                    None => {
                        // The interactive setup wizard reads from stdin. In a
                        // non-interactive run (CI / `echo ... | alva -p`) stdin
                        // carries the *prompt*, not menu answers — entering the
                        // wizard would consume that piped input and garble the
                        // run. Fail fast with config instructions instead.
                        if !std::io::stdin().is_terminal() {
                            output::print_error(
                                "No API key configured and stdin is not a terminal, so the \
                             interactive setup wizard can't run.",
                            );
                            eprintln!("Configure a provider first, e.g.:");
                            eprintln!("  export ALVA_API_KEY=sk-...");
                            eprintln!("  export ALVA_MODEL=gpt-4o");
                            eprintln!("  alva settings set anthropic --api-key sk-... --model claude-opus-4-7");
                            return 1;
                        }
                        match setup::run_setup_wizard(&workspace) {
                            Some(c) => c,
                            None => {
                                eprintln!();
                                output::print_error(
                                    "Setup incomplete. You can also configure manually:",
                                );
                                eprintln!("  export ALVA_API_KEY=sk-...");
                                eprintln!("  export ALVA_MODEL=gpt-4o");
                                eprintln!("  alva settings set anthropic --api-key sk-... --model claude-opus-4-7");
                                return 1;
                            }
                        }
                    }
                }
            }
        }
    };

    // Defense-in-depth: the API key has now been resolved into `config`
    // (in memory). Scrub the secret-bearing env vars from THIS process so a
    // child shell spawned by the agent's `execute_shell` tool cannot read
    // them back — neither via `printenv`/`env`/`$ALVA_API_KEY` (child inherits
    // our env) nor via `/proc/<our-pid>/environ` on Linux (a child running as
    // the same user can read the parent's environment). The provider already
    // holds the key, so removing it here does not affect outbound LLM calls.
    // This is `safe` on edition 2021; it runs before the agent (and its tool
    // threads) are built, minimizing concurrent-env-access risk.
    for var in ["ALVA_API_KEY", "ALVA_AUTH_TOKEN"] {
        std::env::remove_var(var);
    }

    let session_manager = JsonFileSessionManager::for_workspace(&workspace);

    // 2. Build agent (provider, skills, approval channel, checkpoint callback)
    let agent_setup::AgentBundle {
        agent,
        mut approval_rx,
        checkpoint_mgr: _checkpoint_mgr,
    } = agent_setup::build_agent(
        &config,
        &workspace,
        &paths,
        system_prompt_override.as_deref(),
        max_turns,
    )
    .await;

    // 2b. Apply --permission-mode (headless permission control; mirrors
    //     Claude Code / Codex). Applies to all run modes. Default is left
    //     untouched (Ask) when the flag is absent.
    let args: Vec<String> = std::env::args().collect();

    // 3. Check for -p/--print mode (non-interactive, single prompt, stdout-only).
    //    Computed before applying --permission-mode because the sandbox gate
    //    below behaves differently in headless vs interactive runs.
    let print_mode = args.iter().any(|a| a == "-p" || a == "--print");

    match parse_permission_mode(&args) {
        Ok(Some(mode)) => {
            // CLI#7: `accept-shell` / `bypass` auto-run commands on the
            // assumption that an OS sandbox will contain them. When no sandbox
            // is actually enforced (today: any non-macOS platform) that
            // assumption is false, so refuse in headless mode and require an
            // explicit confirmation interactively rather than silently running
            // unsandboxed commands with elevated permissions.
            if mode.assumes_sandbox() && !alva_app_core::SandboxConfig::is_enforced() {
                // Explicit escape hatch (headless orchestration on Linux
                // servers / CI where the environment itself is the
                // isolation): the DEFAULT stays refuse; only a flag scary
                // enough that nobody types it by accident opens the gate —
                // same philosophy as docker --privileged.
                let dangerously_allowed =
                    args.iter().any(|a| a == "--dangerously-allow-unsandboxed");
                if print_mode && dangerously_allowed {
                    eprintln!(
                        "WARNING: --dangerously-allow-unsandboxed — running --permission-mode {} \
                         with NO OS sandbox. Tools execute with this process's full privileges; \
                         only do this inside an isolated environment (container/VM/CI runner).",
                        permission_mode_label(mode),
                    );
                } else if print_mode {
                    output::print_error(&format!(
                        "--permission-mode {} assumes an OS sandbox, but none is enforced on \
                         this platform.\nRefusing to auto-run commands unsandboxed in headless \
                         (-p) mode. Use `--permission-mode ask`, run on a platform with sandbox \
                         support, or — if this environment is ITSELF the isolation (container/\
                         CI) — pass --dangerously-allow-unsandboxed.",
                        permission_mode_label(mode),
                    ));
                    return 1;
                }
                if !print_mode && !confirm_unsandboxed_mode(permission_mode_label(mode)) {
                    output::print_error("Aborted: unsandboxed elevated permission mode declined.");
                    return 1;
                }
            }
            // The user *explicitly* asked for a mode, so a silent no-op is a
            // safety surprise (e.g. they think they're in read-only `plan` but
            // the `permission` component is disabled and tools still run).
            if !agent.set_permission_mode(mode) {
                output::print_error(&format!(
                    "--permission-mode {} was requested, but the `permission` component is \
                     disabled, so it has no effect.\nEnable it in ~/.alva/config.json (or remove \
                     the flag).",
                    permission_mode_label(mode),
                ));
                return 1;
            }
        }
        Ok(None) => {}
        Err(e) => {
            output::print_error(&e);
            return 1;
        }
    }

    if print_mode {
        // Collect the prompt from positional args, skipping the print flags
        // and the flag pairs (`--permission-mode X`, `--output-format X`) so
        // they don't leak into the prompt text.
        let mut prompt_args: Vec<String> = Vec::new();
        let mut output_json = false;
        let mut resume_id: Option<String> = None;
        let mut allowed_tools: Option<Vec<String>> = None;
        let mut i = 1; // skip binary name
        while i < args.len() {
            let a = &args[i];
            if a == "-p" || a == "--print" || a == "--dangerously-allow-unsandboxed" {
                i += 1;
                continue;
            }
            if a == "--permission-mode"
                || a == "--system-prompt"
                || a == "--provider"
                || a == "--max-turns"
            {
                i += 2; // skip the flag and its value (all consumed pre-build)
                continue;
            }
            if a == "--resume" {
                match args.get(i + 1) {
                    Some(id) if !id.is_empty() => resume_id = Some(id.clone()),
                    _ => {
                        eprintln!("Error: --resume expects a session id (from a prior --output-format json run)");
                        return 1;
                    }
                }
                i += 2;
                continue;
            }
            if a == "--allowed-tools" {
                match args.get(i + 1) {
                    Some(list) if !list.is_empty() => {
                        allowed_tools =
                            Some(list.split(',').map(|s| s.trim().to_string()).collect());
                    }
                    _ => {
                        eprintln!("Error: --allowed-tools expects a comma-separated tool list (see `alva tools list`)");
                        return 1;
                    }
                }
                i += 2;
                continue;
            }
            if a == "--output-format" {
                match args.get(i + 1).map(String::as_str) {
                    Some("json") => output_json = true,
                    Some("text") => output_json = false,
                    other => {
                        eprintln!(
                            "Error: --output-format expects `text` or `json`, got {:?}",
                            other.unwrap_or("<missing>")
                        );
                        return 1;
                    }
                }
                i += 2;
                continue;
            }
            prompt_args.push(a.clone());
            i += 1;
        }

        let prompt = if prompt_args.is_empty() {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).ok();
            buf.trim().to_string()
        } else {
            prompt_args.join(" ")
        };

        if prompt.is_empty() {
            eprintln!(
                "Error: no prompt provided in -p mode.\n\
                 Hint: pass the prompt as an argument or pipe it via stdin:\n\
                 \x20\x20alva -p \"summarize README.md\"\n\
                 \x20\x20echo \"summarize README.md\" | alva -p\n\
                 See `alva --help` for flags (including --permission-mode)."
            );
            return 1;
        }

        // Per-invocation tool allowlist. Unknown names fail LOUDLY (exit 1,
        // list what exists) — a typo that silently allows nothing is the
        // component-toggle lesson all over again.
        if let Some(allowed) = &allowed_tools {
            let known = agent.tool_names();
            let unknown: Vec<&String> = allowed.iter().filter(|a| !known.contains(a)).collect();
            if !unknown.is_empty() {
                eprintln!(
                    "Error: --allowed-tools names unknown tool(s): {:?}\nKnown tools: run `alva tools list`",
                    unknown
                );
                return 1;
            }
            agent.retain_tools(allowed).await;
        }

        // Persist the run like every other mode: a real session on disk is
        // what makes the returned session_id a usable --resume handle, and
        // gives external tooling the same RunRecord projection. --resume
        // loads that prior session so the worker continues with full history.
        let print_session = match &resume_id {
            Some(id) => match session_manager.load(id).await {
                Some(sess) => sess,
                None => {
                    eprintln!("Error: --resume {id}: session not found in this workspace");
                    return 1;
                }
            },
            None => session_manager.create(&prompt).await,
        };
        agent.swap_session(print_session.clone()).await;
        session_manager
            .append_config_snapshot_if_needed(&print_session, &agent, &config.model)
            .await;

        let exit_code = if output_json {
            let started = std::time::Instant::now();
            let mut err = io::stderr();
            let outcome =
                event_handler::run_print_mode_collect(&agent, &prompt, &mut approval_rx, &mut err)
                    .await;
            let is_error = outcome.error.is_some();
            // ONE json object on stdout — the whole machine contract.
            println!(
                "{}",
                serde_json::json!({
                    "type": "result",
                    "result": outcome.text,
                    "is_error": is_error,
                    "error": outcome.error,
                    "session_id": print_session.session_id(),
                    "usage": {
                        "input_tokens": outcome.input_tokens,
                        "output_tokens": outcome.output_tokens,
                    },
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
            );
            i32::from(is_error)
        } else {
            event_handler::run_print_mode(&agent, &prompt, &mut approval_rx).await
        };

        let event_count = print_session.count(&EventQuery::default()).await;
        session_manager.refresh_summary(print_session.session_id(), event_count, Some(&prompt));
        session_manager.write_run_record(&print_session).await;
        return exit_code;
    }

    // 4. Print banner (interactive modes only)
    output::print_banner(&config.model, &workspace.display().to_string());
    output::print_git_status(&workspace);
    output::print_banner_end();

    // 5. Interactive — TUI by default. Inline (20 rows) unless explicitly
    //    flipped via flag/env/config. `--repl` falls back to the legacy
    //    reedline line-mode for users who prefer it.
    //
    //    UI mode resolution order (first hit wins):
    //      1. `--ui-mode <inline|fullscreen>` flag
    //      2. `ALVA_UI_MODE` env var
    //      3. `ui_mode` field in ~/.alva/config.json (shared with Tauri)
    //      4. default "inline"
    let cli_args: Vec<String> = std::env::args().collect();
    let want_repl = cli_args.iter().any(|a| a == "--repl")
        || std::env::var("ALVA_REPL").ok().as_deref() == Some("1");
    if !want_repl {
        let mut mode_override: Option<String> = None;
        if let Some(i) = cli_args.iter().position(|a| a == "--ui-mode") {
            mode_override = cli_args.get(i + 1).cloned();
        }
        if mode_override.is_none() {
            if let Ok(env_mode) = std::env::var("ALVA_UI_MODE") {
                if !env_mode.is_empty() {
                    mode_override = Some(env_mode);
                }
            }
        }
        let shared_cfg = alva_app_core::config::load();
        let cfg_mode = shared_cfg.as_ref().and_then(|c| c.ui_mode.clone());
        let cfg_inline_rows = shared_cfg.as_ref().and_then(|c| c.ui_inline_rows);
        let mode_str = mode_override.or(cfg_mode);
        let viewport = ui::app::UiViewport::parse(mode_str.as_deref(), cfg_inline_rows);
        if let Err(e) = ui::app::run_tui(
            &agent,
            &config,
            &workspace,
            &paths,
            &session_manager,
            &_checkpoint_mgr,
            &mut approval_rx,
            viewport,
        )
        .await
        {
            output::print_error(&format!("TUI exited with error: {e}"));
            return 1;
        }
        return 0;
    }

    // 6. Check for single-shot mode (positional arg without leading -).
    //    Skips any value that follows `--ui-mode`/`--print` so flag
    //    arguments don't get treated as prompts.
    let argv: Vec<String> = std::env::args().collect();
    let mut prompt_arg: Option<String> = None;
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == "--ui-mode" || a == "--permission-mode" {
            i += 2; // flag + its value
            continue;
        }
        if a == "-p" || a == "--print" || a == "--tui" || a == "--repl" {
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        prompt_arg = Some(a.clone());
        break;
    }
    if let Some(prompt) = prompt_arg {
        let initial_session = session_manager.create(&prompt).await;
        agent.swap_session(initial_session.clone()).await;
        // Append eval_config_snapshot before the first turn so RunRecord
        // captures the actual model + tool/skill/extension config used.
        // Same shape as Tauri's append_config_snapshot_if_needed.
        session_manager
            .append_config_snapshot_if_needed(&initial_session, &agent, &config.model)
            .await;
        event_handler::run_prompt(&agent, &prompt, &mut approval_rx).await;
        // Persistence is automatic — refresh the index summary, then drop a
        // structured RunRecord next to the raw event log so external tooling
        // gets the same turn / llm-call / tool-call view Tauri builds for its
        // Inspector. Same projection (alva_app_core::session_projection).
        let event_count = initial_session.count(&EventQuery::default()).await;
        session_manager.refresh_summary(initial_session.session_id(), event_count, Some(&prompt));
        session_manager.write_run_record(&initial_session).await;
        return 0;
    }

    // 7. Interactive REPL (default when not TUI / not single-shot).
    repl::run_repl(
        &agent,
        &config,
        &workspace,
        &paths,
        &session_manager,
        &_checkpoint_mgr,
        &mut approval_rx,
    )
    .await;

    0
}

/// Top-level CLI usage text. Kept as a function so the documented flag
/// inventory is unit-tested — a flag that exists but isn't advertised here is
/// a discoverability bug, especially for an AI driving the CLI headlessly.
fn usage_text() -> String {
    "\
alva — coding agent CLI

USAGE:
    alva [FLAGS] [PROMPT]          Interactive (TUI by default)
    alva -p [FLAGS] \"PROMPT\"       Non-interactive: run one prompt, stream to stdout, exit
    echo \"PROMPT\" | alva -p        Non-interactive: read the prompt from stdin
    alva tools list [--output-format json]  List registered tools (no API key needed)
    alva providers list [--output-format json]  List configured provider profiles
                                  (name/model/active only — endpoints and keys
                                  never enter the machine channel)
    alva jobs <submit|wait|status|result|list>  Async worker jobs: submit returns a
                                  job id immediately; wait blocks until done.
    alva <plugins|context|settings> ...   Subcommands (each has its own --help)

FLAGS:
    -p, --print                   Non-interactive single-prompt mode (no REPL/TUI).
                                  Tools that need approval are denied (fail-closed)
                                  unless --permission-mode allows them.
    --resume <SESSION_ID>         -p: continue a prior session (id from a
                                  previous --output-format json run).
    --system-prompt <TEXT>        Replace the default persona (project context
                                  is still appended). For orchestrated workers.
    --provider <NAME>             Use a named provider profile from
                                  ~/.alva/config.json for this invocation
                                  (see `alva providers list`); overrides the
                                  active profile. ALVA_* env vars still override
                                  individual fields on top.
    --allowed-tools <a,b,c>       -p: restrict this invocation to the listed
                                  tools (discover names via `alva tools list`).
    --max-turns <N>               Turn budget for this run (default 20). Lets an
                                  orchestrator bound a cheap-model worker.
    --dangerously-allow-unsandboxed  -p: allow accept-shell/bypass on platforms
                                  with NO OS sandbox. Only inside an isolated
                                  environment (container/VM/CI).
    --output-format <text|json>   -p output: `text` streams the reply (default);
                                  `json` emits one object {result, is_error, error,
                                  session_id, usage, duration_ms} for orchestrators.
    --permission-mode <MODE>      How tool permissions are handled. MODE is one of:
                                    ask           Prompt for each write/execute tool (default).
                                                  In -p mode this means: deny + tell you to
                                                  re-run with a looser mode.
                                    accept-edits  Auto-approve file writes; shell still gated.
                                    accept-shell  Auto-approve shell when the classifier deems it
                                                  safe/unknown; destructive commands blocked.
                                                  Best for headless/sandboxed runs.
                                    plan          Read-only: no file writes or commands.
                                    bypass        Allow everything, no prompts ('dangerously
                                                  skip permissions'); assumes a sandbox.
    --repl                        Use the legacy line-mode REPL instead of the TUI.
    --ui-mode <inline|fullscreen> TUI viewport mode.
    -h, --help                    Show this help.

RECURSION:
    Agent nesting (the `agent` tool, or workers running `alva` again via shell)
    is bounded by `subagent_depth` in ~/.alva/config.json (default 3), tracked
    across processes via ALVA_AGENT_DEPTH. At the limit, runs refuse to start.

EXAMPLES:
    alva -p \"summarize README.md\"
    alva -p --permission-mode accept-shell \"run the test suite and report failures\"
    alva -p --permission-mode bypass \"format the repo\"        # CI / sandbox only
"
    .to_string()
}

/// Decide the cross-process recursion gate. `env_depth` is the raw
/// `ALVA_AGENT_DEPTH` value (`None`/empty = top-level run, depth 0).
/// Returns the depth to export for child processes, or the refusal message.
/// Garbage input fails loudly — silently reading it as 0 would disarm the
/// gate exactly when something upstream is broken.
fn recursion_gate(env_depth: Option<&str>, limit: u32) -> Result<u32, String> {
    let depth: u32 = match env_depth {
        None | Some("") => 0,
        Some(raw) => raw.trim().parse().map_err(|_| {
            format!(
                "ALVA_AGENT_DEPTH is set to {raw:?}, which is not a number.\n\
                 Unset it for a top-level run, or fix whatever set it."
            )
        })?,
    };
    if depth >= limit {
        return Err(format!(
            "agent nesting depth {depth} reached the limit of {limit} (ALVA_AGENT_DEPTH).\n\
             Refusing to start another agent layer — this looks like runaway recursion.\n\
             If deeper nesting is intentional, raise `subagent_depth` in ~/.alva/config.json."
        ));
    }
    Ok(depth + 1)
}

#[cfg(test)]
mod recursion_gate_tests {
    use super::recursion_gate;

    #[test]
    fn top_level_run_exports_depth_one() {
        assert_eq!(recursion_gate(None, 3), Ok(1));
        assert_eq!(recursion_gate(Some(""), 3), Ok(1), "empty = unset");
    }

    #[test]
    fn below_limit_increments() {
        assert_eq!(recursion_gate(Some("2"), 3), Ok(3));
    }

    #[test]
    fn at_and_above_limit_refuse_naming_the_config_knob() {
        for depth in ["3", "7"] {
            let err = recursion_gate(Some(depth), 3).unwrap_err();
            assert!(err.contains("ALVA_AGENT_DEPTH"), "{err}");
            assert!(err.contains("subagent_depth"), "{err}");
        }
    }

    #[test]
    fn garbage_is_a_loud_error_not_depth_zero() {
        let err = recursion_gate(Some("banana"), 3).unwrap_err();
        assert!(err.contains("ALVA_AGENT_DEPTH"), "{err}");
        // -1 must not wrap into a huge in-range u32 either.
        assert!(recursion_gate(Some("-1"), 3).is_err());
    }
}

/// Overlay `ALVA_API_KEY` / `ALVA_MODEL` / `ALVA_BASE_URL` /
/// `ALVA_PROVIDER_KIND` onto an already-resolved provider config. Env vars
/// are the finest-grained override layer: they beat both the active profile
/// and an explicit `--provider` pick, field by field.
fn apply_env_overrides(config: &mut ProviderConfig) {
    if let Ok(k) = std::env::var("ALVA_API_KEY") {
        if !k.is_empty() {
            config.api_key = k;
        }
    }
    if let Ok(m) = std::env::var("ALVA_MODEL") {
        if !m.is_empty() {
            config.model = m;
        }
    }
    if let Ok(b) = std::env::var("ALVA_BASE_URL") {
        if !b.is_empty() {
            config.base_url = b;
        }
    }
    if let Ok(k) = std::env::var("ALVA_PROVIDER_KIND") {
        if !k.is_empty() {
            config.kind = Some(k);
        }
    }
}

/// Parse `--permission-mode <value>` from argv.
///
/// - `Ok(None)` — flag absent; caller keeps the agent's default mode.
/// - `Ok(Some(mode))` — recognized value.
/// - `Err(msg)` — flag present but value missing or unrecognized.
///
/// Mirrors the headless permission controls in Claude Code / Codex so a
/// non-interactive `-p` run can pick its policy instead of hanging on a
/// prompt nobody can answer.
fn parse_permission_mode(args: &[String]) -> Result<Option<alva_app_core::PermissionMode>, String> {
    use alva_app_core::PermissionMode;
    let Some(pos) = args.iter().position(|a| a == "--permission-mode") else {
        return Ok(None);
    };
    let Some(val) = args.get(pos + 1) else {
        return Err(
            "--permission-mode requires a value (ask|accept-edits|accept-shell|plan|bypass)".into(),
        );
    };
    let mode = match val.as_str() {
        "ask" => PermissionMode::Ask,
        "accept-edits" => PermissionMode::AcceptEdits,
        "accept-shell" => PermissionMode::AcceptShell,
        "plan" => PermissionMode::Plan,
        "bypass" => PermissionMode::Bypass,
        other => {
            return Err(format!(
            "unknown --permission-mode '{}' (expected ask|accept-edits|accept-shell|plan|bypass)",
            other
        ))
        }
    };
    Ok(Some(mode))
}

/// Human-facing label for a permission mode (matches the `--permission-mode`
/// flag spelling).
fn permission_mode_label(mode: alva_app_core::PermissionMode) -> &'static str {
    use alva_app_core::PermissionMode;
    match mode {
        PermissionMode::Ask => "ask",
        PermissionMode::AcceptEdits => "accept-edits",
        PermissionMode::AcceptShell => "accept-shell",
        PermissionMode::Plan => "plan",
        PermissionMode::Bypass => "bypass",
    }
}

/// Interactive y/N confirmation before enabling a sandbox-assuming permission
/// mode on a platform with no enforced sandbox (CLI#7). Returns `true` only on
/// an explicit affirmative. Declines automatically when stdin is not a TTY (no
/// one can answer), which keeps the unsafe mode from slipping through a pipe.
fn confirm_unsandboxed_mode(label: &str) -> bool {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!(
        "warning: --permission-mode {label} auto-runs commands and assumes an OS sandbox, \
         but none is enforced on this platform.\nCommands will run UNSANDBOXED. Continue? [y/N] "
    );
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes" | "YES")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_app_core::PermissionMode;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn usage_lists_print_and_permission_mode() {
        // A flag that exists but isn't documented is a discoverability bug.
        // Pin that `--help` advertises -p and the permission controls.
        let u = usage_text();
        assert!(u.contains("-p"), "mentions print mode: {u}");
        assert!(u.contains("--permission-mode"), "mentions the flag: {u}");
        assert!(u.contains("accept-shell"), "lists the modes: {u}");
        assert!(u.contains("bypass"), "lists bypass: {u}");
    }

    #[test]
    fn permission_mode_absent_returns_none() {
        assert_eq!(
            parse_permission_mode(&argv(&["alva", "-p", "hello"])),
            Ok(None)
        );
    }

    #[test]
    fn permission_mode_each_known_value_parses() {
        let cases = [
            ("ask", PermissionMode::Ask),
            ("accept-edits", PermissionMode::AcceptEdits),
            ("accept-shell", PermissionMode::AcceptShell),
            ("plan", PermissionMode::Plan),
            ("bypass", PermissionMode::Bypass),
        ];
        for (s, expected) in cases {
            assert_eq!(
                parse_permission_mode(&argv(&["alva", "--permission-mode", s])),
                Ok(Some(expected)),
                "value {s:?} should parse"
            );
        }
    }

    #[test]
    fn permission_mode_unknown_value_is_error() {
        assert!(parse_permission_mode(&argv(&["alva", "--permission-mode", "yolo"])).is_err());
    }

    #[test]
    fn permission_mode_missing_value_is_error() {
        assert!(parse_permission_mode(&argv(&["alva", "--permission-mode"])).is_err());
    }
}
