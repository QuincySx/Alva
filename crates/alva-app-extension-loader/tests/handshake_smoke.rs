//! End-to-end handshake smoke test.
//!
//! Spawns a trivial Python echo plugin (stdlib-only, no SDK), drives
//! the dispatcher through `initialize` → `shutdown`, and verifies the
//! child exits cleanly with status 0. This is the only Phase 2 test
//! that actually crosses a process boundary.
//!
//! Requires `python3` on PATH. If not available, the test prints a
//! message and returns without failing — CI environments without
//! Python should stay green.

#![cfg(not(target_family = "wasm"))]

use std::io::Write as _;
use std::process::Command as StdCommand;
use std::sync::Arc;

use alva_app_extension_loader::dispatcher::{NoopHostHandler, RpcDispatcher};
use alva_app_extension_loader::manifest::Runtime;
use alva_app_extension_loader::protocol::{
    methods, HostCapabilities, HostInfo, InitializeParams, InitializeResult, PROTOCOL_VERSION,
};
use alva_app_extension_loader::subprocess::{LauncherOverride, SubprocessRuntime};

const PYTHON_ECHO_SOURCE: &str = r#"import json
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
                            "name": "echo",
                            "version": "0.0.1",
                        },
                        "tools": [],
                        "eventSubscriptions": [],
                        "requestedCapabilities": [],
                    },
                }
            )
        elif method == "shutdown":
            send({"jsonrpc": "2.0", "id": req_id, "result": {}})
            return
        elif method == "initialized":
            # Notification; no response.
            continue
        else:
            # Unknown method -> JSON-RPC MethodNotFound.
            if req_id is not None:
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {
                            "code": -32601,
                            "message": f"method not found: {method}",
                        },
                    }
                )


if __name__ == "__main__":
    main()
"#;

fn python_available() -> bool {
    StdCommand::new("python3")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread")]
async fn handshake_initialize_then_shutdown() {
    if !python_available() {
        eprintln!("skipping handshake_initialize_then_shutdown: python3 not on PATH");
        return;
    }

    // Materialise the echo script to a tempfile.
    let mut script = tempfile::Builder::new()
        .prefix("aep-echo-")
        .suffix(".py")
        .tempfile()
        .expect("create tempfile");
    script
        .write_all(PYTHON_ECHO_SOURCE.as_bytes())
        .expect("write python source");
    script.flush().expect("flush tempfile");
    let script_path = script.path().to_path_buf();

    // Spawn the subprocess. Use an explicit launcher so we do not
    // depend on the future SDK — the script is pure stdlib.
    let runtime = SubprocessRuntime::spawn(
        "echo-smoke",
        Runtime::Python,
        script_path,
        None,
        Some(LauncherOverride {
            program: "python3".to_string(),
            prepend_args: vec!["-u".to_string()],
            env: vec![],
        }),
    )
    .await
    .expect("spawn python echo subprocess");

    let dispatcher = RpcDispatcher::spawn(runtime, Arc::new(NoopHostHandler));

    // --- initialize ---
    let init_params = InitializeParams {
        protocol_version: PROTOCOL_VERSION.to_string(),
        host_info: HostInfo {
            name: "alva-smoke".to_string(),
            version: "0.0.0".to_string(),
        },
        host_capabilities: HostCapabilities {
            state_access: vec!["messages".to_string()],
            events: vec![methods::BEFORE_TOOL_CALL.to_string()],
            host_api: vec!["log".to_string()],
        },
    };

    let init_value = dispatcher
        .call(
            methods::INITIALIZE,
            Some(serde_json::to_value(&init_params).expect("serialize init params")),
        )
        .await
        .expect("initialize call succeeded");

    let init_result: InitializeResult =
        serde_json::from_value(init_value).expect("initialize result parses");
    assert_eq!(init_result.plugin.name, "echo");
    assert_eq!(init_result.plugin.version, "0.0.1");
    assert!(init_result.tools.is_empty());
    assert!(init_result.event_subscriptions.is_empty());

    // --- shutdown ---
    let shutdown_value = dispatcher
        .call(methods::SHUTDOWN, None)
        .await
        .expect("shutdown call succeeded");
    // Shutdown returns an empty object.
    assert!(shutdown_value.is_object());
    assert!(
        shutdown_value.as_object().unwrap().is_empty(),
        "shutdown result must be empty object, got: {}",
        shutdown_value
    );

    // Drive the dispatcher to fully tear down the subprocess.
    let status = dispatcher
        .shutdown()
        .await
        .expect("dispatcher shutdown returns exit status");
    assert!(
        status.success(),
        "python echo subprocess should exit cleanly, got {:?}",
        status
    );

    // Tempfile cleanup happens when `script` drops.
    drop(script);
}
