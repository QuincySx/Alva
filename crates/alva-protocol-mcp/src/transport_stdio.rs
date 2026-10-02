// INPUT: tokio process/IO, serde_json, MCP transport/types, Unix libc
// OUTPUT: StdioTransport with bounded handshake, tool discovery/calls and owned-process cleanup
// POS: Native newline-delimited JSON-RPC MCP client; no shell or ambient credential inheritance

use crate::{
    error::McpError,
    transport::McpTransport,
    types::{McpServerConfig, McpToolInfo, McpTransportConfig},
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

const FRAME_LIMIT: usize = 1024 * 1024;
const TOOL_LIMIT: usize = 256;
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
fn failure(message: &str) -> McpError {
    McpError::Transport(message.into())
}

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    sequence: u64,
    has_tools: bool,
    connected: Arc<AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.connected.store(false, Ordering::SeqCst);
        // Kill the group BEFORE Tokio can reap the leader and recycle its PID.
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.start_kill();
    }
}

impl Session {
    async fn send(&mut self, value: Value) -> Result<(), McpError> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| failure("invalid outgoing JSON"))?;
        if bytes.len() > FRAME_LIMIT {
            return Err(failure("MCP frame exceeds 1 MiB"));
        }
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| failure("MCP stdin closed"))?;
        self.input
            .flush()
            .await
            .map_err(|_| failure("MCP stdin closed"))
    }

    async fn read(&mut self) -> Result<Value, McpError> {
        let mut bytes = Vec::new();
        loop {
            let chunk = self
                .output
                .fill_buf()
                .await
                .map_err(|_| failure("MCP stdout unavailable"))?;
            if chunk.is_empty() {
                return Err(failure("MCP server closed stdout"));
            }
            let end = chunk.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let length = end.unwrap_or(chunk.len());
            if bytes.len() + length > FRAME_LIMIT {
                return Err(failure("MCP frame exceeds 1 MiB"));
            }
            bytes.extend_from_slice(&chunk[..length]);
            self.output.consume(length);
            if end.is_some() {
                break;
            }
        }
        serde_json::from_slice(&bytes).map_err(|_| failure("MCP emitted invalid JSON"))
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        self.sequence += 1;
        let id = self.sequence;
        tokio::time::timeout(timeout, async {
            self.send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
                .await?;
            // A notification flood cannot keep a request alive indefinitely.
            for _ in 0..128 {
                let response = self.read().await?;
                if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                    return Err(failure("invalid MCP envelope"));
                }
                if let Some(method) = response.get("method").and_then(Value::as_str) {
                    if let Some(server_id) = response.get("id") {
                        let reply = if method == "ping" {
                            json!({"result":{}})
                        } else {
                            json!({"error":{"code":-32601,"message":"Unsupported client method"}})
                        };
                        let mut reply = reply.as_object().unwrap().clone();
                        reply.insert("jsonrpc".into(), json!("2.0"));
                        reply.insert("id".into(), server_id.clone());
                        self.send(Value::Object(reply)).await?;
                    }
                    continue;
                }
                if response.get("id") != Some(&json!(id)) {
                    return Err(failure("unexpected MCP response identity"));
                }
                if response.get("error").is_some() {
                    return Err(failure("MCP server rejected the request"));
                }
                return response
                    .get("result")
                    .filter(|v| v.is_object())
                    .cloned()
                    .ok_or_else(|| failure("missing MCP result"));
            }
            Err(failure("MCP notification limit exceeded"))
        })
        .await
        .map_err(|_| failure("MCP request timed out"))?
    }
}

pub struct StdioTransport {
    config: McpServerConfig,
    cwd: Option<PathBuf>,
    session: Mutex<Option<Session>>,
    connected: Arc<AtomicBool>,
}

impl StdioTransport {
    pub fn new(config: McpServerConfig, cwd: Option<PathBuf>) -> Self {
        Self {
            config,
            cwd,
            session: Mutex::new(None),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let mut slot = self.session.lock().await;
        // Taking ownership also makes future cancellation fail closed and reap
        // the child, rather than leaving a half-read RPC for the next caller.
        let mut session = slot
            .take()
            .ok_or_else(|| McpError::NotConnected(self.config.id.clone()))?;
        let result = session
            .request(method, params, Duration::from_secs(60))
            .await;
        if result.is_ok() {
            *slot = Some(session);
        }
        result
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn connect(&mut self) -> Result<(), McpError> {
        let McpTransportConfig::Stdio { command, args, env } = &self.config.transport else {
            return Err(failure("Only stdio MCP transport is implemented"));
        };
        if self.session.get_mut().is_some() {
            return Ok(());
        }
        let mut process = Command::new(command);
        process
            .args(args)
            .env_clear()
            .env("PATH", "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin")
            .env("LANG", "en_US.UTF-8")
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(cwd) = &self.cwd {
            process.current_dir(cwd);
        }
        #[cfg(unix)]
        process.process_group(0);
        let mut child = process
            .spawn()
            .map_err(|_| failure("MCP executable could not start"))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| failure("MCP stdin unavailable"))?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| failure("MCP stdout unavailable"))?,
        );
        let mut session = Session {
            child,
            input,
            output,
            sequence: 0,
            has_tools: false,
            connected: self.connected.clone(),
        };
        let result = session.request("initialize", json!({"protocolVersion":VERSIONS[0], "capabilities":{}, "clientInfo":{"name":"alva", "version":env!("CARGO_PKG_VERSION")}}), Duration::from_secs(self.config.connect_timeout_secs.clamp(1, 60) as u64)).await?;
        if !result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .is_some_and(|version| VERSIONS.contains(&version))
        {
            return Err(failure("Unsupported MCP protocol version"));
        }
        if !result.get("capabilities").is_some_and(Value::is_object)
            || !result.get("serverInfo").is_some_and(|info| {
                info.get("name").is_some_and(Value::is_string)
                    && info.get("version").is_some_and(Value::is_string)
            })
        {
            return Err(failure("Invalid MCP initialization response"));
        }
        session.has_tools = result
            .get("capabilities")
            .and_then(|v| v.get("tools"))
            .is_some_and(Value::is_object);
        tokio::time::timeout(
            Duration::from_secs(5),
            session.send(json!({"jsonrpc":"2.0", "method":"notifications/initialized"})),
        )
        .await
        .map_err(|_| failure("MCP initialization timed out"))??;
        *self.session.get_mut() = Some(session);
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), McpError> {
        if let Some(mut session) = self.session.get_mut().take() {
            let _ = session.input.shutdown().await;
            #[cfg(unix)]
            if let Some(pid) = session.child.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = session.child.kill().await;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        if !self
            .session
            .lock()
            .await
            .as_ref()
            .is_some_and(|s| s.has_tools)
        {
            return Ok(vec![]);
        }
        let mut tools = Vec::new();
        let mut names = HashSet::new();
        let mut cursors = HashSet::new();
        let mut cursor = None::<String>;
        for _ in 0..16 {
            let page = self
                .request(
                    "tools/list",
                    cursor
                        .as_ref()
                        .map(|c| json!({"cursor":c}))
                        .unwrap_or(json!({})),
                )
                .await?;
            for item in page
                .get("tools")
                .and_then(Value::as_array)
                .ok_or_else(|| failure("invalid MCP tool list"))?
            {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| failure("missing MCP tool name"))?;
                let schema = item
                    .get("inputSchema")
                    .filter(|v| {
                        v.is_object() && v.get("type").and_then(Value::as_str) == Some("object")
                    })
                    .ok_or_else(|| failure("invalid MCP tool schema"))?;
                let description = item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if tools.len() >= TOOL_LIMIT
                    || name.is_empty()
                    || name.len() > 128
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
                    || !names.insert(name.to_string())
                    || description.len() > 16384
                    || schema.to_string().len() > 65536
                {
                    return Err(failure("MCP tool catalog invalid or exceeds limits"));
                }
                tools.push(McpToolInfo {
                    server_id: self.config.id.clone(),
                    tool_name: name.into(),
                    description: description.into(),
                    input_schema: schema.clone(),
                });
            }
            match page.get("nextCursor") {
                None | Some(Value::Null) => return Ok(tools),
                Some(Value::String(next))
                    if !next.is_empty() && next.len() <= 1024 && cursors.insert(next.clone()) =>
                {
                    cursor = Some(next.clone())
                }
                _ => return Err(failure("invalid MCP pagination cursor")),
            }
        }
        Err(failure("MCP pagination limit exceeded"))
    }

    async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value, McpError> {
        if !arguments.is_object() {
            return Err(failure("MCP tool arguments must be an object"));
        }
        let result = self
            .request(
                "tools/call",
                json!({"name":tool_name,"arguments":arguments}),
            )
            .await?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(McpError::ToolExecution("MCP tool reported failure".into()));
        }
        Ok(result)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    // An in-process authored fixture, not downloaded package/server code.
    const SERVER: &str = r#"
import json, sys, time, os
mode = sys.argv[1]
initialized = False
for line in sys.stdin:
    r = json.loads(line)
    method = r.get('method')
    if method == 'notifications/initialized':
        initialized = True
        continue
    if method == 'initialize':
        if mode == 'stall':
            time.sleep(10)
        result = {'protocolVersion': 'bad-version' if mode == 'version' else r['params']['protocolVersion'], 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'fixture', 'version': '1'}}
    elif method == 'tools/list':
        assert initialized
        if mode == 'oversize':
            print('x' * 1100000, flush=True)
            continue
        second = r.get('params', {}).get('cursor') == 'page-2'
        result = {'tools': [{'name': 'second' if second else 'echo', 'description': 'Fixture', 'inputSchema': {'type': 'object'}}]}
        if mode == 'pages' and not second:
            result['nextCursor'] = 'page-2'
        if mode == 'repeat':
            result['nextCursor'] = 'same'
    elif method == 'tools/call':
        if mode == 'cancel':
            time.sleep(10)
        if mode == 'error':
            result = {'isError': True, 'content': [{'type': 'text', 'text': 'synthetic-private-value'}]}
        else:
            result = {'content': [{'type': 'text', 'text': json.dumps(r['params']['arguments'])}], 'structuredContent': {'cwd': os.getcwd(), 'ambient_home': 'HOME' in os.environ}}
    else:
        result = {}
    print(json.dumps({'jsonrpc':'2.0', 'id':r['id'], 'result':result}), flush=True)
"#;

    fn transport(mode: &str, cwd: &PathBuf) -> StdioTransport {
        StdioTransport::new(
            McpServerConfig {
                id: "fixture".into(),
                display_name: "Fixture".into(),
                transport: McpTransportConfig::Stdio {
                    command: "/usr/bin/python3".into(),
                    args: vec!["-u".into(), "-c".into(), SERVER.into(), mode.into()],
                    env: Default::default(),
                },
                auto_connect: true,
                connect_timeout_secs: 1,
            },
            Some(cwd.clone()),
        )
    }

    #[tokio::test]
    async fn stdio_handshake_pagination_call_and_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = transport("pages", &dir.path().to_path_buf());
        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert!(client.call_tool("echo", json!([])).await.is_err());
        assert!(client.is_connected());
        assert_eq!(
            tools
                .iter()
                .map(|t| t.tool_name.as_str())
                .collect::<Vec<_>>(),
            vec!["echo", "second"]
        );
        let result = client
            .call_tool("echo", json!({"literal":"$(not-a-shell)"}))
            .await
            .unwrap();
        assert_eq!(result["structuredContent"]["ambient_home"], false);
        assert_eq!(
            PathBuf::from(result["structuredContent"]["cwd"].as_str().unwrap()),
            dir.path().canonicalize().unwrap()
        );
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("$(not-a-shell)"));
        client.disconnect().await.unwrap();
        assert!(!client.is_connected());
        assert!(client.call_tool("echo", json!({})).await.is_err());
    }

    #[tokio::test]
    async fn rejects_version_timeout_oversize_and_repeated_cursors() {
        // Keep a real directory alive for all subprocess cwd bindings.
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().to_path_buf();
        for mode in ["version", "stall"] {
            let mut client = transport(mode, &dir);
            assert!(client.connect().await.is_err());
            assert!(!client.is_connected());
        }
        for mode in ["oversize", "repeat"] {
            let mut client = transport(mode, &dir);
            client.connect().await.unwrap();
            assert!(client.list_tools().await.is_err());
            client.disconnect().await.unwrap();
        }
    }

    #[tokio::test]
    async fn cancellation_invalidates_rpc_and_tool_errors_do_not_leak_payloads() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().to_path_buf();
        let mut client = transport("cancel", &dir);
        client.connect().await.unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            client.call_tool("echo", json!({}))
        )
        .await
        .is_err());
        assert!(!client.is_connected());
        let mut client = transport("error", &dir);
        client.connect().await.unwrap();
        let error = client
            .call_tool("echo", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("synthetic-private-value"));
        client.disconnect().await.unwrap();
    }
}
