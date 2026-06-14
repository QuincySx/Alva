//! End-to-end loader smoke test.
//!
//! Writes a tiny Python plugin to a tempdir, registers
//! `SubprocessLoaderExtension` with a real `ExtensionHost`, and drives
//! `before_tool_call` through the registered `AepBridgeMiddleware`'s
//! hook — the same path the agent's middleware stack uses. The plugin
//! blocks shell calls containing `"rm -rf"` and lets everything else
//! through.
//!
//! Requires `python3` on PATH; skips cleanly otherwise.

#![cfg(not(target_family = "wasm"))]

use std::process::Command as StdCommand;
use std::sync::{Arc, RwLock};

use alva_agent_core::extension::{Extension, ExtensionHost, HostAPI};
use alva_app_extension_loader::loader::SubprocessLoaderExtension;
use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{AgentMessage, Message, ToolCall};
use alva_kernel_core::{AgentState, Extensions, Middleware, MiddlewareError};
use alva_test::mock_provider::MockLanguageModel;

/// Build a minimal `AgentState` sufficient to call middleware hooks.
/// The AEP bridge only reads the session (for `on_user_message`); the
/// model and tools are inert placeholders.
fn make_state(session: Arc<InMemoryAgentSession>) -> AgentState {
    AgentState {
        model: Arc::new(MockLanguageModel::new()),
        tools: vec![],
        session,
        extensions: Extensions::new(),
    }
}

/// Pull the registered `AepBridgeMiddleware` out of the host so the
/// test can call its hooks directly.
fn take_bridge(host: &Arc<RwLock<ExtensionHost>>) -> Arc<dyn Middleware> {
    let mws = host.write().unwrap().take_middlewares();
    mws.into_iter()
        .find(|m| m.name() == "aep-bridge")
        .expect("loader must register an aep-bridge middleware")
}

const PLUGIN_MANIFEST: &str = r#"
name = "shell-guard"
version = "0.0.1"
description = "Test plugin that blocks rm -rf"
runtime = "python"
entry = "main.py"
"#;

const PLUGIN_PY: &str = r#"import json
import sys


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def main():
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue

        method = req.get("method")
        req_id = req.get("id")

        if method == "initialize":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "protocolVersion": "0.1.0",
                        "plugin": {
                            "name": "shell-guard",
                            "version": "0.0.1",
                        },
                        "tools": [],
                        "eventSubscriptions": ["before_tool_call"],
                        "requestedCapabilities": [],
                    },
                }
            )
        elif method == "initialized":
            # notification; no reply
            pass
        elif method == "extension/before_tool_call":
            params = req.get("params", {})
            tool = params.get("toolCall", {})
            if tool.get("name") == "shell":
                args = tool.get("arguments", {})
                cmd = args.get("command", "")
                if "rm -rf" in cmd:
                    send(
                        {
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {
                                "action": "block",
                                "reason": "rm -rf is forbidden",
                            },
                        }
                    )
                    continue
            send(
                {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {"action": "continue"},
                }
            )
        elif method == "shutdown":
            send({"jsonrpc": "2.0", "id": req_id, "result": {}})
            return
        else:
            if req_id is not None:
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {"code": -32601, "message": f"unknown: {method}"},
                    }
                )


if __name__ == "__main__":
    main()
"#;

fn python_available() -> bool {
    StdCommand::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_blocks_dangerous_shell_command() {
    if !python_available() {
        eprintln!("skipping plugin_blocks_dangerous_shell_command: python3 not on PATH");
        return;
    }

    // Build a throwaway extensions directory with exactly one plugin.
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = temp.path().join("shell-guard");
    std::fs::create_dir(&plugin_dir).expect("create plugin dir");
    std::fs::write(plugin_dir.join("alva.toml"), PLUGIN_MANIFEST).expect("write manifest");
    std::fs::write(plugin_dir.join("main.py"), PLUGIN_PY).expect("write python entry");

    // Wire up the host + loader like the real agent would.
    let host = Arc::new(RwLock::new(ExtensionHost::new()));
    let api = HostAPI::new(Arc::clone(&host), "subprocess-loader".to_string());
    let ext = SubprocessLoaderExtension::new(vec![temp.path().to_path_buf()]);

    // Phase 1 of lifecycle: sync activate registers handlers.
    ext.activate(&api);

    // Phase 2 of lifecycle: async configure would normally happen,
    // but we do not have a real `ExtensionContext` in tests — we
    // trigger the same plugin-loading code path via the public
    // `load_plugins` helper.
    let count = ext.load_plugins().await.expect("load plugins");
    assert_eq!(count, 1, "expected exactly one plugin loaded");
    assert_eq!(ext.loaded_count(), 1);

    // Grab the middleware the loader registered, and a state to drive it.
    let bridge = take_bridge(&host);
    let session = Arc::new(InMemoryAgentSession::new());
    // Seed a user message so on_user_message has something to read
    // (the shell-guard plugin does not subscribe, but this exercises
    // the latest-user-message path inside on_agent_start).
    session
        .append_message(AgentMessage::Standard(Message::user("hello")), None)
        .await;
    let mut state = make_state(session);

    // --- on_agent_start: unsubscribed plugin → no error ---
    bridge
        .on_agent_start(&mut state)
        .await
        .expect("on_agent_start must not fail for unsubscribed plugin");

    // --- dangerous command: expect Block ---
    let blocked = ToolCall {
        id: "call-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "rm -rf /"}),
    };
    match bridge.before_tool_call(&mut state, &blocked).await {
        Err(MiddlewareError::Blocked { reason }) => {
            assert!(
                reason.contains("rm -rf") || reason.contains("forbidden"),
                "unexpected block reason: {reason}"
            );
        }
        other => panic!("expected Blocked for rm -rf, got {:?}", other),
    }

    // --- safe command: expect Ok (Continue) ---
    let safe = ToolCall {
        id: "call-2".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "ls -la"}),
    };
    bridge
        .before_tool_call(&mut state, &safe)
        .await
        .expect("expected Ok (continue) for ls");

    // Orderly teardown — kills the subprocess.
    ext.shutdown_all().await.expect("shutdown_all");
    drop(temp);
}
