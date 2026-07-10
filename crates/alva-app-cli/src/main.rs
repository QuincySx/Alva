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
mod output;
mod plugins;
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
        Some("context") => return context::run(&argv[2..]).await,
        Some("settings") => return settings_cmd::run(&argv[2..]).await,
        _ => {}
    }

    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let paths = AlvaPaths::new(&workspace);

    // 1. Config — resolution order:
    //    a. env vars (ALVA_*) — `ProviderConfig::load` will pick these up
    //    b. project / global flat-format files (legacy CLI paths)
    //    c. shared `~/.alva/config.json` active provider — same file Tauri reads
    //    d. setup wizard
    //
    // The shared config is checked AFTER env vars + legacy files so old setups
    // keep working unchanged. New users go through the wizard or `alva settings set`.
    let config = match ProviderConfig::load(&workspace) {
        Ok(c) if !c.api_key.is_empty() => c,
        _ => {
            match settings_cmd::try_load_provider_from_shared() {
                Some(mut shared) => {
                    // Env vars still override individual fields when present.
                    if let Ok(k) = std::env::var("ALVA_API_KEY") {
                        if !k.is_empty() {
                            shared.api_key = k;
                        }
                    }
                    if let Ok(m) = std::env::var("ALVA_MODEL") {
                        if !m.is_empty() {
                            shared.model = m;
                        }
                    }
                    if let Ok(b) = std::env::var("ALVA_BASE_URL") {
                        if !b.is_empty() {
                            shared.base_url = b;
                        }
                    }
                    if let Ok(k) = std::env::var("ALVA_PROVIDER_KIND") {
                        if !k.is_empty() {
                            shared.kind = Some(k);
                        }
                    }
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
    } = agent_setup::build_agent(&config, &workspace, &paths).await;

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
                if print_mode {
                    output::print_error(&format!(
                        "--permission-mode {} assumes an OS sandbox, but none is enforced on \
                         this platform.\nRefusing to auto-run commands unsandboxed in headless \
                         (-p) mode. Use `--permission-mode ask` (or run on a platform with \
                         sandbox support).",
                        permission_mode_label(mode),
                    ));
                    return 1;
                }
                if !confirm_unsandboxed_mode(permission_mode_label(mode)) {
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
        let mut i = 1; // skip binary name
        while i < args.len() {
            let a = &args[i];
            if a == "-p" || a == "--print" {
                i += 1;
                continue;
            }
            if a == "--permission-mode" {
                i += 2; // skip the flag and its value
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

        // Persist the run like every other mode: a real session on disk is
        // what makes the returned session_id a usable --resume handle, and
        // gives external tooling the same RunRecord projection.
        let print_session = session_manager.create(&prompt).await;
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
    alva <plugins|context|settings> ...   Subcommands (each has its own --help)

FLAGS:
    -p, --print                   Non-interactive single-prompt mode (no REPL/TUI).
                                  Tools that need approval are denied (fail-closed)
                                  unless --permission-mode allows them.
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

EXAMPLES:
    alva -p \"summarize README.md\"
    alva -p --permission-mode accept-shell \"run the test suite and report failures\"
    alva -p --permission-mode bypass \"format the repo\"        # CI / sandbox only
"
    .to_string()
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
