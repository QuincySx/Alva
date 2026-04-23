# MCP Error Handling

Amp 对 MCP 错误做了一层**分类 + 人类可读的补救提示**。核心是 `vCT()` 函数（错误规范化 → 4 类 code），以及 tool invocation 结果里的 image error 兜底、text error 兜底两个细节。

## 错误分类：`vCT(error)` 函数

反编译原文（精简展示关键片段）：

```js
// from strings.txt:62340 (`vCT`) 
function vCT(T, R, t = false) {
  let r = T.message;

  // C9 是 MCP SDK 的 ProtocolError 类；按 JSON-RPC 错误 code 映射
  if (T instanceof C9) switch (T.code) {
    case s9.RequestTimeout:    return { code: "timeout", message: r };
    case s9.ConnectionClosed:  return { code: "network", message: r || "Connection closed unexpectedly" };
    case s9.InvalidRequest:
    case s9.InvalidParams:
    // ... 其它协议码
  }

  // 消息字符串匹配（fallback）
  let e = "server-error";
  if (r.includes("timeout") || r.includes("Timeout"))
    e = "timeout";
  else if (r.includes("OAuth") ||
           r.includes("authorization") ||
           r.includes("Unauthorized") ||
           r.includes("401"))
    e = "auth-failed";
  else if (r.includes("fetch failed") ||
           r.includes("network") ||
           r.includes("ECONNREFUSED") ||
           r.includes("ECONNRESET") || ...)
    e = "network";

  return { code: e, message: r, stderr: T.stderr };
}
```

四类 error code：`timeout | auth-failed | network | server-error`。还附带：
- `stderr`：StdioClientTransport 捕获的 child process 的 stderr（调试用）
- `message`：原始 error 消息

## 错误文案模板（含补救指引）

每一类错误都会附带**怎么 fix** 的说明，这是 Amp 处理 DX 的重点：

**OAuth 相关**（auth-failed）：

```
If this is due to stale OAuth credentials, clear them and retry:
  amp mcp oauth logout <name>

If this server doesn't support OAuth, add authentication headers to your config.

If it does support OAuth, ensure you've registered with:
  amp mcp oauth login <server-name> --server-url <url> --client-id <id> --auth-url <url> --token-url <url>

Try registering OAuth credentials manually:
  amp mcp oauth login <name> --server-url <url> --client-id <id>
Required: --server-url, --client-id
Optional: --auth-url, --token-url (auto-discovered if not provided), --client-secret, --scopes

If manual registration doesn't work, this server likely doesn't support OAuth.
```

**Workspace trust**（denied / awaiting-approval）：

```
To fix: Add "amp.mcpTrustedWorkspaces": ["<workspace-root>"] to <settings.json>
```

**Port conflict**（OAuth callback server）：

```
OAuth callback port 8976 is already in use.
1. Run: lsof -i :8976 | grep LISTEN
2. Kill the process: kill <PID>
3. Then retry the OAuth flow
```

## Tool invocation 错误兜底

当 LLM 调一个 MCP tool，结果里可能有：
- 正常 text content
- image content（需要转 base64 data URL）
- 错误但没具体消息（上游 server bug）

### 空错误兜底（`RdR`）

```js
// from strings.txt:62339~62340
function RdR(T, R) {
  let t = R
    .filter((r) => r.type === "text")
    .map((r) => r.text.trim())
    .join("\n");
  return `MCP tool "${T}" returned an error response without details.`;
}
```

server 返回 `isError: true` 但 content 空或没 text 块时，回这句占位，避免 LLM 看到空字符串困惑。

### Image content 错误兜底（`tdR`）

```js
// from strings.txt:62339
if (R.type === "image") {
  let t = tdR(R.data);  // 校验 base64
  if (t) return { type: "text", text: `[MCP image error: ${t}]` };
}
```

image data base64 decode 失败（损坏或格式错）就降级为 text 节点 `[MCP image error: ...]`，不让整个 tool call 崩。

### Tool output 截断

```js
// from strings.txt:62339
... [Tool result truncated - showing first ${Math.round(FL/1024)}KB of ${e}KB total.
     The tool result was too long and has been shortened.
     Consider using more specific queries or parameters to get focused results.]
```

`FL` 是 tool output 的字节预算（和 resource 的 `$z` 不同变量）。截断时模型会看到原样提示，建议它用更精确的 query。

## Connect-time 错误展示

反编译里的 startup 诊断路径：

```js
// from strings.txt:63796
await Promise.all([T.mcpService.initialized, T.toolboxService.initialized]);
let R = await g0(T.mcpService.servers);
for (let r of R)
  if (r.status.type === "failed")
    T.stderr.write(`error connecting to ${r.name}: ${r.status.error.message || "Unknown error"}`);
```

stderr 而不是 TUI 展示，适合 `-x`（execute mode）/ 非交互场景。

TUI 模式另走 UI 路径（`├── ✗ <name>: <error-message>` 红字）。

## Reconnect 策略

反编译里 `reconnecting` 状态能从 `connected` 转入，字符串里有：
```
case "reconnecting": return `${name}: reconnecting...`;
```

但**没找到**明确的退避算法 / 最大重试次数的字符串。`Lsparse` 这类工具没对 MCP SDK 作 reconnect 强化，猜测就是 MCP SDK 默认（transport 层断开 → server 主动 close → client 可选 reconnect）。Amp 层面看起来没做 exponential backoff 也没次数封顶，遇到持续失败最终进 `failed`。

## Timeout 处理

`connect_timeout_secs` 在 config 里（Amp 默认未明确但和 Alva 一致走 30s 左右）。

tool call 内部超时：反编译里看到部分 tool 有 `meta: { disableTimeout: true }`（Bash 和某些 subagent），其它 tool 走默认 timeout。MCP tool 的 default timeout 没找到明确字符串，应该是 MCP SDK 的 request timeout（典型 60s）。

## 对 Alva 的启发

当前 `alva-protocol-mcp/src/error.rs`（94 行）有 `McpError` 枚举但**没分类 code**：

```rust
// 现状大致
pub enum McpError {
    ServerNotFound(String),
    Transport(String),
    Serialization(String),
    Io(String),
    Protocol(String),
}
```

建议重构成两层：**根因枚举** + **分类 code**。

```rust
// 根因
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("server {0} not found")]
    ServerNotFound(String),
    #[error("connect timeout after {0}s")]
    Timeout(u32),
    #[error("auth failed: {0}")]
    AuthFailed(String),
    #[error("network: {0}")]
    Network(String),
    #[error("server error: {0}")]
    ServerError(String),
    #[error("workspace not trusted")]
    WorkspaceNotTrusted,
    #[error("oauth required (auth_url: {auth_url})")]
    OauthRequired { auth_url: String },
    // ...
}

// 展示层 code（UI 用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum McpErrorCode {
    Timeout,
    AuthFailed,
    Network,
    ServerError,
    Trust,
    Oauth,
}

impl McpError {
    pub fn code(&self) -> McpErrorCode { ... }
    pub fn remediation(&self) -> &'static str { ... }  // 补救指引
}
```

具体做法建议：

1. **分类靠精确信号**（不要用 Amp 的字符串 `.contains("401")`）：
   - `reqwest::StatusCode` 判 auth-failed
   - `tokio::time::error::Elapsed` 判 timeout
   - `std::io::ErrorKind::ConnectionRefused / ConnectionReset` 判 network
2. **`remediation()` 方法**返回多行 str，extension / Tauri UI 直接展示。Amp 的补救文案**是资产**，值得抄的清单：
   - auth-failed：提示 logout / 重新 login / 检查 server 支不支持 OAuth
   - trust：提示 `approve` 命令和 settings key
   - port conflict：`lsof + kill` 提示
3. **`McpError::source_stderr(&self) -> Option<&str>`**：stdio transport 的 child process stderr。用户一看就知道 MCP server 为什么启动失败。reqwest / SSE 场景这个返回 None。
4. **`is_retryable(&self) -> bool`**：network / timeout 返回 true；auth-failed / trust / oauth-required 返回 false。`McpClient` 的 reconnect 逻辑查这个决定要不要重连。
5. **Empty result tool fallback**：在 `McpToolAdapter::execute()` 里判空：
   ```rust
   if result.content.is_empty() && !result.is_error {
       return Ok(ToolOutput::text(format!(
           "MCP tool \"{}\" returned empty content.",
           self.info.tool_name
       )));
   }
   if result.is_error {
       let msg = extract_error_text(&result.content);
       return Err(AgentError::ToolError {
           tool_name: self.full_name.clone(),
           message: msg.unwrap_or_else(|| format!(
               "MCP tool \"{}\" returned an error response without details.",
               self.info.tool_name
           )),
       });
   }
   ```
6. **Tool output 截断**：和 Amp 一致的文案模板，`MAX_TOOL_OUTPUT_BYTES = 65536`（64KB，tool 比 resource 紧）。

**不要**把 reconnect 策略放进 `McpClient` —— 应该做成独立的 `McpReconnectPolicy` trait（exponential / fixed / never），extension 注入。Alva 的 kernel 哲学是"策略可替换"。
