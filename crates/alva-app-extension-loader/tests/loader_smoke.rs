//! End-to-end loader smoke test.
//!
//! Writes a tiny Python plugin to a tempdir, registers
//! `SubprocessLoaderPlugin` with a real `PluginHost`, verifies that the
//! remote subscription is assembled as a phase contribution, and drives
//! `before_tool_call` through the generated phase adapter middleware.
//! The plugin
//! blocks shell calls containing `"rm -rf"` and lets everything else
//! through.
//!
//! Requires `python3` on PATH; skips cleanly otherwise.

#![cfg(not(target_family = "wasm"))]

use std::process::Command as StdCommand;
use std::sync::{Arc, RwLock};

use alva_agent_core::extension::{Plugin, PluginHost, Registrar};
use alva_agent_core::AgentBuilder;
use alva_app_extension_loader::loader::SubprocessLoaderPlugin;
use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{
    AgentError, AgentMessage, Message, MessageRole, MinimalExecutionContext, Phase, PhaseEffect,
    ToolCall, ToolOutput,
};
use alva_kernel_core::middleware::ToolCallFn;
use alva_kernel_core::{AgentState, Extensions, Middleware};
use alva_test::mock_provider::MockLanguageModel;
use async_trait::async_trait;

/// Build a minimal `AgentState` sufficient to call middleware hooks.
/// The model and tools are inert placeholders.
fn make_state(session: Arc<InMemoryAgentSession>) -> AgentState {
    AgentState {
        model: Arc::new(MockLanguageModel::new()),
        tools: vec![],
        session,
        extensions: Extensions::new(),
    }
}

/// Pull the compiled phase handler middleware out of the host so the
/// test can call its hooks directly.
fn take_before_tool_phase_adapter(host: &Arc<RwLock<PluginHost>>) -> Arc<dyn Middleware> {
    let mws = host.write().unwrap().take_middlewares();
    assert!(
        !mws.iter().any(|m| m.name() == "aep-bridge"),
        "loader must not register the legacy aggregate aep-bridge middleware"
    );
    assert!(
        !mws.iter().any(|m| m.name().starts_with("aep-phase:")),
        "loader must not register loader-specific phase adapter middleware"
    );
    mws.into_iter()
        .find(|m| m.name() == "phase:aep:shell-guard:before_tool_call")
        .expect("loader must register an executable before_tool_call phase handler")
}

fn take_phase_adapters(host: &Arc<RwLock<PluginHost>>) -> Vec<Arc<dyn Middleware>> {
    host.write().unwrap().take_middlewares()
}

struct EchoToolCall;

#[async_trait]
impl ToolCallFn for EchoToolCall {
    async fn call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput::text(format!(
            "executed:{}",
            tool_call.arguments["command"].as_str().unwrap_or("")
        )))
    }
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


def call_host(req_id, method, params):
    send({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})
    while True:
        raw = sys.stdin.readline()
        if not raw:
            raise RuntimeError("host closed stdin")
        msg = json.loads(raw)
        if msg.get("id") == req_id:
            return msg


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
                if cmd == "rewrite":
                    send(
                        {
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {
                                "action": "modify",
                                "modified_arguments": {"command": "rewritten"},
                            },
                        }
                    )
                    continue
                if cmd == "state-rewrite":
                    state = call_host(
                        "plugin-state-1",
                        "host/state.get_messages",
                        {"handle": params.get("stateHandle")},
                    )
                    messages = state.get("result", {}).get("messages", [])
                    latest_text = ""
                    if messages:
                        content = messages[-1].get("content", [])
                        if content:
                            latest_text = content[0].get("text", "")
                    send(
                        {
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {
                                "action": "modify",
                                "modified_arguments": {"command": "state:" + latest_text},
                            },
                        }
                    )
                    continue
                if cmd == "replace":
                    send(
                        {
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {
                                "action": "replace_result",
                                "result": {
                                    "content": [
                                        {"type": "text", "text": "replaced before execution"}
                                    ],
                                    "is_error": False,
                                },
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

const TOOL_PLUGIN_MANIFEST: &str = r#"
name = "remote-tools"
version = "0.0.1"
description = "Test plugin that exposes a remote tool"
runtime = "python"
entry = "main.py"
"#;

const TOOL_PLUGIN_PY: &str = r#"import json
import sys


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def main():
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        req = json.loads(line)
        method = req.get("method")
        req_id = req.get("id")
        if method == "initialize":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "0.1.0",
                    "plugin": {"name": "remote-tools", "version": "0.0.1"},
                    "tools": [{
                        "name": "remote_echo",
                        "description": "Echo text through a remote plugin",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"text": {"type": "string"}},
                            "required": ["text"]
                        }
                    }],
                    "eventSubscriptions": [],
                    "requestedCapabilities": []
                }
            })
        elif method == "initialized":
            pass
        elif method == "tools/call":
            params = req.get("params") or {}
            args = params.get("arguments") or {}
            text = args.get("text", "")
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "content": [{"type": "text", "text": "remote:" + text}],
                    "is_error": False
                }
            })
        elif method == "shutdown":
            send({"jsonrpc": "2.0", "id": req_id, "result": {}})
            return
        else:
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {"code": -32601, "message": "unknown method"}
            })


if __name__ == "__main__":
    main()
"#;

const LLM_MUTATOR_MANIFEST: &str = r#"
name = "llm-mutator"
version = "0.0.1"
description = "Test plugin that mutates LLM messages"
runtime = "python"
entry = "main.py"
"#;

const LLM_MUTATOR_PY: &str = r#"import json
import sys


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def text_message(msg_id, role, text):
    return {
        "id": msg_id,
        "role": role,
        "content": [{"type": "text", "text": text}],
        "timestamp": 0,
    }


def main():
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        req = json.loads(line)
        method = req.get("method")
        req_id = req.get("id")
        if method == "initialize":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "0.1.0",
                    "plugin": {"name": "llm-mutator", "version": "0.0.1"},
                    "tools": [],
                    "eventSubscriptions": [
                        "on_llm_call_start",
                        "on_llm_call_end",
                        "after_tool_call"
                    ],
                    "requestedCapabilities": []
                }
            })
        elif method == "initialized":
            pass
        elif method == "extension/on_llm_call_start":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "action": "modify_messages",
                    "messages": [text_message("sys-1", "system", "mutated system")]
                }
            })
        elif method == "extension/on_llm_call_end":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "action": "modify_response",
                    "response": text_message("assistant-1", "assistant", "mutated response")
                }
            })
        elif method == "extension/after_tool_call":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "action": "modify_result",
                    "result": {
                        "content": [{"type": "text", "text": "mutated tool result"}],
                        "is_error": False
                    }
                }
            })
        elif method == "shutdown":
            send({"jsonrpc": "2.0", "id": req_id, "result": {}})
            return
        else:
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {"code": -32601, "message": "unknown method"}
            })


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
    let host = Arc::new(RwLock::new(PluginHost::new()));
    let ext = SubprocessLoaderPlugin::new(vec![temp.path().to_path_buf()]);

    // Drive the single `register` lifecycle phase against a Registrar
    // backed by the real host — this loads the subprocess plugins and
    // registers phase contributions on the host, exactly as the agent
    // builder does.
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
    assert_eq!(ext.loaded_count(), 1, "expected exactly one plugin loaded");

    let phase_contributions = host.write().unwrap().take_phase_contributions();
    assert_eq!(phase_contributions.len(), 1);
    assert_eq!(phase_contributions[0].0, "subprocess-loader");
    assert_eq!(
        phase_contributions[0].1.name,
        "aep:shell-guard:before_tool_call"
    );
    assert_eq!(phase_contributions[0].1.phase, Phase::BeforeToolCall);
    assert_eq!(phase_contributions[0].1.effect, PhaseEffect::Wrap);

    // Grab the generated phase adapter, and a state to drive it.
    let bridge = take_before_tool_phase_adapter(&host);
    let session = Arc::new(InMemoryAgentSession::new());
    session
        .append_message(AgentMessage::Standard(Message::user("session hello")), None)
        .await;
    let mut state = make_state(session);

    // --- on_agent_start: unsubscribed plugin → no error ---
    bridge
        .on_agent_start(&mut state)
        .await
        .expect("on_agent_start must not fail for unsubscribed plugin");

    // --- dangerous command: expect blocked ToolOutput ---
    let blocked = ToolCall {
        id: "call-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "rm -rf /"}),
    };
    let result = bridge
        .wrap_tool_call(&mut state, &blocked, &EchoToolCall)
        .await
        .expect("blocked tool call should return an error result");
    assert!(result.is_error);
    assert!(result.model_text().contains("rm -rf"));

    // --- safe command: expect Ok (Continue) ---
    let safe = ToolCall {
        id: "call-2".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "ls -la"}),
    };
    let result = bridge
        .wrap_tool_call(&mut state, &safe, &EchoToolCall)
        .await
        .expect("expected Ok (continue) for ls");
    assert_eq!(result.model_text(), "executed:ls -la");

    // --- modified command: expect rewritten args to reach the tool ---
    let modified = ToolCall {
        id: "call-3".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "rewrite"}),
    };
    let result = bridge
        .wrap_tool_call(&mut state, &modified, &EchoToolCall)
        .await
        .expect("expected modified args to execute");
    assert_eq!(result.model_text(), "executed:rewritten");

    // --- state-backed command: plugin reads host/state.get_messages ---
    let state_rewrite = ToolCall {
        id: "call-state".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "state-rewrite"}),
    };
    let result = bridge
        .wrap_tool_call(&mut state, &state_rewrite, &EchoToolCall)
        .await
        .expect("expected state-backed args to execute");
    assert_eq!(result.model_text(), "executed:state:session hello");

    // --- replaced command: expect plugin result without execution ---
    let replaced = ToolCall {
        id: "call-4".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "replace"}),
    };
    let result = bridge
        .wrap_tool_call(&mut state, &replaced, &EchoToolCall)
        .await
        .expect("expected replace_result to skip execution");
    assert_eq!(result.model_text(), "replaced before execution");

    // Orderly teardown — kills the subprocess.
    ext.shutdown_all().await.expect("shutdown_all");
    drop(temp);
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_mutates_llm_messages_and_response() {
    if !python_available() {
        eprintln!("skipping plugin_mutates_llm_messages_and_response: python3 not on PATH");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = temp.path().join("llm-mutator");
    std::fs::create_dir(&plugin_dir).expect("create plugin dir");
    std::fs::write(plugin_dir.join("alva.toml"), LLM_MUTATOR_MANIFEST).expect("write manifest");
    std::fs::write(plugin_dir.join("main.py"), LLM_MUTATOR_PY).expect("write python entry");

    let host = Arc::new(RwLock::new(PluginHost::new()));
    let ext = SubprocessLoaderPlugin::new(vec![temp.path().to_path_buf()]);
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
    assert_eq!(ext.loaded_count(), 1, "expected exactly one plugin loaded");

    let phase_contributions = host.write().unwrap().take_phase_contributions();
    assert_eq!(phase_contributions.len(), 3);
    assert!(
        phase_contributions
            .iter()
            .any(|(_, c)| c.name == "aep:llm-mutator:on_llm_call_start"
                && c.phase == Phase::BeforeLlmCall
                && c.effect == PhaseEffect::Mutate),
        "missing llm start contribution: {phase_contributions:?}"
    );
    assert!(
        phase_contributions
            .iter()
            .any(|(_, c)| c.name == "aep:llm-mutator:on_llm_call_end"
                && c.phase == Phase::AfterLlmCall
                && c.effect == PhaseEffect::Mutate),
        "missing llm end contribution: {phase_contributions:?}"
    );
    assert!(
        phase_contributions
            .iter()
            .any(|(_, c)| c.name == "aep:llm-mutator:after_tool_call"
                && c.phase == Phase::AfterToolCall
                && c.effect == PhaseEffect::Mutate),
        "missing after tool contribution: {phase_contributions:?}"
    );

    let adapters = take_phase_adapters(&host);
    let start = adapters
        .iter()
        .find(|m| m.name() == "phase:aep:llm-mutator:on_llm_call_start")
        .expect("llm start adapter")
        .clone();
    let end = adapters
        .iter()
        .find(|m| m.name() == "phase:aep:llm-mutator:on_llm_call_end")
        .expect("llm end adapter")
        .clone();
    let after_tool = adapters
        .iter()
        .find(|m| m.name() == "phase:aep:llm-mutator:after_tool_call")
        .expect("after tool adapter")
        .clone();

    let session = Arc::new(InMemoryAgentSession::new());
    let mut state = make_state(session);

    let mut messages = vec![Message::user("original")];
    start
        .before_llm_call(&mut state, &mut messages)
        .await
        .expect("llm start mutation should succeed");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, MessageRole::System);
    assert_eq!(messages[0].text_content(), "mutated system");

    let mut response = Message::system("original response");
    end.after_llm_call(&mut state, &mut response)
        .await
        .expect("llm end mutation should succeed");
    assert_eq!(response.role, MessageRole::Assistant);
    assert_eq!(response.text_content(), "mutated response");

    let tool_call = ToolCall {
        id: "call-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({"command": "echo original"}),
    };
    let mut result = alva_kernel_abi::ToolOutput::text("original tool result");
    after_tool
        .after_tool_call(&mut state, &tool_call, &mut result)
        .await
        .expect("after tool mutation should succeed");
    assert!(!result.is_error);
    assert_eq!(result.model_text(), "mutated tool result");

    ext.shutdown_all().await.expect("shutdown_all");
    drop(temp);
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_declared_tools_register_as_agent_tools_and_execute() {
    if !python_available() {
        eprintln!("skipping plugin_declared_tools_register_as_agent_tools_and_execute: python3 not on PATH");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = temp.path().join("remote-tools");
    std::fs::create_dir(&plugin_dir).expect("create plugin dir");
    std::fs::write(plugin_dir.join("alva.toml"), TOOL_PLUGIN_MANIFEST).expect("write manifest");
    std::fs::write(plugin_dir.join("main.py"), TOOL_PLUGIN_PY).expect("write python entry");

    let agent = AgentBuilder::new()
        .model(Arc::new(MockLanguageModel::new()))
        .plugin(Box::new(SubprocessLoaderPlugin::new(vec![temp
            .path()
            .to_path_buf()])))
        .build()
        .await
        .expect("agent build should succeed");

    let tool = agent
        .tools()
        .iter()
        .find(|tool| tool.name() == "remote_echo")
        .expect("remote tool should be registered")
        .clone();
    assert_eq!(tool.description(), "Echo text through a remote plugin");
    assert_eq!(tool.parameters_schema()["required"][0], "text");

    let output = tool
        .execute(
            serde_json::json!({"text": "hello"}),
            &MinimalExecutionContext::new(),
        )
        .await
        .expect("remote tool should execute");
    assert!(!output.is_error);
    assert_eq!(output.model_text(), "remote:hello");
}
