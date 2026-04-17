//! End-to-end loader smoke test.
//!
//! Writes a tiny Python plugin to a tempdir, registers
//! `SubprocessLoaderExtension` with a real `ExtensionHost`, and
//! drives `before_tool_call` events through the host's dispatch
//! machinery. The plugin blocks shell calls containing `"rm -rf"`
//! and lets everything else through.
//!
//! Requires `python3` on PATH; skips cleanly otherwise.

#![cfg(not(target_family = "wasm"))]

use std::process::Command as StdCommand;
use std::sync::{Arc, RwLock};

use alva_agent_core::extension::{
    Extension, EventResult, ExtensionEvent, ExtensionHost, HostAPI,
};
use alva_app_extension_loader::loader::SubprocessLoaderExtension;

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

    // --- dangerous command: expect Block ---
    let blocked = ExtensionEvent::BeforeToolCall {
        tool_name: "shell".to_string(),
        tool_call_id: "call-1".to_string(),
        arguments: serde_json::json!({"command": "rm -rf /"}),
    };
    let result = host.read().unwrap().emit(&blocked);
    match result {
        EventResult::Block { reason } => {
            assert!(
                reason.contains("rm -rf") || reason.contains("forbidden"),
                "unexpected block reason: {reason}"
            );
        }
        other => panic!("expected Block for rm -rf, got {:?}", other),
    }

    // --- safe command: expect Continue ---
    let safe = ExtensionEvent::BeforeToolCall {
        tool_name: "shell".to_string(),
        tool_call_id: "call-2".to_string(),
        arguments: serde_json::json!({"command": "ls -la"}),
    };
    let result = host.read().unwrap().emit(&safe);
    assert!(
        matches!(result, EventResult::Continue),
        "expected Continue for ls, got {:?}",
        result
    );

    // --- event the plugin does not subscribe to: expect Continue ---
    // The plugin only subscribed to `before_tool_call`; `agent_start`
    // should pass through without an RPC round-trip.
    let start = ExtensionEvent::AgentStart;
    let result = host.read().unwrap().emit(&start);
    assert!(
        matches!(result, EventResult::Continue),
        "expected Continue for unsubscribed event, got {:?}",
        result
    );

    // Orderly teardown — kills the subprocess.
    ext.shutdown_all().await.expect("shutdown_all");
    drop(temp);
}
