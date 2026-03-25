# Multi-Engine Runtime Adapter: alva-engine-runtime + alva-engine-adapter-claude

## Overview

引入多引擎运行时抽象层，使上层应用可以通过统一接口对接多种 Agent 引擎（自建 alva-core、Claude Code、OpenClaw、ACP 远程 Agent），通过配置动态切换，不绑定单一实现。

本次实现范围：
1. **alva-engine-runtime** — `EngineRuntime` trait + 统一事件/请求/错误类型
2. **alva-engine-adapter-claude** — Claude Agent SDK Bridge 适配器

## Background

### 问题

当前 `BaseAgent`（alva-app-core）硬编码了 alva-core 作为唯一 Agent 引擎。无法接入 Claude Code、OpenClaw 等外部引擎。

### 参考实现

**LobsterAI（netease-youdao/LobsterAI）** 已实现类似架构：

```
CoworkRuntime（接口）
├── ClaudeRuntimeAdapter    → 包装 CoworkRunner（调 SDK query()）
├── OpenClawRuntimeAdapter  → 包装 OpenClaw Gateway
└── CoworkEngineRouter      → 路由器，按配置分发
```

关键发现：
- LobsterAI patch 了 Claude Agent SDK（`spawn` → `fork`）解决 Electron 打包问题，我们是 Rust 原生，无此问题
- SDK 内部 spawn 的是 Node.js 子进程跑 `cli.js`，不是 `claude` CLI 二进制
- `canUseTool` 交互式权限回调是 SDK 模式独有能力，CLI 模式不支持

### Claude Agent SDK 工作原理

```
SDK query() → spawn Node.js 子进程（ProcessTransport）
                │
     stdin  ◀───┤  JSON 控制消息（prompt, permission response, interrupt）
     stdout ──▶ │  JSON-line 事件流（SDKMessage）
                │
                └── 内部 Agent Loop（LLM ↔ 内置工具）
```

核心类型：`SDKMessage` 联合类型（20+ 种消息），通过 `AsyncGenerator<SDKMessage>` 流式返回。

详细分析见：
- `~/docs/github-article/netease-youdao-lobsterai-blueprint.md`
- `~/docs/github-article/anthropics-claude-agent-sdk-typescript-blueprint.md`

## Crate Structure

```
crates/
├── alva-engine-runtime/            ← EngineRuntime trait + 统一类型（纯接口，零引擎依赖）
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                  ← re-exports
│       ├── runtime.rs              ← AgentRuntime trait
│       ├── event.rs                ← RuntimeEvent, StreamDelta, RuntimeUsage
│       ├── request.rs              ← RuntimeRequest, RuntimeOptions
│       └── error.rs                ← RuntimeError
│
└── alva-engine-adapter-claude/     ← Claude SDK Bridge 适配器
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs                  ← re-exports
    │   ├── adapter.rs              ← ClaudeAdapter impl AgentRuntime
    │   ├── config.rs               ← ClaudeAdapterConfig
    │   ├── process.rs              ← 子进程生命周期管理
    │   ├── protocol.rs             ← SDKMessage JSON 反序列化类型
    │   ├── mapping.rs              ← SDKMessage → RuntimeEvent 映射
    │   └── bridge.rs               ← JS bridge 脚本管理
    └── bridge/
        └── index.mjs              ← Node.js bridge 脚本（打包进 crate）
```

### 依赖关系

```
alva-types                          ← 基础类型（ContentBlock, ToolResult, MessageRole...）
    ↑
alva-engine-runtime                 ← EngineRuntime trait（依赖 alva-types 复用类型）
    ↑
alva-engine-adapter-claude          ← Claude 适配器（依赖 runtime trait）
    ↑
alva-app-core                           ← 消费者（通过 feature flag 选择适配器）
    ↑
alva-app                            ← UI 层
```

`alva-engine-runtime` 不依赖 `alva-core`。`alva-engine-adapter-claude` 也不依赖 `alva-core`。两者完全独立。

### workspace 变更

```toml
# Cargo.toml (workspace)
members = [
    # ... 现有 crates ...
    "crates/alva-engine-runtime",
    "crates/alva-engine-adapter-claude",
]
```

## alva-engine-runtime 设计

### EngineRuntime trait

> **命名说明**：不使用 `AgentRuntime`，因为 `alva-runtime` crate 已导出同名 struct `AgentRuntime`。
> 使用 `EngineRuntime` 与 crate 名 `alva-engine-runtime` 对齐，避免冲突。

```rust
use std::pin::Pin;
use futures_core::Stream;
use async_trait::async_trait;

/// 统一的 Agent 引擎运行时接口。
///
/// 所有引擎适配器实现此 trait。上层代码只依赖此接口，
/// 不感知具体引擎实现。
#[async_trait]
pub trait EngineRuntime: Send + Sync {
    /// 执行一次 Agent 会话，返回事件流。
    ///
    /// 返回 `Result` 以区分"引擎启动失败"（Err）和"执行中遇到错误"（流中的 Error 事件）。
    /// 返回的 Stream 是 'static 的，不借用 &self。
    /// 内部通过 channel 或 pipe 实现，调用者通过 StreamExt::next() 消费。
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError>;

    /// 取消当前正在执行的会话。
    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError>;

    /// 响应权限请求（仅部分引擎支持）。
    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError>;

    /// 查询引擎能力。
    fn capabilities(&self) -> RuntimeCapabilities;
}
```

### RuntimeEvent

```rust
/// 统一事件类型——所有引擎的事件映射到这里。
///
/// **终止语义**：`Completed` 是唯一的终止事件，无论成功或失败都会发出。
/// `Error` 用于中途可恢复错误或状态通知，不作为终止事件。
/// 引擎出错时，先发 `Error { recoverable: false }`，最后发 `Completed { result: None }`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeEvent {
    /// 会话已启动
    SessionStarted {
        session_id: String,
        model: Option<String>,
        tools: Vec<String>,
    },

    /// 完整的 Assistant 消息。
    ///
    /// **content 中不包含 ToolUse/ToolResult**——这些被提取为独立的
    /// ToolStart/ToolEnd 事件。content 只包含 Text 和 Reasoning 块。
    Message {
        id: String,
        role: MessageRole,
        content: Vec<ContentBlock>,
    },

    /// 流式增量（复用 alva_types::StreamEvent）
    MessageDelta {
        id: String,
        delta: StreamEvent,
    },

    /// 工具调用开始
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// 工具调用结束。
    ///
    /// **实现注意**：SDK 的 tool_result 只携带 tool_use_id，不含 tool_name。
    /// 适配器需维护 `HashMap<tool_use_id, tool_name>` 来填充 name 字段。
    ToolEnd {
        id: String,
        name: String,
        result: ToolResult,
        duration_ms: Option<u64>,
    },

    /// 需要用户授权
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: Option<String>,
    },

    /// 会话完成
    Completed {
        session_id: String,
        result: Option<String>,
        usage: Option<RuntimeUsage>,
    },

    /// 错误
    Error {
        message: String,
        recoverable: bool,
    },
}
```

### StreamDelta

> **不再定义独立的 StreamDelta 类型**。直接复用 `alva_types::StreamEvent`。
> 已在 `RuntimeEvent::MessageDelta` 中使用 `delta: StreamEvent`。
> 理由：`alva-engine-runtime` 已依赖 `alva-types`，且 `StreamEvent` 的 `Start`/`Done`/`Error`/`Usage`
> 变体在流中自然出现，不应人为裁剪。

### RuntimeRequest

```rust
pub struct RuntimeRequest {
    /// 用户 prompt
    pub prompt: String,

    /// 恢复已有会话（传入 session_id）
    pub resume_session: Option<String>,

    /// 系统 prompt
    pub system_prompt: Option<String>,

    /// 工作目录
    pub working_directory: Option<PathBuf>,

    /// 运行时选项
    pub options: RuntimeOptions,
}

pub struct RuntimeOptions {
    /// 是否启用流式
    pub streaming: bool,

    /// 最大轮数
    pub max_turns: Option<u32>,

    /// 引擎特有配置透传
    pub extra: HashMap<String, serde_json::Value>,
}
```

### PermissionDecision

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionDecision {
    /// 允许本次操作
    Allow {
        updated_input: Option<serde_json::Value>,
    },
    /// 拒绝本次操作
    Deny {
        message: String,
    },
}
```

### RuntimeUsage

```rust
/// 引擎级别的使用量统计。
///
/// token 字段使用 u32（与 alva_types::UsageMetadata 一致，u32 max ~4B 对当前模型足够）。
/// 额外包含 cost、duration、turns 等引擎级指标。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub num_turns: u32,
}
```

### RuntimeCapabilities

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    /// 支持流式输出
    pub streaming: bool,
    /// 支持外部控制工具执行（自建引擎 true，Claude/OpenClaw false）
    pub tool_control: bool,
    /// 支持交互式权限回调
    pub permission_callback: bool,
    /// 支持恢复会话
    pub resume: bool,
    /// 支持取消执行
    pub cancel: bool,
}
```

### RuntimeError

```rust
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Engine not ready: {0}")]
    NotReady(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Permission request not found: {0}")]
    PermissionNotFound(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}
```

### Dependencies (alva-engine-runtime)

```toml
[dependencies]
alva-types = { path = "../alva-types" }
async-trait = "0.1"
futures-core = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

## alva-engine-adapter-claude 设计

### 架构

```
ClaudeAdapter
    │
    ├── config: ClaudeAdapterConfig
    │     ├── node_path: Option<String>
    │     ├── sdk_package: Option<String>
    │     ├── api_key: Option<String>
    │     ├── model: Option<String>
    │     ├── permission_mode: PermissionMode
    │     ├── allowed_tools / disallowed_tools
    │     ├── mcp_servers: HashMap<String, McpServerConfig>
    │     └── env: HashMap<String, String>
    │
    ├── execute() 流程
    │     1. 确保 bridge 脚本已写入临时目录
    │     2. spawn Node.js 进程跑 bridge/index.mjs
    │     3. stdin writer task（发送 prompt + 控制消息）
    │     4. stdout reader task（读 JSON-line → 解析 SDKMessage）
    │     5. mapping: SDKMessage → RuntimeEvent
    │     6. 通过 channel 发送 RuntimeEvent
    │     7. 返回 ReceiverStream
    │
    └── 进程管理
          ├── cancel() → stdin 写 interrupt 消息
          ├── respond_permission() → stdin 写 permission response
          ├── stderr 监控（fatal pattern 检测）
          └── 进程退出处理
```

### ClaudeAdapterConfig

```rust
pub struct ClaudeAdapterConfig {
    /// Node.js 可执行文件路径（默认 "node"）
    pub node_path: Option<String>,

    /// @anthropic-ai/claude-agent-sdk 包路径
    /// 默认从 npm 全局或项目 node_modules 查找
    pub sdk_package_path: Option<String>,

    /// API Key（不设置则用环境变量 ANTHROPIC_API_KEY）
    pub api_key: Option<String>,

    /// 模型名称
    pub model: Option<String>,

    /// 权限模式
    pub permission_mode: PermissionMode,

    /// 预批准的工具列表
    pub allowed_tools: Vec<String>,

    /// 禁用的工具列表
    pub disallowed_tools: Vec<String>,

    /// 最大预算（USD）
    pub max_budget_usd: Option<f64>,

    /// MCP Server 配置
    pub mcp_servers: HashMap<String, serde_json::Value>,

    /// 额外环境变量
    pub env: HashMap<String, String>,
}

pub enum PermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
    Plan,
    DontAsk,
}
```

### Bridge 脚本（bridge/index.mjs）

一个小的 Node.js 脚本，充当 Rust 进程和 Claude Agent SDK 之间的桥梁：

```javascript
// bridge/index.mjs — 嵌入到 Rust crate 中，运行时写到临时目录
//
// 协议：
// - stdin: JSON-line 控制消息（prompt, permission_response, cancel）
// - stdout: JSON-line 事件（SDKMessage 原样透传 + 包装）
//
// Rust 进程 ←→ bridge/index.mjs ←→ Claude Agent SDK ←→ Claude Code 子进程

import { query } from "@anthropic-ai/claude-agent-sdk";
import { createInterface } from "readline";

const config = JSON.parse(process.argv[2] || "{}");
const rl = createInterface({ input: process.stdin });

// 发送事件到 Rust
function emit(type, data) {
  process.stdout.write(JSON.stringify({ type, ...data }) + "\n");
}

// 读取 stdin 控制消息
const pendingPermissions = new Map();
rl.on("line", (line) => {
  try {
    const msg = JSON.parse(line);
    if (msg.type === "permission_response") {
      const resolve = pendingPermissions.get(msg.request_id);
      if (resolve) {
        pendingPermissions.delete(msg.request_id);
        resolve(msg.decision);
      }
    }
    // cancel 等其他控制消息...
  } catch {}
});

// canUseTool 回调——桥接到 Rust 进程的 permission 机制
async function canUseTool(toolName, toolInput, { signal }) {
  const requestId = crypto.randomUUID();
  emit("permission_request", { request_id: requestId, tool_name: toolName, tool_input: toolInput });
  return new Promise((resolve) => {
    pendingPermissions.set(requestId, resolve);
    signal?.addEventListener("abort", () => {
      pendingPermissions.delete(requestId);
      resolve({ behavior: "deny", message: "Aborted" });
    });
  });
}

// 构建 SDK options
const options = {
  cwd: config.cwd,
  model: config.model,
  permissionMode: config.permission_mode || "default",
  allowedTools: config.allowed_tools || [],
  disallowedTools: config.disallowed_tools || [],
  includePartialMessages: true,
  maxBudgetUsd: config.max_budget_usd,
  mcpServers: config.mcp_servers || {},
  env: { ...process.env, ...config.env },
};

if (config.permission_mode === "default") {
  options.canUseTool = canUseTool;
}
if (config.api_key) {
  options.env.ANTHROPIC_API_KEY = config.api_key;
}
if (config.sdk_executable_path) {
  options.pathToClaudeCodeExecutable = config.sdk_executable_path;
}

// 主循环
try {
  const result = query({ prompt: config.prompt, options });
  for await (const message of result) {
    emit("sdk_message", { message });
  }
  emit("done", {});
} catch (err) {
  emit("error", { message: err.message });
}
```

**注意**：以上是简化伪代码，实际实现需要处理：
- streaming input（multi-turn 对话）
- session resume
- abort controller 集成
- 错误分类和 fatal pattern 检测

### SDKMessage 协议类型（protocol.rs）

```rust
/// Claude Agent SDK 消息的 Rust 反序列化类型。
/// 只定义我们关心的字段，其余忽略。
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeMessage {
    #[serde(rename = "sdk_message")]
    SdkMessage { message: SdkMessage },

    #[serde(rename = "permission_request")]
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: Value,
    },

    #[serde(rename = "done")]
    Done,

    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SdkMessage {
    #[serde(rename = "system")]
    System {
        subtype: String,
        session_id: Option<String>,
        model: Option<String>,
        tools: Option<Vec<String>>,
    },

    #[serde(rename = "assistant")]
    Assistant {
        uuid: String,
        session_id: String,
        message: SdkAssistantContent,
    },

    #[serde(rename = "stream_event")]
    StreamEvent {
        uuid: String,
        event: Value,  // BetaRawMessageStreamEvent — 解析关心的部分
    },

    #[serde(rename = "result")]
    Result {
        subtype: String,
        session_id: String,
        result: Option<String>,
        total_cost_usd: Option<f64>,
        duration_ms: Option<u64>,
        num_turns: Option<u32>,
        usage: Option<SdkUsage>,
    },

    /// 其他消息类型统一忽略
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct SdkAssistantContent {
    pub content: Option<Vec<SdkContentBlock>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SdkContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: Option<String>, is_error: Option<bool> },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct SdkUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
```

### 事件映射（mapping.rs）

```
BridgeMessage                      → RuntimeEvent
────────────────────────────────────────────────────────
SdkMessage::System(init)           → SessionStarted { session_id, model, tools }
SdkMessage::Assistant              → Message { id, role: Assistant, content }
SdkMessage::StreamEvent            → MessageDelta { id, delta }
  ├── content_block_delta(text)    →   StreamDelta::Text
  ├── content_block_delta(thinking)→   StreamDelta::Reasoning
  └── content_block_delta(input)   →   StreamDelta::ToolCallDelta
ContentBlock::ToolUse              → ToolStart { id, name, input }
ContentBlock::ToolResult           → ToolEnd { id, name, result }
SdkMessage::Result(success)        → Completed { session_id, result, usage }
SdkMessage::Result(error_*)        → Error { recoverable: false } + Completed { result: None }
BridgeMessage::PermissionRequest   → PermissionRequest { ... }
BridgeMessage::Error               → Error { recoverable: false } + Completed { result: None }
BridgeMessage::Done                → （已在 Result 映射中处理，Done 仅作为 bridge 进程退出信号）
```

### 进程管理（process.rs）

```rust
/// 管理 Node.js bridge 子进程的生命周期。
pub(crate) struct BridgeProcess {
    child: tokio::process::Child,
    stdin: tokio::io::BufWriter<tokio::process::ChildStdin>,
    stdout_lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
}

impl BridgeProcess {
    /// spawn Node.js bridge 进程
    pub async fn spawn(config: &BridgeSpawnConfig) -> Result<Self, RuntimeError>;

    /// 向 stdin 写入 JSON-line 控制消息
    pub async fn send(&mut self, msg: &BridgeOutbound) -> Result<(), RuntimeError>;

    /// 从 stdout 读取下一条 JSON-line 消息
    pub async fn recv(&mut self) -> Result<Option<BridgeMessage>, RuntimeError>;

    /// 优雅关闭：发送 shutdown 消息 → 等待 5 秒 → kill
    pub async fn shutdown(&mut self) -> Result<(), RuntimeError>;

    /// 强制终止进程
    pub async fn kill(&mut self) -> Result<(), RuntimeError>;
}
```

**stderr 监控**：spawn 时启动独立 tokio task 读取 stderr，匹配 fatal patterns
（`authentication_error`、`invalid_api_key`、`rate_limit` 等，参考 LobsterAI 的 STDERR_FATAL_PATTERNS）。
匹配到 fatal pattern 时触发 abort，通过 event channel 发出 `RuntimeEvent::Error { recoverable: false }`。
非 fatal 的 stderr 输出通过 `tracing::warn!` 记录。

**优雅关闭**：`shutdown()` 先向 stdin 发送 `{ "type": "shutdown" }` 消息，
bridge 脚本收到后调用 `query.close()`，等待 Claude Code 子进程退出。
如果 5 秒内未退出，fallback 到 `kill()`。

### bridge 脚本管理（bridge.rs）

```rust
/// 确保 bridge 脚本存在于用户级缓存目录中。
/// 脚本内容编译时嵌入（include_str!），运行时写出。
///
/// **安全**：使用 dirs::cache_dir() 而非 temp_dir() 避免多用户环境下的
/// 符号链接攻击风险。脚本写入前检查内容是否一致，避免不必要的 I/O。
/// **注意**：此函数包含同步 I/O，调用者应在 execute() 中通过
/// spawn_blocking 调用，避免阻塞 async runtime。
pub(crate) fn ensure_bridge_script() -> Result<PathBuf, RuntimeError> {
    let script_content = include_str!("../bridge/index.mjs");
    let base = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("alva-engine-claude-bridge");
    std::fs::create_dir_all(&base)?;
    let script_path = base.join("index.mjs");
    let needs_write = match std::fs::read_to_string(&script_path) {
        Ok(existing) => existing != script_content,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&script_path, script_content)?;
    }
    Ok(script_path)
}
```

### Dependencies (alva-engine-adapter-claude)

```toml
[dependencies]
alva-engine-runtime = { path = "../alva-engine-runtime" }
alva-types = { path = "../alva-types" }
async-trait = "0.1"
futures-core = "0.3"
tokio = { version = "1", features = ["process", "io-util", "sync", "rt", "macros"] }
tokio-stream = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }
dirs = "5"
```

## 不做的事

- **不实现 CLI 模式** — 后续由 `alva-engine-adapter-acp` 覆盖
- **不删 BaseAgent** — 等 `alva-engine-adapter-alva` 实现时再删
- **不改 alva-core** — 保持完全独立
- **不实现 session resume** — trait 预留了接口，adapter 第一版不实现
- **不实现 streaming input（multi-turn）** — 第一版单轮 prompt → result

## 未来扩展路径

```
alva-engine-runtime
├── alva-engine-adapter-claude      ← 本次实现
├── alva-engine-adapter-alva        ← 包装 alva-core，替代 BaseAgent
├── alva-engine-adapter-acp         ← ACP 标准协议，覆盖 CLI 场景 + 远程 Agent
└── alva-engine-adapter-openclaw    ← 等 OpenClaw 开源后实现
```
