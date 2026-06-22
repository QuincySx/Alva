//! Phase 3 demo — two Python plugins loaded dynamically from a
//! tempdir, driven through a real `PluginHost`.
//!
//! Run with:
//!
//! ```bash
//! cargo run -p alva-app-extension-loader --example demo
//! ```
//!
//! Requires `python3` on PATH. No Python SDK, no `pip install` —
//! plugins are stdlib-only scripts that talk JSON-RPC on stdio.

#![cfg(not(target_family = "wasm"))]

use std::process::Command as StdCommand;
use std::sync::{Arc, RwLock};

use alva_agent_core::extension::{Plugin, PluginHost, Registrar};
use alva_app_extension_loader::loader::SubprocessLoaderPlugin;
use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{AgentMessage, Message, ToolCall};
use alva_kernel_core::{AgentState, Extensions, Middleware, MiddlewareError};
use alva_test::mock_provider::MockLanguageModel;

// ===========================================================
// Plugin 1 — shell-guard: blocks destructive shell commands
// ===========================================================

const SHELL_GUARD_MANIFEST: &str = r#"
name = "shell-guard"
version = "0.0.1"
description = "Blocks dangerous shell commands"
runtime = "python"
entry = "main.py"
"#;

const SHELL_GUARD_PY: &str = r#"import json, sys

def send(o):
    sys.stdout.write(json.dumps(o) + "\n")
    sys.stdout.flush()

def log(msg):
    sys.stderr.write(f"[shell-guard] {msg}\n")
    sys.stderr.flush()

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    req = json.loads(line)
    m, rid = req.get("method"), req.get("id")

    if m == "initialize":
        log("handshake: hello from shell-guard")
        send({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "protocolVersion": "0.1.0",
                "plugin": {"name": "shell-guard", "version": "0.0.1"},
                "tools": [],
                "eventSubscriptions": ["before_tool_call"],
                "requestedCapabilities": [],
            },
        })
    elif m == "initialized":
        log("ready to guard")
    elif m == "extension/before_tool_call":
        tc = req.get("params", {}).get("toolCall", {})
        tool = tc.get("name")
        cmd = tc.get("arguments", {}).get("command", "")
        log(f"inspecting tool={tool} cmd={cmd[:50]!r}")
        if tool == "shell" and "rm -rf" in cmd:
            log(f"BLOCKING: {cmd!r}")
            send({
                "jsonrpc": "2.0",
                "id": rid,
                "result": {
                    "action": "block",
                    "reason": f"rm -rf is forbidden: {cmd}",
                },
            })
        else:
            send({"jsonrpc": "2.0", "id": rid, "result": {"action": "continue"}})
    elif m == "shutdown":
        log("shutting down")
        send({"jsonrpc": "2.0", "id": rid, "result": {}})
        break
"#;

// ===========================================================
// Plugin 2 — tool-logger: observes every event, blocks nothing
// ===========================================================

const TOOL_LOGGER_MANIFEST: &str = r#"
name = "tool-logger"
version = "0.0.1"
description = "Prints every host event to stderr"
runtime = "python"
entry = "main.py"
"#;

const TOOL_LOGGER_PY: &str = r#"import json, sys

def send(o):
    sys.stdout.write(json.dumps(o) + "\n")
    sys.stdout.flush()

def log(msg):
    sys.stderr.write(f"[tool-logger] {msg}\n")
    sys.stderr.flush()

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    req = json.loads(line)
    m, rid = req.get("method"), req.get("id")

    if m == "initialize":
        log("handshake: hello from tool-logger")
        send({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "protocolVersion": "0.1.0",
                "plugin": {"name": "tool-logger", "version": "0.0.1"},
                "tools": [],
                "eventSubscriptions": [
                    "before_tool_call",
                    "after_tool_call",
                    "on_agent_start",
                    "on_agent_end",
                    "on_user_message",
                ],
                "requestedCapabilities": [],
            },
        })
    elif m == "initialized":
        log("observing all events")
    elif m and m.startswith("extension/"):
        evt = m[len("extension/"):]
        params = req.get("params", {})
        tc = params.get("toolCall", {})
        tool = tc.get("name", "-")
        log(f"saw event={evt} tool={tool}")
        send({"jsonrpc": "2.0", "id": rid, "result": {"action": "continue"}})
    elif m == "shutdown":
        log("shutting down")
        send({"jsonrpc": "2.0", "id": rid, "result": {}})
        break
"#;

// ===========================================================
// Helpers
// ===========================================================

fn python_available() -> bool {
    StdCommand::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn banner(label: &str) {
    println!("━━━ {} ━━━", label);
}

fn line(s: impl std::fmt::Display) {
    println!("  {}", s);
}

fn result_summary(r: &Result<(), MiddlewareError>) -> String {
    match r {
        Ok(()) => "Continue".to_string(),
        Err(MiddlewareError::Blocked { reason }) => format!("BLOCKED — {}", reason),
        Err(e) => format!("ERROR — {}", e),
    }
}

// ===========================================================
// main
// ===========================================================

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "alva_app_extension_loader=info,aep.plugin.stderr=warn",
                )
            }),
        )
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(true)
        .compact()
        .init();

    if !python_available() {
        eprintln!("python3 not on PATH; cannot run demo");
        std::process::exit(2);
    }

    println!();
    println!("┌─────────────────────────────────────────────────────────┐");
    println!("│  alva-app-extension-loader — Phase 3 demo               │");
    println!("│                                                         │");
    println!("│  Two Python plugins, loaded dynamically from a tempdir, │");
    println!("│  reacting to host events. No SDK, no pip, no Rust —     │");
    println!("│  just stdlib Python on stdin/stdout.                    │");
    println!("└─────────────────────────────────────────────────────────┘");
    println!();

    // ----- Install plugins on disk -----
    let temp = tempfile::tempdir().expect("tempdir");
    banner("install");
    for (name, manifest, py) in [
        ("shell-guard", SHELL_GUARD_MANIFEST, SHELL_GUARD_PY),
        ("tool-logger", TOOL_LOGGER_MANIFEST, TOOL_LOGGER_PY),
    ] {
        let dir = temp.path().join(name);
        std::fs::create_dir(&dir).expect("create plugin dir");
        std::fs::write(dir.join("alva.toml"), manifest).expect("write manifest");
        std::fs::write(dir.join("main.py"), py).expect("write python entry");
        line(format!("✓ {name} → {}/alva.toml + main.py", dir.display()));
    }
    println!();
    line(format!("extensions dir: {}", temp.path().display()));
    println!();

    // ----- Wire up host + loader -----
    banner("bootstrap");
    let host = Arc::new(RwLock::new(PluginHost::new()));
    let ext = SubprocessLoaderPlugin::new(vec![temp.path().to_path_buf()]);

    line("register → spawning subprocesses + AEP handshake + phase handlers");
    let bus = alva_kernel_abi::Bus::new();
    let bus_writer = bus.writer();
    let bus_handle = bus_writer.handle();
    let reg = Registrar::new(
        Arc::clone(&host),
        "subprocess-loader".to_string(),
        bus_handle,
        bus_writer,
        temp.path().to_path_buf(),
    );
    ext.register(&reg).await;
    line(format!("✓ {} plugins loaded", ext.loaded_count()));
    println!();

    // Pull out the compiled AEP phase handlers and drive their hooks like
    // the agent's middleware stack would.
    let adapters: Vec<Arc<dyn Middleware>> = {
        let mws = host.write().unwrap().take_middlewares();
        mws.into_iter()
            .filter(|m| m.name().starts_with("phase:aep:"))
            .collect()
    };
    assert!(
        !adapters.is_empty(),
        "loader must register executable AEP phase handlers"
    );

    // Minimal state.
    let session = Arc::new(InMemoryAgentSession::new());
    let user_message = AgentMessage::Standard(Message::user("list my files please"));
    session.append_message(user_message.clone(), None).await;
    let mut state = AgentState {
        model: Arc::new(MockLanguageModel::new()),
        tools: vec![],
        session,
        extensions: Extensions::new(),
    };

    // ----- Drive a sequence of hooks -----
    banner("hook: on_agent_start");
    let mut start_result = Ok(());
    for adapter in &adapters {
        start_result = adapter.on_agent_start(&mut state).await;
        if start_result.is_err() {
            break;
        }
    }
    line(format!("→ {}", result_summary(&start_result)));
    println!();

    banner("hook: input_committed (on_user_message)");
    let mut input_result = Ok(());
    for adapter in &adapters {
        input_result = adapter.input_committed(&mut state, &user_message).await;
        if input_result.is_err() {
            break;
        }
    }
    line(format!("→ {}", result_summary(&input_result)));
    println!();

    let tool_calls = vec![
        ("before_tool_call(shell: ls -la)", "c1", "ls -la"),
        ("before_tool_call(shell: echo hi)", "c2", "echo hi"),
        (
            "before_tool_call(shell: rm -rf /tmp/x)  ← should be blocked",
            "c3",
            "rm -rf /tmp/x",
        ),
    ];
    for (label, id, command) in tool_calls {
        let tc = ToolCall {
            id: id.to_string(),
            name: "shell".to_string(),
            arguments: serde_json::json!({ "command": command }),
        };
        banner(&format!("hook: {label}"));
        let mut result = Ok(());
        for adapter in &adapters {
            result = adapter.before_tool_call(&mut state, &tc).await;
            if result.is_err() {
                break;
            }
        }
        line(format!("→ {}", result_summary(&result)));
        println!();
    }

    banner("hook: on_agent_end");
    let mut end_result = Ok(());
    for adapter in &adapters {
        end_result = adapter.on_agent_end(&mut state, None).await;
        if end_result.is_err() {
            break;
        }
    }
    line(format!("→ {}", result_summary(&end_result)));
    println!();

    // ----- Teardown -----
    banner("shutdown");
    ext.shutdown_all().await.expect("shutdown_all");
    line("✓ all plugins stopped");
    drop(temp);
    println!();
    println!("demo complete.");
}
