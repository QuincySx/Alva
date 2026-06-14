// INPUT:  alva_app_core::AlvaPaths, alva_app_extension_loader (start_plugin, discover_plugins, proxy), serde_json
// OUTPUT: run(args): dispatcher for `alva plugins ...` subcommand
// POS:    CLI-only plugin debugging — list discovered plugins, or start one and fire an event at it, bypassing the agent loop for fast iteration.

//! `alva plugins` — CLI plugin debugging.
//!
//! # Subcommands
//!
//! - `alva plugins list` — show all discovered plugins (from project +
//!   global extensions dirs) without starting any subprocess.
//! - `alva plugins exec <dir> <event> [--data JSON]` — start one plugin
//!   in isolation, fire `event` at it, print whatever the plugin calls
//!   back into the stub host, then shut it down.
//!
//! # Why exec, not `alva <prompt>` with a debugger attached
//!
//! Plugin development iteration was: edit .py → run full agent →
//! trigger a prompt that exercises the plugin → watch output → edit.
//! Minutes per cycle. With `exec`, it's seconds: no LLM call, no
//! session state, no UI — just `plugin → event → host RPC trace`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alva_app_core::AlvaPaths;
use alva_app_extension_loader::loader::{discover_plugins, start_plugin};
use alva_app_extension_loader::proxy::{AepEvent, RemoteExtensionProxy};
use alva_kernel_abi::tool::execution::ToolOutput;
use serde_json::Value;

/// Top-level dispatcher. `args` is everything after `plugins` (so for
/// `alva plugins exec foo bar` it's `["exec", "foo", "bar"]`).
pub async fn run(args: &[String]) -> i32 {
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let paths = AlvaPaths::new(&workspace);

    match args.first().map(|s| s.as_str()) {
        Some("list") => run_list(&paths).await,
        Some("exec") => run_exec(&args[1..]).await,
        Some("--help") | Some("-h") | Some("help") | None => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("alva plugins: unknown subcommand `{other}`");
            print_help();
            2
        }
    }
}

fn print_help() {
    eprintln!("alva plugins — debug AEP plugins");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  alva plugins list");
    eprintln!("  alva plugins exec <plugin_dir> <event> [--data JSON] [--timeout SECS]");
    eprintln!();
    eprintln!("EVENTS:");
    eprintln!("  agent_start");
    eprintln!("  agent_end          --data '{{\"error\": \"...\"}}'");
    eprintln!("  before_tool_call   --data '{{\"tool_name\":\"...\",\"tool_call_id\":\"...\",\"arguments\":{{...}}}}'");
    eprintln!("  after_tool_call    --data '{{\"tool_name\":\"...\",\"tool_call_id\":\"...\",\"result\":{{...}}}}'");
    eprintln!("  input              --data '{{\"text\": \"...\"}}'");
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

async fn run_list(paths: &AlvaPaths) -> i32 {
    let dirs = vec![paths.project_extensions_dir(), paths.global_extensions_dir()];
    let plugins = discover_plugins(&dirs).await;

    if plugins.is_empty() {
        println!("No plugins found.");
        println!("  project: {}", paths.project_extensions_dir().display());
        println!("  global:  {}", paths.global_extensions_dir().display());
        return 0;
    }

    for p in &plugins {
        let rel = p
            .dir
            .strip_prefix(std::env::current_dir().ok().unwrap_or_default())
            .unwrap_or(&p.dir);
        println!("• {} ({})", p.manifest.name, rel.display());
        println!("    version:     {}", p.manifest.version);
        println!("    runtime:     {:?}", p.manifest.runtime);
        println!("    entry:       {}", p.manifest.entry);
        if let Some(desc) = &p.manifest.description {
            println!("    description: {}", desc);
        }
        if !p.manifest.requested_capabilities.is_empty() {
            println!(
                "    capabilities: {}",
                p.manifest.requested_capabilities.join(", ")
            );
        }
        println!();
    }
    0
}

// ---------------------------------------------------------------------------
// exec
// ---------------------------------------------------------------------------

async fn run_exec(args: &[String]) -> i32 {
    // Positional: <plugin_dir> <event>. Options: --data JSON, --timeout SECS.
    let mut plugin_dir: Option<PathBuf> = None;
    let mut event_name: Option<String> = None;
    let mut data_json = String::from("{}");
    let mut timeout_secs: u64 = 2;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => {
                let Some(v) = it.next() else {
                    eprintln!("alva plugins exec: --data requires a value");
                    return 2;
                };
                data_json = v.clone();
            }
            "--timeout" => {
                let Some(v) = it.next() else {
                    eprintln!("alva plugins exec: --timeout requires a value");
                    return 2;
                };
                match v.parse::<u64>() {
                    Ok(n) => timeout_secs = n,
                    Err(_) => {
                        eprintln!("alva plugins exec: --timeout must be an integer");
                        return 2;
                    }
                }
            }
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            other if plugin_dir.is_none() => plugin_dir = Some(PathBuf::from(other)),
            other if event_name.is_none() => event_name = Some(other.to_string()),
            other => {
                eprintln!("alva plugins exec: unexpected positional arg `{other}`");
                return 2;
            }
        }
    }

    let (Some(plugin_dir), Some(event_name)) = (plugin_dir, event_name) else {
        eprintln!("alva plugins exec: missing arguments");
        print_help();
        return 2;
    };

    let data: Value = match serde_json::from_str(&data_json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("alva plugins exec: --data must be valid JSON ({e})");
            return 2;
        }
    };

    let event = match build_event(&event_name, &data) {
        Ok(ev) => ev,
        Err(e) => {
            eprintln!("alva plugins exec: {e}");
            return 2;
        }
    };

    // Start the plugin.
    let proxy: Arc<RemoteExtensionProxy> = match start_plugin(plugin_dir.clone()).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("alva plugins exec: failed to start plugin: {e}");
            return 1;
        }
    };
    println!(
        "[plugins] loaded `{}` ({:?})",
        proxy.name(),
        proxy.manifest().runtime
    );
    println!(
        "[plugins] subscribes to: {:?}",
        proxy.init_result().event_subscriptions
    );

    // Dispatch. `dispatch_event_sync` uses block_in_place internally — we
    // spawn it onto a blocking task to avoid blocking the current tokio
    // worker thread. `AepEvent` borrows its payload, so we move the owned
    // event into the closure and borrow it there (keeping the closure
    // `'static`).
    let proxy_for_dispatch = proxy.clone();
    let owned_event = event;
    let dispatch = tokio::task::spawn_blocking(move || {
        proxy_for_dispatch.dispatch_event_sync(&owned_event.as_event())
    });

    // Wait for dispatch to return OR timeout — whichever first.
    let result = match tokio::time::timeout(Duration::from_secs(timeout_secs), dispatch).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            eprintln!("[plugins] dispatch task panicked: {e}");
            let _ = try_shutdown(proxy).await;
            return 1;
        }
        Err(_) => {
            eprintln!("[plugins] dispatch timed out after {timeout_secs}s");
            let _ = try_shutdown(proxy).await;
            return 1;
        }
    };

    println!("[plugins] event `{event_name}` handled: {result:?}");

    // Shutdown.
    let _ = try_shutdown(proxy).await;
    0
}

async fn try_shutdown(proxy: Arc<RemoteExtensionProxy>) -> i32 {
    // Arc::try_unwrap so we can consume into shutdown(self).
    match Arc::try_unwrap(proxy) {
        Ok(p) => {
            if let Err(e) = p.shutdown().await {
                eprintln!("[plugins] shutdown error: {e}");
                return 1;
            }
            0
        }
        Err(_) => {
            // Someone else holds an Arc — can't take ownership. Shouldn't
            // happen in exec flow (we only cloned once for the dispatch
            // task which already returned). Log and move on.
            eprintln!("[plugins] shutdown skipped: proxy still referenced");
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Event construction from JSON data
// ---------------------------------------------------------------------------

/// Owned mirror of the loader's borrowing [`AepEvent`].
///
/// `AepEvent` borrows its payload so the hot middleware path avoids
/// cloning. The CLI builds the event from a parsed `--data` blob and
/// then hands it to a `spawn_blocking` closure, so it needs an owned,
/// `'static` carrier. `as_event()` lends out a borrowing `AepEvent`.
#[derive(Debug)]
enum OwnedAepEvent {
    AgentStart,
    AgentEnd { error: Option<String> },
    BeforeToolCall {
        tool_name: String,
        tool_call_id: String,
        arguments: Value,
    },
    AfterToolCall {
        tool_name: String,
        tool_call_id: String,
        result: ToolOutput,
    },
    UserMessage { text: String },
}

impl OwnedAepEvent {
    fn as_event(&self) -> AepEvent<'_> {
        match self {
            OwnedAepEvent::AgentStart => AepEvent::AgentStart,
            OwnedAepEvent::AgentEnd { error } => AepEvent::AgentEnd {
                error: error.as_deref(),
            },
            OwnedAepEvent::BeforeToolCall {
                tool_name,
                tool_call_id,
                arguments,
            } => AepEvent::BeforeToolCall {
                tool_name,
                tool_call_id,
                arguments,
            },
            OwnedAepEvent::AfterToolCall {
                tool_name,
                tool_call_id,
                result,
            } => AepEvent::AfterToolCall {
                tool_name,
                tool_call_id,
                result,
            },
            OwnedAepEvent::UserMessage { text } => AepEvent::UserMessage { text },
        }
    }
}

fn build_event(name: &str, data: &Value) -> Result<OwnedAepEvent, String> {
    let name = name.replace('.', "_");
    match name.as_str() {
        "agent_start" => Ok(OwnedAepEvent::AgentStart),
        "agent_end" => {
            let error = data.get("error").and_then(Value::as_str).map(String::from);
            Ok(OwnedAepEvent::AgentEnd { error })
        }
        "before_tool_call" => {
            let tool_name = data
                .get("tool_name")
                .and_then(Value::as_str)
                .ok_or_else(|| "data.tool_name missing".to_string())?
                .to_string();
            let tool_call_id = data
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("stub-id")
                .to_string();
            let arguments = data
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));
            Ok(OwnedAepEvent::BeforeToolCall {
                tool_name,
                tool_call_id,
                arguments,
            })
        }
        "after_tool_call" => {
            let tool_name = data
                .get("tool_name")
                .and_then(Value::as_str)
                .ok_or_else(|| "data.tool_name missing".to_string())?
                .to_string();
            let tool_call_id = data
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("stub-id")
                .to_string();
            // Minimal stub result — plugin authors mostly care about
            // tool_name; those that inspect `result` can pass a richer
            // JSON via --data.result and we'd need a Value → ToolOutput
            // deserialization. ToolOutput::text works for text-only
            // cases which is the 90% use case.
            let result_text = data
                .get("result")
                .and_then(|v| v.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let result = if result_text.is_empty() {
                ToolOutput::text("")
            } else {
                ToolOutput::text(result_text)
            };
            Ok(OwnedAepEvent::AfterToolCall {
                tool_name,
                tool_call_id,
                result,
            })
        }
        "input" => {
            let text = data
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "data.text missing".to_string())?
                .to_string();
            Ok(OwnedAepEvent::UserMessage { text })
        }
        other => Err(format!(
            "unknown event `{other}`; see `alva plugins --help` for the list"
        )),
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `build_event` — parses a plugin event name and
    //! optional `--data` JSON into an [`OwnedAepEvent`]. Used by
    //! `alva plugins exec <dir> <event> --data JSON`, the fast path
    //! for plugin authors to test their handler without running a
    //! full agent loop. Errors here cost developer time, so the
    //! field-handling contract is worth pinning down.
    use super::*;
    use serde_json::json;

    #[test]
    fn build_event_agent_start_takes_no_data() {
        let ev = build_event("agent_start", &json!({})).unwrap();
        assert!(matches!(ev, OwnedAepEvent::AgentStart));
    }

    #[test]
    fn build_event_dot_in_name_normalized_to_underscore() {
        // The CLI accepts "agent.start" as an alias for "agent_start";
        // build_event replaces dots with underscores before matching.
        let ev = build_event("agent.start", &json!({})).unwrap();
        assert!(matches!(ev, OwnedAepEvent::AgentStart));
        // Multi-dot too
        let ev2 = build_event("before.tool.call", &json!({"tool_name": "t"})).unwrap();
        assert!(matches!(ev2, OwnedAepEvent::BeforeToolCall { .. }));
    }

    #[test]
    fn build_event_agent_end_with_error() {
        let ev = build_event("agent_end", &json!({"error": "boom"})).unwrap();
        match ev {
            OwnedAepEvent::AgentEnd { error } => assert_eq!(error.as_deref(), Some("boom")),
            other => panic!("expected AgentEnd, got {:?}", other),
        }
    }

    #[test]
    fn build_event_agent_end_without_error_is_none() {
        let ev = build_event("agent_end", &json!({})).unwrap();
        match ev {
            OwnedAepEvent::AgentEnd { error } => assert!(error.is_none()),
            other => panic!("expected AgentEnd, got {:?}", other),
        }
    }

    #[test]
    fn build_event_before_tool_call_full() {
        let ev = build_event(
            "before_tool_call",
            &json!({"tool_name": "read_file", "tool_call_id": "tc-1", "arguments": {"path": "/x"}}),
        )
        .unwrap();
        match ev {
            OwnedAepEvent::BeforeToolCall { tool_name, tool_call_id, arguments } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_call_id, "tc-1");
                assert_eq!(arguments["path"], "/x");
            }
            other => panic!("expected BeforeToolCall, got {:?}", other),
        }
    }

    #[test]
    fn build_event_before_tool_call_missing_tool_name_is_err() {
        let err = build_event("before_tool_call", &json!({})).unwrap_err();
        assert!(err.contains("tool_name"), "err message should mention field: {err}");
    }

    #[test]
    fn build_event_before_tool_call_defaults_when_optional_fields_missing() {
        // tool_call_id defaults to "stub-id", arguments defaults to {}.
        let ev = build_event("before_tool_call", &json!({"tool_name": "t"})).unwrap();
        match ev {
            OwnedAepEvent::BeforeToolCall { tool_call_id, arguments, .. } => {
                assert_eq!(tool_call_id, "stub-id");
                assert!(arguments.is_object() && arguments.as_object().unwrap().is_empty());
            }
            other => panic!("expected BeforeToolCall, got {:?}", other),
        }
    }

    #[test]
    fn build_event_after_tool_call_with_result_text() {
        let ev = build_event(
            "after_tool_call",
            &json!({"tool_name": "edit", "tool_call_id": "tc-2", "result": {"text": "done"}}),
        )
        .unwrap();
        match ev {
            OwnedAepEvent::AfterToolCall { tool_name, tool_call_id, result } => {
                assert_eq!(tool_name, "edit");
                assert_eq!(tool_call_id, "tc-2");
                // ToolOutput::text("done") → first content block is Text("done")
                assert_eq!(result.content[0].as_text(), Some("done"));
            }
            other => panic!("expected AfterToolCall, got {:?}", other),
        }
    }

    #[test]
    fn build_event_after_tool_call_missing_tool_name_is_err() {
        let err = build_event("after_tool_call", &json!({})).unwrap_err();
        assert!(err.contains("tool_name"));
    }

    #[test]
    fn build_event_after_tool_call_missing_result_text_gives_empty() {
        let ev = build_event("after_tool_call", &json!({"tool_name": "t"})).unwrap();
        match ev {
            OwnedAepEvent::AfterToolCall { result, .. } => {
                // No result.text → empty string content
                assert_eq!(result.content[0].as_text(), Some(""));
            }
            other => panic!("expected AfterToolCall, got {:?}", other),
        }
    }

    #[test]
    fn build_event_input_with_text() {
        let ev = build_event("input", &json!({"text": "hello"})).unwrap();
        match ev {
            OwnedAepEvent::UserMessage { text } => assert_eq!(text, "hello"),
            other => panic!("expected UserMessage, got {:?}", other),
        }
    }

    #[test]
    fn build_event_input_missing_text_is_err() {
        let err = build_event("input", &json!({})).unwrap_err();
        assert!(err.contains("text"), "err should mention field: {err}");
    }

    #[test]
    fn build_event_unknown_event_returns_err_with_help_hint() {
        let err = build_event("definitely_not_an_event", &json!({})).unwrap_err();
        assert!(err.contains("unknown event"));
        assert!(err.contains("definitely_not_an_event"));
        assert!(err.contains("--help"), "err should point user to --help: {err}");
    }
}

