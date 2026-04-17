//! End-to-end test for the Python SDK at `sdk/python/alva_sdk`.
//!
//! Spawns a plugin file that imports `alva_sdk`, uses the
//! `@before_tool_call` decorator, and calls back into the host via
//! `self.host.log`. Proves:
//!
//! 1. The SDK's async JSON-RPC loop handshakes correctly
//! 2. Decorator-based handler discovery works end-to-end
//! 3. Host-side `AlvaHostHandler` accepts `host/log` during a nested
//!    handler invocation (bidirectional RPC on the same channel)
//! 4. A `Block` action survives round-trip through both sides
//!
//! Requires `python3` on PATH **and** the repo's `sdk/python/`
//! directory to be present (it always is for in-tree builds, but
//! the test skips cleanly otherwise so downstream vendored crates
//! do not fail).

#![cfg(not(target_family = "wasm"))]

use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::Arc;

use alva_app_extension_loader::dispatcher::RpcDispatcher;
use alva_app_extension_loader::host_api::AlvaHostHandler;
use alva_app_extension_loader::manifest::Runtime;
use alva_app_extension_loader::protocol::{
    methods, ExtensionAction, HostCapabilities, HostInfo, InitializeParams,
    InitializeResult, PROTOCOL_VERSION,
};
use alva_app_extension_loader::subprocess::{LauncherOverride, SubprocessRuntime};

const SDK_PLUGIN_PY: &str = r#"from alva_sdk import Extension, ToolCall, before_tool_call, run


class ShellGuard(Extension):
    name = "shell-guard"
    version = "0.1.0"
    description = "Test plugin using alva-sdk decorators"

    @before_tool_call
    async def guard(self, call: ToolCall):
        command = call.args.get("command", "")
        if "rm -rf" in command:
            # Exercise the host API reverse-call path too.
            await self.host.log(
                f"blocking {command}", level="warn", detail="test"
            )
            return self.block(f"rm -rf forbidden: {command}")
        return self.continue_()


if __name__ == "__main__":
    run(ShellGuard())
"#;

fn python_available() -> bool {
    StdCommand::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn sdk_root() -> PathBuf {
    // crates/alva-app-extension-loader  →  ..  →  crates  →  ..  →  repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .join("sdk")
        .join("python")
}

#[tokio::test(flavor = "multi_thread")]
async fn sdk_handshake_and_block_round_trip() {
    if !python_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let sdk_dir = sdk_root();
    if !sdk_dir.join("alva_sdk").join("__init__.py").exists() {
        eprintln!("skipping: alva_sdk not found at {}", sdk_dir.display());
        return;
    }

    // Write a throwaway plugin file that imports the SDK.
    let tmp = tempfile::tempdir().expect("tempdir");
    let plugin_path = tmp.path().join("plugin.py");
    std::fs::write(&plugin_path, SDK_PLUGIN_PY).expect("write plugin");

    // Spawn with PYTHONPATH pointing at sdk/python so the import
    // resolves without a `pip install`.
    let runtime = SubprocessRuntime::spawn(
        "sdk-smoke",
        Runtime::Python,
        plugin_path,
        None,
        Some(LauncherOverride {
            program: "python3".to_string(),
            prepend_args: vec!["-u".to_string()],
            env: vec![(
                "PYTHONPATH".to_string(),
                sdk_dir.display().to_string(),
            )],
        }),
    )
    .await
    .expect("spawn SDK plugin");

    // Use the real AlvaHostHandler so host/log actually succeeds.
    let dispatcher = RpcDispatcher::spawn(
        runtime,
        Arc::new(AlvaHostHandler::new("sdk-smoke")),
    );

    // ----- initialize -----
    let init_params = InitializeParams {
        protocol_version: PROTOCOL_VERSION.to_string(),
        host_info: HostInfo {
            name: "alva-sdk-test".to_string(),
            version: "0.0.0".to_string(),
        },
        host_capabilities: HostCapabilities {
            state_access: vec![],
            events: vec![methods::BEFORE_TOOL_CALL.to_string()],
            host_api: vec!["log".to_string()],
        },
    };
    let init_value = dispatcher
        .call(
            methods::INITIALIZE,
            Some(serde_json::to_value(&init_params).unwrap()),
        )
        .await
        .expect("initialize");
    let init_result: InitializeResult =
        serde_json::from_value(init_value).expect("parse init result");
    assert_eq!(init_result.plugin.name, "shell-guard");
    assert_eq!(init_result.plugin.version, "0.1.0");
    assert!(
        init_result
            .event_subscriptions
            .contains(&"before_tool_call".to_string()),
        "expected before_tool_call in subscriptions: {:?}",
        init_result.event_subscriptions
    );

    // ----- initialized notification -----
    dispatcher
        .notify(methods::INITIALIZED, None)
        .await
        .expect("initialized");

    // ----- dangerous command → expect Block (also exercises host/log reverse call) -----
    let block_params = serde_json::json!({
        "stateHandle": "test-state",
        "toolCall": {
            "id": "c1",
            "name": "shell",
            "arguments": {"command": "rm -rf /tmp/test"},
        }
    });
    let result = dispatcher
        .call(methods::BEFORE_TOOL_CALL, Some(block_params))
        .await
        .expect("before_tool_call rm -rf");
    let action: ExtensionAction =
        serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::Block { reason } => {
            assert!(
                reason.contains("rm -rf"),
                "unexpected block reason: {reason}"
            );
        }
        other => panic!("expected Block for rm -rf, got {:?}", other),
    }

    // ----- safe command → expect Continue -----
    let safe_params = serde_json::json!({
        "stateHandle": "test-state",
        "toolCall": {
            "id": "c2",
            "name": "shell",
            "arguments": {"command": "ls -la"},
        }
    });
    let result = dispatcher
        .call(methods::BEFORE_TOOL_CALL, Some(safe_params))
        .await
        .expect("before_tool_call ls");
    let action: ExtensionAction =
        serde_json::from_value(result).expect("parse action");
    assert!(
        matches!(action, ExtensionAction::Continue),
        "expected Continue for ls, got {:?}",
        action
    );

    // ----- shutdown -----
    let _ = dispatcher
        .call(methods::SHUTDOWN, None)
        .await
        .expect("shutdown");
    let status = dispatcher
        .shutdown()
        .await
        .expect("dispatcher teardown");
    assert!(
        status.success(),
        "SDK plugin should exit cleanly, got {:?}",
        status
    );

    drop(tmp);
}
