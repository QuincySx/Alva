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
use alva_app_extension_loader::host_api::{AlvaHostHandler, StateSnapshot};
use alva_app_extension_loader::manifest::Runtime;
use alva_app_extension_loader::protocol::{
    methods, ExtensionAction, HostCapabilities, HostInfo, InitializeParams, InitializeResult,
    PROTOCOL_VERSION,
};
use alva_app_extension_loader::subprocess::{LauncherOverride, SubprocessRuntime};

const SDK_PLUGIN_PY: &str = r#"from alva_sdk import (
    Message,
    Plugin,
    ToolCall,
    ToolResult,
    after_tool_call,
    before_tool_call,
    on_llm_call_end,
    on_llm_call_start,
    run,
    tool,
)


class ShellGuard(Plugin):
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
        if command == "rewrite":
            return self.modify_args({"command": "rewritten"})
        if command == "replace":
            return self.replace_result({
                "content": [{"type": "text", "text": "sdk replaced"}],
                "is_error": False,
            })
        if command == "state-rewrite":
            messages = await self.host.state_get_messages()
            latest = messages[-1].text if messages else ""
            return self.modify_args({"command": "state:" + latest})
        return self.continue_()

    @tool(
        name="remote_echo",
        description="Echo text from the Python plugin",
        input_schema={
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    )
    async def remote_echo(self, text: str):
        return ToolResult.text(f"remote:{text}")

    @on_llm_call_start
    async def rewrite_messages(self, messages):
        _original = messages[0].text if messages else ""
        return self.modify_messages([Message.system("sdk system")])

    @on_llm_call_end
    async def rewrite_response(self, response):
        _original = response.text
        return self.modify_response(Message.assistant("sdk response"))

    @after_tool_call
    async def rewrite_tool_result(self, call: ToolCall, result):
        return self.modify_result(ToolResult.text(f"sdk result:{call.name}:{result.text}"))


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
            env: vec![("PYTHONPATH".to_string(), sdk_dir.display().to_string())],
        }),
    )
    .await
    .expect("spawn SDK plugin");

    // Use the real AlvaHostHandler so host/log and host/state.* succeed.
    let host_handler = Arc::new(AlvaHostHandler::new("sdk-smoke"));
    host_handler.set_state_snapshot(StateSnapshot {
        handle: "test-state".to_string(),
        messages: vec![alva_kernel_abi::Message::user("sdk session")],
        metadata: serde_json::json!({"session_id": "sdk-session"}),
    });
    let dispatcher = RpcDispatcher::spawn(runtime, host_handler);

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
    assert!(
        init_result
            .event_subscriptions
            .contains(&"after_tool_call".to_string()),
        "expected after_tool_call in subscriptions: {:?}",
        init_result.event_subscriptions
    );
    assert!(
        init_result
            .event_subscriptions
            .contains(&"on_llm_call_start".to_string()),
        "expected on_llm_call_start in subscriptions: {:?}",
        init_result.event_subscriptions
    );
    assert!(
        init_result
            .event_subscriptions
            .contains(&"on_llm_call_end".to_string()),
        "expected on_llm_call_end in subscriptions: {:?}",
        init_result.event_subscriptions
    );
    assert_eq!(init_result.tools.len(), 1);
    assert_eq!(init_result.tools[0].name, "remote_echo");

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
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
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
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    assert!(
        matches!(action, ExtensionAction::Continue),
        "expected Continue for ls, got {:?}",
        action
    );

    // ----- rewritten command → expect Modify -----
    let rewrite_params = serde_json::json!({
        "stateHandle": "test-state",
        "toolCall": {
            "id": "c-rewrite",
            "name": "shell",
            "arguments": {"command": "rewrite"},
        }
    });
    let result = dispatcher
        .call(methods::BEFORE_TOOL_CALL, Some(rewrite_params))
        .await
        .expect("before_tool_call rewrite");
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::Modify { modified_arguments } => {
            assert_eq!(
                modified_arguments["command"],
                serde_json::json!("rewritten")
            );
        }
        other => panic!("expected Modify for rewrite, got {:?}", other),
    }

    // ----- replaced command → expect ReplaceResult -----
    let replace_params = serde_json::json!({
        "stateHandle": "test-state",
        "toolCall": {
            "id": "c-replace",
            "name": "shell",
            "arguments": {"command": "replace"},
        }
    });
    let result = dispatcher
        .call(methods::BEFORE_TOOL_CALL, Some(replace_params))
        .await
        .expect("before_tool_call replace");
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::ReplaceResult { result } => {
            assert_eq!(
                result["content"][0]["text"],
                serde_json::json!("sdk replaced")
            );
        }
        other => panic!("expected ReplaceResult for replace, got {:?}", other),
    }

    // ----- state-backed command → expect Modify using host/state.get_messages -----
    let state_params = serde_json::json!({
        "stateHandle": "test-state",
        "toolCall": {
            "id": "c-state",
            "name": "shell",
            "arguments": {"command": "state-rewrite"},
        }
    });
    let result = dispatcher
        .call(methods::BEFORE_TOOL_CALL, Some(state_params))
        .await
        .expect("before_tool_call state-rewrite");
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::Modify { modified_arguments } => {
            assert_eq!(
                modified_arguments["command"],
                serde_json::json!("state:sdk session")
            );
        }
        other => panic!("expected Modify for state-rewrite, got {:?}", other),
    }

    // ----- LLM start mutation → expect ModifyMessages -----
    let result = dispatcher
        .call(
            methods::ON_LLM_CALL_START,
            Some(serde_json::json!({
                "stateHandle": "test-state",
                "messages": [{
                    "id": "user-1",
                    "role": "user",
                    "content": [{"type": "text", "text": "hello"}],
                    "timestamp": 0
                }]
            })),
        )
        .await
        .expect("on_llm_call_start");
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::ModifyMessages { messages } => {
            assert_eq!(messages[0]["role"], serde_json::json!("system"));
            assert_eq!(
                messages[0]["content"][0]["text"],
                serde_json::json!("sdk system")
            );
        }
        other => panic!("expected ModifyMessages, got {:?}", other),
    }

    // ----- LLM end mutation → expect ModifyResponse -----
    let result = dispatcher
        .call(
            methods::ON_LLM_CALL_END,
            Some(serde_json::json!({
                "stateHandle": "test-state",
                "response": {
                    "id": "assistant-original",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "original"}],
                    "timestamp": 0
                }
            })),
        )
        .await
        .expect("on_llm_call_end");
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::ModifyResponse { response } => {
            assert_eq!(response["role"], serde_json::json!("assistant"));
            assert_eq!(
                response["content"][0]["text"],
                serde_json::json!("sdk response")
            );
        }
        other => panic!("expected ModifyResponse, got {:?}", other),
    }

    // ----- Tool result mutation → expect ModifyResult -----
    let result = dispatcher
        .call(
            methods::AFTER_TOOL_CALL,
            Some(serde_json::json!({
                "stateHandle": "test-state",
                "toolCall": {
                    "id": "c3",
                    "name": "shell"
                },
                "result": {
                    "content": [{"type": "text", "text": "original"}],
                    "is_error": false
                }
            })),
        )
        .await
        .expect("after_tool_call");
    let action: ExtensionAction = serde_json::from_value(result).expect("parse action");
    match action {
        ExtensionAction::ModifyResult { result } => {
            assert_eq!(
                result["content"][0]["text"],
                serde_json::json!("sdk result:shell:original")
            );
        }
        other => panic!("expected ModifyResult, got {:?}", other),
    }

    // ----- declared SDK tool -> expect ToolOutput-shaped response -----
    let tool_value = dispatcher
        .call(
            methods::TOOLS_CALL,
            Some(serde_json::json!({
                "name": "remote_echo",
                "arguments": {"text": "hello"},
            })),
        )
        .await
        .expect("tools/call remote_echo");
    assert_eq!(tool_value["isError"], serde_json::json!(false));
    assert_eq!(tool_value["content"][0]["type"], serde_json::json!("text"));
    assert_eq!(
        tool_value["content"][0]["text"],
        serde_json::json!("remote:hello")
    );

    // ----- shutdown -----
    let _ = dispatcher
        .call(methods::SHUTDOWN, None)
        .await
        .expect("shutdown");
    let status = dispatcher.shutdown().await.expect("dispatcher teardown");
    assert!(
        status.success(),
        "SDK plugin should exit cleanly, got {:?}",
        status
    );

    drop(tmp);
}
