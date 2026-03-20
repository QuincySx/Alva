# Sub-3: ACP 协议技术规格

> Crate: `srow-engine` (新增模块 `adapters/acp/`) | 依赖: Sub-2 | 被依赖: Sub-5

---

## 1. 目标与范围

Sub-3 实现 ACP（Agent Communication Protocol）协议层，让 Srow Agent 能以标准化方式接入外部 CLI Agent。协议设计 1:1 复刻 Wukong 对 ACP 的实现（逆向分析来源见 `REPORT-spark-acp.md`）。

**包含：**
- ACP 子进程管理（spawn / 生命周期 / 孤儿清理）
- Bootstrap payload 握手（workspace / authorized_roots / sandbox_level / model_config）
- ACP 完整消息类型（事件 / 内容块 / 生命周期 / 工具 / 权限 / 特殊）
- stdin/stdout JSON-line 管道（tokio async I/O）
- 权限请求拦截与回传（PermissionRequest → 用户 → PermissionData）
- ACP 消息持久化（SQLite）
- `AgentDelegate` trait — 让 Sub-2 引擎能将任务委托给外部 Agent
- 支持的外部 Agent：Claude Code / Qwen Code / Codex CLI / Gemini CLI / 通用 ACP

**不包含（后续子项目）：**
- Sub-5 编排层决策（"这个任务交给 Claude Code"——决策逻辑在 Sub-5）
- Sub-7 沙箱隔离（HITL 权限审批的 UI 弹窗在 Sub-7）
- 外部 Agent 本身的 LLM 调用（外部 Agent 自己负责）

---

## 2. 模块架构

```
srow-engine/src/adapters/acp/
├── mod.rs                      # pub use 统一导出
│
├── protocol/                   # ACP 协议类型（纯数据，零副作用）
│   ├── mod.rs
│   ├── bootstrap.rs            # BootstrapPayload — spawn 时写入 stdin
│   ├── message.rs              # AcpMessage — 所有消息类型 enum
│   ├── content.rs              # ContentBlock: TextBlock / ToolUseBlock / ToolResultBlock
│   ├── lifecycle.rs            # TaskStart / TaskComplete / SystemMessage / FinishData / ErrorData
│   ├── tool.rs                 # PreToolUse / PostToolUse / ToolCallData
│   ├── permission.rs           # PermissionRequest / PermissionData / PermissionOption
│   └── special.rs              # Plan / PingPong
│
├── process/                    # 子进程管理
│   ├── mod.rs
│   ├── manager.rs              # AcpProcessManager — 全局进程池
│   ├── handle.rs               # AcpProcessHandle — 单进程句柄
│   ├── discovery.rs            # Agent CLI 发现（PATH + 内置路径）
│   └── orphan.rs               # 孤儿进程清理（环境变量标记）
│
├── session/                    # ACP 会话（一个子进程对应一个或多个会话）
│   ├── mod.rs
│   ├── session.rs              # AcpSession — 会话状态机
│   └── permission_manager.rs   # 权限审批状态（allow_always / reject_always 缓存）
│
├── storage/                    # 持久化
│   ├── mod.rs
│   └── sqlite.rs               # acp_messages 表 CRUD
│
└── delegate.rs                 # AgentDelegate trait + AcpAgentDelegate 实现
```

### 分层依赖规则

```
protocol  ←  process  ←  session  ←  delegate
    ↑              ↑           ↑
  (纯类型)    (进程 I/O)   (业务逻辑)
```

- `protocol/`：无 tokio 依赖，只有 serde + 标准库
- `process/`：依赖 protocol，使用 tokio::process + tokio::io
- `session/`：依赖 protocol + process，维护会话状态机
- `delegate.rs`：实现 Sub-2 的 `AgentDelegate` trait，组合 session

---

## 3. 协议类型定义

### 3.1 Bootstrap Payload

Bootstrap 是子进程启动后，Wukong（Srow）通过 stdin 写入的第一条 JSON，相当于握手包。

```rust
// src/adapters/acp/protocol/bootstrap.rs

use serde::{Deserialize, Serialize};

/// sandbox_level 枚举
/// 与 Wukong 的 sandbox_level 字段保持一致
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    /// 无沙箱（开发阶段）
    None,
    /// 网络沙箱（允许文件，禁网络出入）
    Network,
    /// 完整沙箱（macOS sandbox-exec，Sub-7 启用）
    Full,
}

impl Default for SandboxLevel {
    fn default() -> Self { Self::None }
}

/// model_config — 告知外部 Agent 使用哪个模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,     // "anthropic" | "openai" | "google" | "alibaba"
    pub model: String,        // "claude-opus-4-5" | "qwen-coder-plus" 等
    pub api_key: String,
    /// 覆盖默认 base_url（代理 / 私有部署）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// 最大 token 数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// 完整 Bootstrap Payload
/// 由 Srow 在 spawn 后立即写入 stdin（一行 JSON，以 \n 结尾）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPayload {
    /// 工作目录（绝对路径）
    pub workspace: String,
    /// 允许访问的根路径列表（用于文件操作权限校验）
    pub authorized_roots: Vec<String>,
    /// 沙箱级别
    #[serde(default)]
    pub sandbox_level: SandboxLevel,
    /// 模型配置（外部 Agent 用它调 LLM）
    pub model_config: ModelConfig,
    /// 附件路径列表（可选，用于传递初始上下文文件）
    #[serde(default)]
    pub attachment_paths: Vec<String>,
    /// Srow 版本（供外部 Agent 做兼容判断）
    #[serde(default = "default_version")]
    pub srow_version: String,
}

fn default_version() -> String { env!("CARGO_PKG_VERSION").to_string() }
```

### 3.2 ACP 消息主类型

所有消息通过 `acp_event_type` 字段做 tag 区分，采用 externally-tagged JSON。

```rust
// src/adapters/acp/protocol/message.rs

use serde::{Deserialize, Serialize};
use super::{
    content::{ContentBlock},
    lifecycle::{TaskStartData, TaskCompleteData, SystemMessageData, FinishData, ErrorData},
    tool::{PreToolUseData, PostToolUseData, ToolCallData},
    permission::{PermissionRequest, PermissionData},
    special::{PlanData, PingPongData},
};

/// ACP 协议：外部 Agent → Srow（从 stdout 读取）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "acp_event_type", rename_all = "snake_case")]
pub enum AcpInboundMessage {
    /// 会话状态更新（通常含 ContentBlock 列表）
    SessionUpdate {
        session_id: String,
        #[serde(default)]
        content: Vec<ContentBlock>,
    },
    /// 单条消息内容更新（流式 delta）
    MessageUpdate {
        message_id: String,
        #[serde(default)]
        content: Vec<ContentBlock>,
    },
    /// 外部 Agent 请求权限（工具执行前）
    RequestPermission {
        request_id: String,
        data: PermissionRequest,
    },
    /// 任务开始
    TaskStart {
        data: TaskStartData,
    },
    /// 任务完成
    TaskComplete {
        data: TaskCompleteData,
    },
    /// 系统消息（日志 / 状态通知）
    SystemMessage {
        data: SystemMessageData,
    },
    /// 完成数据（含最终输出摘要）
    FinishData {
        data: FinishData,
    },
    /// 错误数据
    ErrorData {
        data: ErrorData,
    },
    /// 工具调用前通知
    PreToolUse {
        data: PreToolUseData,
    },
    /// 工具调用后通知
    PostToolUse {
        data: PostToolUseData,
    },
    /// 工具调用数据（完整参数）
    ToolCallData {
        data: ToolCallData,
    },
    /// Agent 执行计划（展示给用户的步骤列表）
    Plan {
        data: PlanData,
    },
    /// 心跳
    #[serde(rename = "ping")]
    PingPong {
        data: PingPongData,
    },
}

/// ACP 协议：Srow → 外部 Agent（写入 stdin）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpOutboundMessage {
    /// 用户 prompt（启动任务）
    Prompt {
        content: String,
        /// 可选：continuation prompt（恢复上次任务）
        #[serde(skip_serializing_if = "Option::is_none")]
        resume: Option<bool>,
    },
    /// 权限响应（回应 RequestPermission）
    PermissionResponse {
        request_id: String,
        data: PermissionData,
    },
    /// 取消当前任务
    Cancel,
    /// 关闭 Agent（优雅退出）
    Shutdown,
    /// 心跳响应
    #[serde(rename = "pong")]
    Pong {
        id: String,
    },
}
```

### 3.3 内容块

```rust
// src/adapters/acp/protocol/content.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// 文本输出（流式 delta 拼接）
    Text {
        text: String,
        /// true = 这是增量 delta；false = 完整内容
        #[serde(default)]
        is_delta: bool,
    },
    /// 工具调用请求
    ToolUse {
        id: String,
        name: String,
        /// 完整工具参数（JSON）
        input: Value,
    },
    /// 工具执行结果（由外部 Agent 的工具执行后回传）
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}
```

### 3.4 生命周期消息

```rust
// src/adapters/acp/protocol/lifecycle.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStartData {
    pub task_id: String,
    /// 人类可读的任务描述（Agent 自己生成）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompleteData {
    pub task_id: String,
    pub finish_reason: TaskFinishReason,
    /// Agent 生成的任务总结（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskFinishReason {
    Complete,
    Cancelled,
    Error,
    MaxIterations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessageData {
    pub level: SystemMessageLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemMessageLevel {
    Info,
    Warning,
    Error,
    Debug,
}

/// 最终完成数据（通常跟在 TaskComplete 之后）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishData {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorData {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recoverable: Option<bool>,
}
```

### 3.5 工具通知消息

```rust
// src/adapters/acp/protocol/tool.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具调用前通知（Srow 收到后可决定是否拦截）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseData {
    pub tool_call_id: String,
    pub tool_name: String,
    /// 完整工具输入参数
    pub input: Value,
}

/// 工具调用后通知（携带执行结果）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseData {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    #[serde(default)]
    pub is_error: bool,
    /// 执行耗时（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// 完整工具调用数据（含输入 + 输出，用于持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub output: String,
    #[serde(default)]
    pub is_error: bool,
}
```

### 3.6 权限消息

```rust
// src/adapters/acp/protocol/permission.rs

use serde::{Deserialize, Serialize};

/// 外部 Agent 发起的权限请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// 人类可读的说明（"我将要执行 rm -rf /tmp/xxx"）
    pub description: String,
    /// 危险级别
    pub risk_level: RiskLevel,
    /// 工具名
    pub tool_name: String,
    /// 工具参数摘要（展示给用户）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Srow 回传给外部 Agent 的权限响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionData {
    pub option: PermissionOption,
    /// 可选：拒绝原因（option = reject_* 时填写）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 权限选项（1:1 复刻 Wukong 的四选项）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOption {
    /// 本次允许
    AllowOnce,
    /// 永久允许（记入 session_approval_memory）
    AllowAlways,
    /// 本次拒绝
    RejectOnce,
    /// 永久拒绝（记入 session_approval_memory）
    RejectAlways,
}
```

### 3.7 特殊消息

```rust
// src/adapters/acp/protocol/special.rs

use serde::{Deserialize, Serialize};

/// Agent 执行计划（步骤列表，展示给用户）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanData {
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub index: u32,
    pub description: String,
    pub status: PlanStepStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

/// 心跳（双向）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingPongData {
    pub id: String,
    pub timestamp_ms: u64,
}
```

---

## 4. 进程管理

### 4.1 Agent CLI 发现策略

```rust
// src/adapters/acp/process/discovery.rs

use std::path::{Path, PathBuf};
use crate::adapters::acp::AcpError;

/// 支持的外部 Agent 类型
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalAgentKind {
    /// Claude Code (claude-code-acp)
    ClaudeCode,
    /// 通义千问 Qwen Code
    QwenCode,
    /// Codex CLI (Zed Industries)
    CodexCli,
    /// Gemini CLI
    GeminiCli,
    /// 通用 ACP（任意实现协议的 CLI，用户自定义命令）
    Generic { command: String },
}

/// 发现结果：完整的可执行命令和参数
#[derive(Debug, Clone)]
pub struct AgentCliCommand {
    pub kind: ExternalAgentKind,
    /// 可执行文件路径（绝对路径）
    pub executable: PathBuf,
    /// 附加参数（如 npx 的包名）
    pub args: Vec<String>,
}

pub struct AgentDiscovery;

impl AgentDiscovery {
    /// 发现指定 Agent 的 CLI 命令
    pub fn discover(kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        match kind {
            ExternalAgentKind::ClaudeCode => Self::discover_claude_code(),
            ExternalAgentKind::QwenCode => Self::discover_qwen_code(),
            ExternalAgentKind::CodexCli => Self::discover_codex_cli(),
            ExternalAgentKind::GeminiCli => Self::discover_gemini_cli(),
            ExternalAgentKind::Generic { command } => Self::discover_generic(command),
        }
    }

    /// Claude Code: PATH 查找 `claude-code-acp`
    fn discover_claude_code() -> Result<AgentCliCommand, AcpError> {
        let exe = which("claude-code-acp")
            .ok_or_else(|| AcpError::AgentNotFound {
                kind: "claude-code-acp".to_string(),
                hint: "Install Claude Code and ensure `claude-code-acp` is in $PATH".to_string(),
            })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::ClaudeCode,
            executable: exe,
            args: vec![],
        })
    }

    /// Qwen Code:
    ///   1. PATH 查找 `qwen`
    ///   2. 内置路径 $APP_DATA/packages/qwen/node_modules/.bin/qwen
    fn discover_qwen_code() -> Result<AgentCliCommand, AcpError> {
        // 优先 PATH
        if let Some(exe) = which("qwen") {
            return Ok(AgentCliCommand {
                kind: ExternalAgentKind::QwenCode,
                executable: exe,
                args: vec![],
            });
        }
        // 内置路径（macOS: ~/Library/Application Support/srow-agent/packages/qwen/...）
        let builtin = builtin_packages_dir().join("qwen").join("node_modules").join(".bin").join("qwen");
        if builtin.exists() {
            return Ok(AgentCliCommand {
                kind: ExternalAgentKind::QwenCode,
                executable: builtin,
                args: vec![],
            });
        }
        Err(AcpError::AgentNotFound {
            kind: "qwen".to_string(),
            hint: "Install Qwen Code CLI or place the package in the built-in packages directory".to_string(),
        })
    }

    /// Codex CLI:
    ///   1. PATH 查找 `codex-acp`
    ///   2. `npx @zed-industries/codex-acp` fallback
    fn discover_codex_cli() -> Result<AgentCliCommand, AcpError> {
        if let Some(exe) = which("codex-acp") {
            return Ok(AgentCliCommand {
                kind: ExternalAgentKind::CodexCli,
                executable: exe,
                args: vec![],
            });
        }
        // fallback: npx
        let npx = which("npx").ok_or_else(|| AcpError::AgentNotFound {
            kind: "codex-acp".to_string(),
            hint: "Install Node.js/npx or `codex-acp` binary in $PATH".to_string(),
        })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::CodexCli,
            executable: npx,
            args: vec!["@zed-industries/codex-acp".to_string()],
        })
    }

    /// Gemini CLI: PATH 查找 `gemini` 或 `gemini-cli`
    fn discover_gemini_cli() -> Result<AgentCliCommand, AcpError> {
        for name in &["gemini", "gemini-cli"] {
            if let Some(exe) = which(name) {
                return Ok(AgentCliCommand {
                    kind: ExternalAgentKind::GeminiCli,
                    executable: exe,
                    args: vec![],
                });
            }
        }
        Err(AcpError::AgentNotFound {
            kind: "gemini-cli".to_string(),
            hint: "Install Gemini CLI and ensure it is in $PATH".to_string(),
        })
    }

    /// 通用 ACP：直接使用用户指定的命令字符串
    fn discover_generic(command: &str) -> Result<AgentCliCommand, AcpError> {
        // 支持带空格的命令（e.g. "my-agent --acp-mode"）
        let mut parts = command.split_whitespace();
        let exe_str = parts.next().ok_or_else(|| AcpError::InvalidConfig("empty command".to_string()))?;
        let extra_args: Vec<String> = parts.map(str::to_string).collect();
        let exe = which(exe_str).ok_or_else(|| AcpError::AgentNotFound {
            kind: exe_str.to_string(),
            hint: format!("Ensure `{}` is in $PATH", exe_str),
        })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::Generic { command: command.to_string() },
            executable: exe,
            args: extra_args,
        })
    }
}

/// 在系统 PATH 中查找可执行文件
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths).find_map(|dir| {
                let full = dir.join(name);
                if full.is_file() { Some(full) } else { None }
            })
        })
}

/// 内置包目录（平台相关）
fn builtin_packages_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("srow-agent")
            .join("packages")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Temp"))
            .join("srow-agent")
            .join("packages")
    }
}
```

### 4.2 孤儿进程管理

参考 Wukong 的 `REWIND_PROCESS_MANAGER_MARKER` 环境变量方案：父进程将自己的 PID 注入子进程环境变量，子进程启动时检测父进程是否存活，若父进程已死则自我退出。

```rust
// src/adapters/acp/process/orphan.rs

/// 注入子进程的环境变量名（父进程 PID）
pub const SROW_PARENT_PID_ENV: &str = "SROW_PROCESS_MANAGER_PID";

/// 子进程自检：父进程是否还活着
/// 如果外部 Agent 实现此检查，可在 Srow 崩溃时自动退出
/// 此函数由 Srow 在 spawn 时注入父 PID 到环境变量，外部 Agent 自行实现轮询
pub fn parent_pid_env_value() -> String {
    std::process::id().to_string()
}

/// Srow 侧的孤儿清理：
/// 在 AcpProcessManager 启动时，扫描带有 SROW_PARENT_PID_ENV 标记的子进程，
/// 若其父 PID 与当前 Srow 进程 PID 不符（说明是上次崩溃留下的孤儿），则 kill。
///
/// 实现依赖平台：
///   macOS/Linux: 解析 /proc/{pid}/environ 或使用 sysctl
///   Windows: 使用 CreateToolhelp32Snapshot 枚举进程
///
/// 此函数在 AcpProcessManager::new() 中调用一次。
pub async fn cleanup_orphan_processes() {
    tracing::info!("scanning for orphan ACP processes...");
    // 实现占位：平台相关，Phase 1 可以 no-op，Phase 2 实现完整版
    // TODO: 枚举系统进程，找到带 SROW_PARENT_PID_ENV 且父 PID 不是当前进程的，发送 SIGTERM
}
```

### 4.3 进程句柄

```rust
// src/adapters/acp/process/handle.rs

use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::sync::{mpsc, Mutex};
use crate::adapters::acp::{
    protocol::{
        bootstrap::BootstrapPayload,
        message::{AcpInboundMessage, AcpOutboundMessage},
    },
    AcpError,
};

/// 子进程生命周期状态
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    /// 正在运行
    Running,
    /// 正常退出（exit code = 0）
    Exited,
    /// 异常退出（exit code != 0 或信号）
    Crashed { exit_code: Option<i32> },
    /// 正在重启（崩溃后自动重试）
    Restarting { attempt: u32 },
}

/// 单个 ACP 子进程的句柄
pub struct AcpProcessHandle {
    /// 进程 ID
    pub pid: u32,
    /// Agent 类型标识
    pub agent_kind: String,
    /// 工作目录
    pub workspace: PathBuf,
    /// 子进程状态
    state: Arc<Mutex<ProcessState>>,
    /// 向子进程发送消息的通道（对 stdin 的写端封装）
    stdin_tx: mpsc::Sender<AcpOutboundMessage>,
    /// 子进程发来的消息（从 stdout 读取，外层通过 subscribe 订阅）
    inbound_tx: mpsc::Sender<AcpInboundMessage>,
}

impl AcpProcessHandle {
    /// spawn 子进程，写入 bootstrap，启动读写 task
    pub async fn spawn(
        agent_cmd: &super::discovery::AgentCliCommand,
        bootstrap: BootstrapPayload,
        inbound_tx: mpsc::Sender<AcpInboundMessage>,
    ) -> Result<Self, AcpError> {
        use super::orphan::SROW_PARENT_PID_ENV;

        let mut cmd = tokio::process::Command::new(&agent_cmd.executable);
        cmd.args(&agent_cmd.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // 注入父 PID（孤儿检测）
            .env(SROW_PARENT_PID_ENV, super::orphan::parent_pid_env_value())
            // 注入工作目录
            .env("SROW_WORKSPACE", &bootstrap.workspace)
            // 禁用颜色转义序列（确保 stdout 是纯 JSON）
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");

        let mut child: Child = cmd.spawn().map_err(|e| AcpError::SpawnFailed {
            agent: agent_cmd.executable.display().to_string(),
            reason: e.to_string(),
        })?;

        let pid = child.id().unwrap_or(0);
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // 写入 bootstrap（一行 JSON）
        let bootstrap_json = serde_json::to_string(&bootstrap)
            .map_err(|e| AcpError::Serialization(e.to_string()))?;

        let mut writer = BufWriter::new(stdin);
        writer.write_all(bootstrap_json.as_bytes()).await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        writer.write_all(b"\n").await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        writer.flush().await
            .map_err(|e| AcpError::Io(e.to_string()))?;

        // 封装 stdin 写入 channel
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<AcpOutboundMessage>(64);
        let state = Arc::new(Mutex::new(ProcessState::Running));
        let state_clone = state.clone();

        // Task 1: stdin writer
        tokio::spawn(async move {
            while let Some(msg) = stdin_rx.recv().await {
                let line = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => { tracing::error!("acp serialize outbound: {e}"); continue; }
                };
                if writer.write_all(line.as_bytes()).await.is_err() { break; }
                if writer.write_all(b"\n").await.is_err() { break; }
                if writer.flush().await.is_err() { break; }
            }
        });

        // Task 2: stdout reader（逐行解析 AcpInboundMessage）
        let inbound_tx_clone = inbound_tx.clone();
        let state_for_reader = state.clone();
        tokio::spawn(async move {
            let mut lines: Lines<BufReader<ChildStdout>> = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() { continue; }
                match serde_json::from_str::<AcpInboundMessage>(&line) {
                    Ok(msg) => {
                        let _ = inbound_tx_clone.send(msg).await;
                    }
                    Err(e) => {
                        tracing::warn!("acp parse inbound: {e}, raw: {line}");
                    }
                }
            }
            // stdout closed = 进程退出
            *state_for_reader.lock().await = ProcessState::Exited;
        });

        // Task 3: stderr logger
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("[acp-stderr][pid={pid}] {line}");
            }
        });

        // Task 4: 进程 wait（检测崩溃）
        tokio::spawn(async move {
            let status = child.wait().await;
            let exit_code = status.ok().and_then(|s| s.code());
            let new_state = match exit_code {
                Some(0) => ProcessState::Exited,
                code => ProcessState::Crashed { exit_code: code },
            };
            *state_clone.lock().await = new_state;
        });

        Ok(Self {
            pid,
            agent_kind: format!("{:?}", agent_cmd.kind),
            workspace: PathBuf::from(&bootstrap.workspace),
            state,
            stdin_tx,
            inbound_tx,
        })
    }

    /// 获取当前进程状态
    pub async fn state(&self) -> ProcessState {
        self.state.lock().await.clone()
    }

    /// 向子进程发送消息（写入 stdin）
    pub async fn send(&self, msg: AcpOutboundMessage) -> Result<(), AcpError> {
        self.stdin_tx.send(msg).await
            .map_err(|_| AcpError::ProcessDead { pid: self.pid })
    }

    /// 优雅关闭
    pub async fn shutdown(&self) {
        let _ = self.send(AcpOutboundMessage::Shutdown).await;
    }
}
```

### 4.4 进程管理器

```rust
// src/adapters/acp/process/manager.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, broadcast};
use crate::adapters::acp::{
    protocol::{bootstrap::BootstrapPayload, message::AcpInboundMessage},
    process::{
        discovery::{AgentCliCommand, ExternalAgentKind},
        handle::{AcpProcessHandle, ProcessState},
    },
    AcpError,
};

/// 进程管理器配置
#[derive(Debug, Clone)]
pub struct ProcessManagerConfig {
    /// 崩溃后自动重启的最大次数
    pub max_restart_attempts: u32,
    /// 重启间隔（秒）
    pub restart_delay_secs: u64,
    /// 进程无响应超时（秒）——触发强制 kill
    pub heartbeat_timeout_secs: u64,
}

impl Default for ProcessManagerConfig {
    fn default() -> Self {
        Self {
            max_restart_attempts: 3,
            restart_delay_secs: 2,
            heartbeat_timeout_secs: 30,
        }
    }
}

/// 全局 ACP 进程管理器（单例，在 AppState 中持有）
pub struct AcpProcessManager {
    config: ProcessManagerConfig,
    /// process_id → handle
    processes: Arc<Mutex<HashMap<String, AcpProcessHandle>>>,
    /// 广播通道：所有进程的消息统一广播（session 通过 process_id 过滤）
    event_tx: broadcast::Sender<(String, AcpInboundMessage)>,
}

impl AcpProcessManager {
    pub async fn new(config: ProcessManagerConfig) -> Self {
        // 启动时清理孤儿进程
        super::orphan::cleanup_orphan_processes().await;

        let (event_tx, _) = broadcast::channel(1024);
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    /// 启动一个新的外部 Agent 子进程
    /// 返回 process_id（UUID），可用于后续 send / shutdown
    pub async fn spawn(
        &self,
        kind: ExternalAgentKind,
        bootstrap: BootstrapPayload,
    ) -> Result<String, AcpError> {
        let cmd = super::discovery::AgentDiscovery::discover(&kind)?;
        let process_id = uuid::Uuid::new_v4().to_string();

        // 创建消息路由 channel（process → 广播）
        let (inbound_tx, mut inbound_rx) = mpsc::channel::<AcpInboundMessage>(256);
        let event_tx = self.event_tx.clone();
        let pid_for_broadcast = process_id.clone();

        tokio::spawn(async move {
            while let Some(msg) = inbound_rx.recv().await {
                let _ = event_tx.send((pid_for_broadcast.clone(), msg));
            }
        });

        let handle = AcpProcessHandle::spawn(&cmd, bootstrap, inbound_tx).await?;

        tracing::info!(
            "acp process spawned: process_id={process_id} pid={} kind={:?}",
            handle.pid, kind
        );

        self.processes.lock().await.insert(process_id.clone(), handle);
        Ok(process_id)
    }

    /// 向指定进程发送消息
    pub async fn send(
        &self,
        process_id: &str,
        msg: crate::adapters::acp::protocol::message::AcpOutboundMessage,
    ) -> Result<(), AcpError> {
        let processes = self.processes.lock().await;
        let handle = processes.get(process_id)
            .ok_or_else(|| AcpError::ProcessNotFound(process_id.to_string()))?;
        handle.send(msg).await
    }

    /// 订阅指定进程的消息
    pub fn subscribe(&self, process_id: String) -> broadcast::Receiver<(String, AcpInboundMessage)> {
        // 调用方通过 filter 只处理匹配 process_id 的消息
        self.event_tx.subscribe()
    }

    /// 关闭并移除进程
    pub async fn shutdown(&self, process_id: &str) {
        let mut processes = self.processes.lock().await;
        if let Some(handle) = processes.remove(process_id) {
            handle.shutdown().await;
            tracing::info!("acp process shutdown: process_id={process_id}");
        }
    }

    /// 获取进程状态
    pub async fn process_state(&self, process_id: &str) -> Option<ProcessState> {
        let processes = self.processes.lock().await;
        match processes.get(process_id) {
            Some(h) => Some(h.state().await),
            None => None,
        }
    }

    /// 列出所有活跃进程
    pub async fn list_processes(&self) -> Vec<String> {
        self.processes.lock().await.keys().cloned().collect()
    }
}
```

---

## 5. ACP 会话层

### 5.1 会话状态机

```
                    AcpSession 状态机
                    ─────────────────

  Created
      │ bootstrap + spawn 成功
      ▼
  Ready ─────── send_prompt() ──────► Running
                                          │
                    ┌─────────────────────┤
                    │                     │
                    ▼                     ▼
              WaitingForPermission    TaskStart 到达
                    │
          用户 allow/reject
                    │
                    ▼
              Running（继续）
                    │
          TaskComplete 到达
                    │
                    ▼
              Completed / Cancelled / Error

  任意状态 ── process crashed ──► Crashed（可重启）
```

### 5.2 AcpSession

```rust
// src/adapters/acp/session/session.rs

use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex, oneshot};
use crate::adapters::acp::{
    protocol::{
        message::{AcpInboundMessage, AcpOutboundMessage},
        lifecycle::TaskFinishReason,
        permission::{PermissionRequest, PermissionData, PermissionOption},
    },
    process::manager::AcpProcessManager,
    session::permission_manager::PermissionManager,
    AcpError,
};
use crate::application::engine::EngineEvent;

#[derive(Debug, Clone, PartialEq)]
pub enum AcpSessionState {
    Ready,
    Running,
    WaitingForPermission { request_id: String },
    Completed,
    Cancelled,
    Error { message: String },
    Crashed,
}

/// 一次 ACP 交互会话（对应一个 prompt → response 周期）
pub struct AcpSession {
    pub session_id: String,
    /// 绑定的 ACP 子进程 ID
    pub process_id: String,
    pub state: Arc<Mutex<AcpSessionState>>,
    /// 待处理的权限请求（request_id → 回调 sender）
    pending_permissions: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<PermissionData>>>>,
    permission_manager: Arc<PermissionManager>,
    process_manager: Arc<AcpProcessManager>,
    /// 向 Sub-2 引擎事件总线转发的 sender
    engine_event_tx: mpsc::Sender<EngineEvent>,
}

impl AcpSession {
    pub fn new(
        session_id: String,
        process_id: String,
        permission_manager: Arc<PermissionManager>,
        process_manager: Arc<AcpProcessManager>,
        engine_event_tx: mpsc::Sender<EngineEvent>,
    ) -> Self {
        Self {
            session_id,
            process_id,
            state: Arc::new(Mutex::new(AcpSessionState::Ready)),
            pending_permissions: Arc::new(Mutex::new(Default::default())),
            permission_manager,
            process_manager,
            engine_event_tx,
        }
    }

    /// 发送 prompt 给外部 Agent，启动执行循环
    pub async fn send_prompt(&self, prompt: String, resume: bool) -> Result<(), AcpError> {
        *self.state.lock().await = AcpSessionState::Running;
        self.process_manager.send(
            &self.process_id,
            AcpOutboundMessage::Prompt { content: prompt, resume: if resume { Some(true) } else { None } },
        ).await
    }

    /// 取消当前任务
    pub async fn cancel(&self) -> Result<(), AcpError> {
        self.process_manager.send(&self.process_id, AcpOutboundMessage::Cancel).await
    }

    /// 处理来自外部 Agent 的入站消息
    /// 由 AcpProcessManager 的 subscribe 驱动（在独立 task 中运行）
    pub async fn handle_inbound(&self, msg: AcpInboundMessage) {
        match msg {
            AcpInboundMessage::TaskStart { data } => {
                tracing::debug!("acp task_start: task_id={}", data.task_id);
                *self.state.lock().await = AcpSessionState::Running;
            }

            AcpInboundMessage::TaskComplete { data } => {
                let new_state = match data.finish_reason {
                    TaskFinishReason::Complete => AcpSessionState::Completed,
                    TaskFinishReason::Cancelled => AcpSessionState::Cancelled,
                    TaskFinishReason::Error => AcpSessionState::Error {
                        message: data.summary.unwrap_or_else(|| "unknown error".to_string()),
                    },
                    TaskFinishReason::MaxIterations => AcpSessionState::Error {
                        message: "max iterations reached".to_string(),
                    },
                };
                *self.state.lock().await = new_state;
                let _ = self.engine_event_tx.send(EngineEvent::Completed {
                    session_id: self.session_id.clone(),
                }).await;
            }

            AcpInboundMessage::SessionUpdate { content, .. } |
            AcpInboundMessage::MessageUpdate { content, .. } => {
                for block in content {
                    self.forward_content_block(block).await;
                }
            }

            AcpInboundMessage::RequestPermission { request_id, data } => {
                self.handle_permission_request(request_id, data).await;
            }

            AcpInboundMessage::PreToolUse { data } => {
                let _ = self.engine_event_tx.send(EngineEvent::ToolCallStarted {
                    session_id: self.session_id.clone(),
                    tool_name: data.tool_name,
                    tool_call_id: data.tool_call_id,
                }).await;
            }

            AcpInboundMessage::PostToolUse { data } => {
                let _ = self.engine_event_tx.send(EngineEvent::ToolCallCompleted {
                    session_id: self.session_id.clone(),
                    tool_call_id: data.tool_call_id,
                    output: data.output,
                    is_error: data.is_error,
                }).await;
            }

            AcpInboundMessage::ErrorData { data } => {
                *self.state.lock().await = AcpSessionState::Error {
                    message: data.message.clone(),
                };
                let _ = self.engine_event_tx.send(EngineEvent::Error {
                    session_id: self.session_id.clone(),
                    error: data.message,
                }).await;
            }

            AcpInboundMessage::PingPong { data } => {
                // 回复 pong
                let _ = self.process_manager.send(
                    &self.process_id,
                    AcpOutboundMessage::Pong { id: data.id },
                ).await;
            }

            // Plan / SystemMessage / FinishData / ToolCallData — 记录日志或持久化，不影响状态机
            AcpInboundMessage::Plan { data } => {
                tracing::info!("acp plan: {} steps", data.steps.len());
            }
            AcpInboundMessage::SystemMessage { data } => {
                tracing::debug!("acp system[{:?}]: {}", data.level, data.message);
            }
            AcpInboundMessage::FinishData { data } => {
                tracing::debug!("acp finish_data: output_len={}", data.output.len());
            }
            AcpInboundMessage::ToolCallData { .. } => {
                // 持久化由 storage 层处理（在更上层注入）
            }
        }
    }

    /// 内容块 → EngineEvent 转发
    async fn forward_content_block(&self, block: crate::adapters::acp::protocol::content::ContentBlock) {
        use crate::adapters::acp::protocol::content::ContentBlock;
        match block {
            ContentBlock::Text { text, .. } => {
                let _ = self.engine_event_tx.send(EngineEvent::TextDelta {
                    session_id: self.session_id.clone(),
                    text,
                }).await;
            }
            ContentBlock::ToolUse { id, name, .. } => {
                let _ = self.engine_event_tx.send(EngineEvent::ToolCallStarted {
                    session_id: self.session_id.clone(),
                    tool_name: name,
                    tool_call_id: id,
                }).await;
            }
            ContentBlock::ToolResult { tool_use_id, content, is_error } => {
                let _ = self.engine_event_tx.send(EngineEvent::ToolCallCompleted {
                    session_id: self.session_id.clone(),
                    tool_call_id: tool_use_id,
                    output: content,
                    is_error,
                }).await;
            }
        }
    }

    /// 处理权限请求：先查缓存，命中则直接回传；未命中则等待 UI 回调
    async fn handle_permission_request(&self, request_id: String, req: PermissionRequest) {
        // 1. 查 allow_always / reject_always 缓存
        if let Some(cached) = self.permission_manager.check_cached(&req.tool_name).await {
            let _ = self.process_manager.send(
                &self.process_id,
                AcpOutboundMessage::PermissionResponse {
                    request_id: request_id.clone(),
                    data: cached,
                },
            ).await;
            return;
        }

        // 2. 更新会话状态 → WaitingForPermission
        *self.state.lock().await = AcpSessionState::WaitingForPermission {
            request_id: request_id.clone(),
        };

        // 3. 创建 oneshot channel 等待 UI 响应
        let (tx, rx) = oneshot::channel::<PermissionData>();
        self.pending_permissions.lock().await.insert(request_id.clone(), tx);

        // 4. 通知 UI 层（通过 EngineEvent::WaitingForHuman 复用，或新增专用事件）
        // Sub-7 会在这里弹出 HITL 权限弹窗
        let _ = self.engine_event_tx.send(EngineEvent::WaitingForHuman {
            session_id: self.session_id.clone(),
            question: format!(
                "[Permission Request] {}\nTool: {} | Risk: {:?}",
                req.description, req.tool_name, req.risk_level
            ),
            ask_id: request_id.clone(),
        }).await;

        // 5. 等待用户响应（异步挂起，不阻塞 tokio executor）
        let process_manager = self.process_manager.clone();
        let process_id = self.process_id.clone();
        let session_state = self.state.clone();
        let permission_manager = self.permission_manager.clone();
        let tool_name = req.tool_name.clone();

        tokio::spawn(async move {
            if let Ok(data) = rx.await {
                // 持久化 allow_always / reject_always 到缓存
                permission_manager.record(&tool_name, &data).await;

                // 回传 PermissionData 给外部 Agent
                let _ = process_manager.send(
                    &process_id,
                    AcpOutboundMessage::PermissionResponse { request_id, data },
                ).await;

                // 恢复 Running 状态
                *session_state.lock().await = AcpSessionState::Running;
            }
        });
    }

    /// UI 层调用：用户做出权限选择后，通过此方法回传
    pub async fn resolve_permission(&self, request_id: &str, data: PermissionData) -> Result<(), AcpError> {
        let mut pending = self.pending_permissions.lock().await;
        if let Some(tx) = pending.remove(request_id) {
            let _ = tx.send(data);
            Ok(())
        } else {
            Err(AcpError::PermissionRequestNotFound(request_id.to_string()))
        }
    }
}
```

### 5.3 权限管理器

```rust
// src/adapters/acp/session/permission_manager.rs

use std::collections::HashMap;
use tokio::sync::RwLock;
use crate::adapters::acp::protocol::permission::{PermissionData, PermissionOption};

/// 权限审批缓存（Session 级别，生命周期 = 进程生命周期）
/// allow_always / reject_always 的记录跨 prompt 有效，直到 Srow 重启
pub struct PermissionManager {
    /// tool_name → PermissionOption (仅存 AllowAlways / RejectAlways)
    cache: RwLock<HashMap<String, PermissionOption>>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self { cache: RwLock::new(HashMap::new()) }
    }

    /// 检查是否有缓存的 always 策略
    pub async fn check_cached(&self, tool_name: &str) -> Option<PermissionData> {
        let cache = self.cache.read().await;
        match cache.get(tool_name)? {
            PermissionOption::AllowAlways => Some(PermissionData {
                option: PermissionOption::AllowAlways,
                reason: None,
            }),
            PermissionOption::RejectAlways => Some(PermissionData {
                option: PermissionOption::RejectAlways,
                reason: Some("previously rejected always".to_string()),
            }),
            // AllowOnce / RejectOnce 不缓存
            _ => None,
        }
    }

    /// 记录用户选择（AllowAlways / RejectAlways 才持久化）
    pub async fn record(&self, tool_name: &str, data: &PermissionData) {
        match data.option {
            PermissionOption::AllowAlways | PermissionOption::RejectAlways => {
                self.cache.write().await.insert(tool_name.to_string(), data.option.clone());
            }
            _ => {}
        }
    }
}
```

---

## 6. AgentDelegate Trait

### 6.1 设计原则

ACP Agent 在 Sub-2 体系中有两种集成方式：

| 方式 | 实现 | 使用场景 |
|------|------|---------|
| **Tool trait** | `AcpDelegateTool` | 决策 Agent 通过 function call 委托任务 |
| **AgentDelegate trait** | `AcpAgentDelegate` | Sub-5 编排层直接驱动外部 Agent |

两者均需实现，优先实现 `AgentDelegate`，再用它包装为 `Tool`。

### 6.2 AgentDelegate Trait

```rust
// src/adapters/acp/delegate.rs

use async_trait::async_trait;
use crate::{
    application::engine::EngineEvent,
    error::EngineError,
};
use tokio::sync::mpsc;

/// 委托执行结果
#[derive(Debug, Clone)]
pub struct DelegateResult {
    /// 外部 Agent 的最终文本输出（拼接所有 TextBlock）
    pub output: String,
    /// 完成原因
    pub finish_reason: DelegateFinishReason,
    /// 外部 Agent 执行期间产生的工具调用摘要（可选）
    pub tool_calls_summary: Vec<DelegateToolCallSummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DelegateFinishReason {
    Complete,
    Cancelled,
    Error { message: String },
    ProcessCrashed,
}

#[derive(Debug, Clone)]
pub struct DelegateToolCallSummary {
    pub tool_name: String,
    pub is_error: bool,
}

/// AgentDelegate — Sub-5 编排层通过此 trait 驱动外部 Agent
/// 实现者负责创建和管理 ACP 进程生命周期
#[async_trait]
pub trait AgentDelegate: Send + Sync {
    /// 委托方标识（"claude-code" / "qwen-code" 等）
    fn agent_kind(&self) -> &str;

    /// 执行任务：
    ///   - 启动（或复用已有的）ACP 子进程
    ///   - 发送 prompt
    ///   - 等待 TaskComplete
    ///   - 返回汇总结果
    ///
    /// `event_tx` 用于将执行过程中的 EngineEvent 转发给 UI 层
    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<DelegateResult, EngineError>;

    /// 取消正在执行的委托
    async fn cancel(&self) -> Result<(), EngineError>;
}
```

### 6.3 AcpAgentDelegate 实现

```rust
/// ACP 协议的 AgentDelegate 具体实现
pub struct AcpAgentDelegate {
    kind: super::process::discovery::ExternalAgentKind,
    model_config: super::protocol::bootstrap::ModelConfig,
    process_manager: std::sync::Arc<super::process::manager::AcpProcessManager>,
    permission_manager: std::sync::Arc<super::session::permission_manager::PermissionManager>,
    /// 当前活跃的 session（任意时刻只有一个）
    current_process_id: tokio::sync::Mutex<Option<String>>,
}

impl AcpAgentDelegate {
    pub fn new(
        kind: super::process::discovery::ExternalAgentKind,
        model_config: super::protocol::bootstrap::ModelConfig,
        process_manager: std::sync::Arc<super::process::manager::AcpProcessManager>,
    ) -> Self {
        Self {
            kind,
            model_config,
            process_manager,
            permission_manager: std::sync::Arc::new(
                super::session::permission_manager::PermissionManager::new()
            ),
            current_process_id: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl AgentDelegate for AcpAgentDelegate {
    fn agent_kind(&self) -> &str {
        match &self.kind {
            super::process::discovery::ExternalAgentKind::ClaudeCode => "claude-code",
            super::process::discovery::ExternalAgentKind::QwenCode => "qwen-code",
            super::process::discovery::ExternalAgentKind::CodexCli => "codex-cli",
            super::process::discovery::ExternalAgentKind::GeminiCli => "gemini-cli",
            super::process::discovery::ExternalAgentKind::Generic { command } => command.as_str(),
        }
    }

    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<DelegateResult, EngineError> {
        use super::protocol::{
            bootstrap::BootstrapPayload,
            message::AcpInboundMessage,
            lifecycle::TaskFinishReason,
        };
        use super::session::session::{AcpSession, AcpSessionState};

        // 1. 构造 Bootstrap payload
        let bootstrap = BootstrapPayload {
            workspace: workspace.to_string_lossy().to_string(),
            authorized_roots: vec![workspace.to_string_lossy().to_string()],
            sandbox_level: Default::default(),
            model_config: self.model_config.clone(),
            attachment_paths: vec![],
            srow_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        // 2. spawn 子进程
        let process_id = self.process_manager.spawn(self.kind.clone(), bootstrap).await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        *self.current_process_id.lock().await = Some(process_id.clone());

        // 3. 创建会话
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = AcpSession::new(
            session_id.clone(),
            process_id.clone(),
            self.permission_manager.clone(),
            self.process_manager.clone(),
            event_tx.clone(),
        );

        // 4. 订阅进程消息
        let mut rx = self.process_manager.subscribe(process_id.clone());

        // 5. 发送 prompt
        session.send_prompt(prompt, false).await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        // 6. 驱动消息循环，直到 TaskComplete / Error / Crash
        let mut output_buffer = String::new();
        let mut tool_calls_summary = vec![];

        loop {
            match tokio::time::timeout(
                std::time::Duration::from_secs(300), // 5 分钟超时
                rx.recv(),
            ).await {
                Ok(Ok((pid, msg))) if pid == process_id => {
                    // 收集输出
                    if let AcpInboundMessage::SessionUpdate { ref content, .. } |
                           AcpInboundMessage::MessageUpdate { ref content, .. } = msg {
                        for block in content {
                            if let super::protocol::content::ContentBlock::Text { text, .. } = block {
                                output_buffer.push_str(text);
                            }
                        }
                    }
                    if let AcpInboundMessage::PostToolUse { ref data } = msg {
                        tool_calls_summary.push(DelegateToolCallSummary {
                            tool_name: data.tool_name.clone(),
                            is_error: data.is_error,
                        });
                    }

                    // 处理 session 状态变更
                    session.handle_inbound(msg.clone()).await;

                    // 检查终止条件
                    match session.state.lock().await.clone() {
                        AcpSessionState::Completed => {
                            self.process_manager.shutdown(&process_id).await;
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::Complete,
                                tool_calls_summary,
                            });
                        }
                        AcpSessionState::Cancelled => {
                            self.process_manager.shutdown(&process_id).await;
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::Cancelled,
                                tool_calls_summary,
                            });
                        }
                        AcpSessionState::Error { message } => {
                            self.process_manager.shutdown(&process_id).await;
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::Error { message },
                                tool_calls_summary,
                            });
                        }
                        AcpSessionState::Crashed => {
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::ProcessCrashed,
                                tool_calls_summary,
                            });
                        }
                        _ => {}  // 继续等待
                    }
                }
                Ok(Ok(_)) => continue,  // 其他进程的消息，忽略
                Ok(Err(_)) => {
                    // broadcast channel lagged — 跳过
                    continue;
                }
                Err(_) => {
                    // 超时
                    self.process_manager.shutdown(&process_id).await;
                    return Err(EngineError::ToolExecution(
                        "ACP delegate timeout (300s)".to_string()
                    ));
                }
            }
        }
    }

    async fn cancel(&self) -> Result<(), EngineError> {
        if let Some(pid) = self.current_process_id.lock().await.as_deref() {
            self.process_manager.send(
                pid,
                super::protocol::message::AcpOutboundMessage::Cancel,
            ).await.map_err(|e| EngineError::ToolExecution(e.to_string()))
        } else {
            Ok(())
        }
    }
}
```

### 6.4 AcpDelegateTool（包装为 Tool trait）

这使得决策 Agent 可以通过 function calling 的方式调用外部 Agent：

```rust
// src/adapters/acp/delegate.rs（续）

use crate::ports::tool::{Tool, ToolContext};
use crate::domain::tool::{ToolDefinition, ToolResult};

/// 将 AcpAgentDelegate 包装为 Tool trait，
/// 让 Sub-2 的 AgentEngine 能通过 tool_call 机制调用外部 Agent。
///
/// function calling schema:
/// {
///   "name": "delegate_to_claude_code",
///   "description": "将编码任务委托给 Claude Code 执行",
///   "parameters": {
///     "type": "object",
///     "required": ["task"],
///     "properties": {
///       "task": { "type": "string", "description": "完整的任务描述" }
///     }
///   }
/// }
pub struct AcpDelegateTool {
    delegate: std::sync::Arc<dyn AgentDelegate>,
    /// 工具名（如 "delegate_to_claude_code"）
    tool_name: String,
    /// 工具描述（展示给决策 Agent 的 LLM）
    description: String,
    /// 事件转发 sender（从 ToolContext 或外部注入）
    event_tx: mpsc::Sender<EngineEvent>,
}

impl AcpDelegateTool {
    pub fn new(
        delegate: std::sync::Arc<dyn AgentDelegate>,
        tool_name: impl Into<String>,
        description: impl Into<String>,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Self {
        Self {
            delegate,
            tool_name: tool_name.into(),
            description: description.into(),
            event_tx,
        }
    }
}

#[async_trait::async_trait]
impl Tool for AcpDelegateTool {
    fn name(&self) -> &str { &self.tool_name }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.tool_name.clone(),
            description: self.description.clone(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["task"],
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Complete task description for the external agent"
                    }
                }
            }),
        }
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, EngineError> {
        let task = input["task"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'task' field".to_string()))?
            .to_string();

        let result = self.delegate.delegate(
            task,
            ctx.workspace.clone(),
            self.event_tx.clone(),
        ).await?;

        let output = match result.finish_reason {
            DelegateFinishReason::Complete => result.output,
            DelegateFinishReason::Cancelled => format!("[Cancelled]\n{}", result.output),
            DelegateFinishReason::Error { ref message } => {
                format!("[Error: {}]\n{}", message, result.output)
            }
            DelegateFinishReason::ProcessCrashed => {
                format!("[Process Crashed]\n{}", result.output)
            }
        };

        let is_error = !matches!(result.finish_reason, DelegateFinishReason::Complete);

        Ok(ToolResult {
            tool_call_id: uuid::Uuid::new_v4().to_string(),
            tool_name: self.tool_name.clone(),
            output,
            is_error,
            duration_ms: 0,  // AcpAgentDelegate 内部计时
            created_at: chrono::Utc::now(),
        })
    }
}
```

---

## 7. 持久化层

### 7.1 SQLite Schema

```sql
-- migrations/002_acp.sql

-- ACP 消息持久化（1:1 复刻 Wukong 的 acp_messages 表）
CREATE TABLE IF NOT EXISTS acp_messages (
    id                  TEXT PRIMARY KEY,
    conversation_id     TEXT NOT NULL,  -- 对应 AcpSession.session_id
    process_id          TEXT NOT NULL,  -- 对应 AcpProcessHandle.pid（字符串形式）
    message_type        TEXT NOT NULL,  -- "acp_tool_call" | "acp_permission" | "plan" | "system" | "error" | "finish"
    content             TEXT NOT NULL,  -- JSON 序列化的消息内容
    timestamp           INTEGER NOT NULL,  -- Unix 毫秒时间戳
    created_at          TEXT NOT NULL   -- ISO 8601
);

CREATE INDEX IF NOT EXISTS idx_acp_messages_conversation_ts
    ON acp_messages(conversation_id, timestamp);

CREATE INDEX IF NOT EXISTS idx_acp_messages_type
    ON acp_messages(message_type);

-- 时间戳规范化触发器（确保 timestamp 单调递增，复刻 Wukong）
CREATE TRIGGER IF NOT EXISTS acp_messages_ts_normalize_insert
AFTER INSERT ON acp_messages
BEGIN
    UPDATE acp_messages
    SET timestamp = MAX(
        NEW.timestamp,
        COALESCE((
            SELECT MAX(timestamp) FROM acp_messages
            WHERE conversation_id = NEW.conversation_id
        ), 0) + 1
    )
    WHERE id = NEW.id
      AND timestamp <= COALESCE((
        SELECT MAX(timestamp) FROM acp_messages
        WHERE conversation_id = NEW.conversation_id AND id != NEW.id
      ), -1);
END;

-- ACP 进程记录（用于崩溃恢复分析）
CREATE TABLE IF NOT EXISTS acp_processes (
    process_id          TEXT PRIMARY KEY,
    agent_kind          TEXT NOT NULL,
    workspace           TEXT NOT NULL,
    pid                 INTEGER,
    state               TEXT NOT NULL DEFAULT 'running',
    started_at          TEXT NOT NULL,
    ended_at            TEXT
);
```

### 7.2 存储接口

```rust
// src/adapters/acp/storage/sqlite.rs

use crate::adapters::acp::protocol::message::AcpInboundMessage;
use crate::adapters::acp::AcpError;

pub struct AcpSqliteStorage {
    pool: tokio_rusqlite::Connection,
}

impl AcpSqliteStorage {
    pub async fn open(path: &std::path::Path) -> Result<Self, AcpError> {
        let conn = tokio_rusqlite::Connection::open(path).await
            .map_err(|e| AcpError::Storage(e.to_string()))?;
        // 运行 migration 002_acp.sql
        Ok(Self { pool: conn })
    }

    /// 记录 ACP 消息（工具调用 / 权限 / 错误 / 完成）
    pub async fn record_message(
        &self,
        conversation_id: &str,
        process_id: &str,
        message_type: &str,
        content: &serde_json::Value,
    ) -> Result<(), AcpError> {
        let id = uuid::Uuid::new_v4().to_string();
        let content_str = serde_json::to_string(content)
            .map_err(|e| AcpError::Serialization(e.to_string()))?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let now_str = chrono::Utc::now().to_rfc3339();

        let cid = conversation_id.to_string();
        let pid = process_id.to_string();
        let mtype = message_type.to_string();

        self.pool.call(move |conn| {
            conn.execute(
                "INSERT INTO acp_messages (id, conversation_id, process_id, message_type, content, timestamp, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, cid, pid, mtype, content_str, now_ms, now_str],
            )?;
            Ok(())
        }).await.map_err(|e| AcpError::Storage(e.to_string()))
    }

    /// 查询会话的历史消息
    pub async fn get_messages(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, AcpError> {
        let cid = conversation_id.to_string();
        self.pool.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT content FROM acp_messages WHERE conversation_id = ?1 ORDER BY timestamp ASC"
            )?;
            let rows: Vec<String> = stmt.query_map(
                rusqlite::params![cid],
                |row| row.get(0),
            )?.collect::<Result<_, _>>()?;
            Ok(rows)
        }).await
            .map_err(|e| AcpError::Storage(e.to_string()))?
            .into_iter()
            .map(|s| serde_json::from_str(&s).map_err(|e| AcpError::Serialization(e.to_string())))
            .collect()
    }
}
```

---

## 8. 错误类型

```rust
// src/adapters/acp/mod.rs（或单独的 error.rs）

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("Agent CLI not found: {kind} — {hint}")]
    AgentNotFound { kind: String, hint: String },

    #[error("Failed to spawn agent process '{agent}': {reason}")]
    SpawnFailed { agent: String, reason: String },

    #[error("Process {pid} is no longer alive")]
    ProcessDead { pid: u32 },

    #[error("Process '{0}' not found in manager")]
    ProcessNotFound(String),

    #[error("Permission request '{0}' not found (already resolved or expired)")]
    PermissionRequestNotFound(String),

    #[error("Invalid ACP configuration: {0}")]
    InvalidConfig(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Protocol error: {0}")]
    Protocol(String),
}

// 允许 AcpError 转换为 EngineError
impl From<AcpError> for crate::error::EngineError {
    fn from(e: AcpError) -> Self {
        crate::error::EngineError::ToolExecution(e.to_string())
    }
}
```

---

## 9. 与 Sub-2 的集成

### 9.1 集成方案

Sub-2 的 `EngineBuilder` 支持通过 `with_tool()` 注入任何实现 `Tool` trait 的对象。Sub-3 通过 `AcpDelegateTool` 无缝接入：

```rust
// 示例：在决策 Agent 的 EngineBuilder 中注册外部 Agent 委托工具

let process_manager = Arc::new(AcpProcessManager::new(ProcessManagerConfig::default()).await);

let claude_delegate = Arc::new(AcpAgentDelegate::new(
    ExternalAgentKind::ClaudeCode,
    ModelConfig {
        provider: "anthropic".to_string(),
        model: "claude-opus-4-5".to_string(),
        api_key: std::env::var("ANTHROPIC_API_KEY").unwrap(),
        base_url: None,
        max_tokens: Some(8192),
    },
    process_manager.clone(),
));

let engine = EngineBuilder::new(agent_config)
    .with_llm(AnthropicProvider::new(&api_key, "claude-opus-4-5"))
    // 注册 Claude Code 委托工具
    .with_tool(AcpDelegateTool::new(
        claude_delegate,
        "delegate_to_claude_code",
        "Delegate a coding task to Claude Code. Use when the task requires deep code editing, \
         refactoring, or complex file operations. Input: complete task description.",
        event_tx.clone(),
    ))
    .with_default_sqlite_storage().await?
    .build(event_tx, cancel_rx)?;
```

### 9.2 决策 Agent 的 System Prompt 提示

为让决策 Agent 正确使用委托工具，system prompt 需要包含使用指引：

```
可用的外部 Agent 委托工具：

- delegate_to_claude_code: 适用于代码编写、重构、文件批量修改等编码任务。
  Claude Code 有独立的文件系统访问能力和代码执行能力。
  使用时提供完整的任务描述，包括目标、约束条件和预期输出格式。

- delegate_to_qwen_code: 适用于需要通义千问处理的编码任务。

注意：
1. 委托任务后等待完成，不要并发委托多个任务给同一个 Agent。
2. 委托任务的描述要足够详细，外部 Agent 没有你的上下文。
3. 若委托失败（finish_reason = error/crashed），尝试重新描述任务或换其他 Agent。
```

### 9.3 数据流图

```
用户 prompt
    │
    ▼
决策 Agent (AgentEngine + LLM)
    │
    │  tool_call: delegate_to_claude_code
    │  { "task": "重构 src/main.rs，..." }
    ▼
AcpDelegateTool.execute()
    │
    ├── AcpAgentDelegate.delegate()
    │       │
    │       ├── AcpProcessManager.spawn()
    │       │       │
    │       │       ├── AgentDiscovery.discover(ClaudeCode)
    │       │       ├── tokio::process::Command::spawn()
    │       │       └── stdin ← BootstrapPayload (JSON)
    │       │
    │       ├── AcpSession.send_prompt()
    │       │       └── stdin ← { "type": "prompt", "content": "..." }
    │       │
    │       └── 消息循环（broadcast::Receiver）
    │               │
    │               │  stdout → AcpInboundMessage
    │               │
    │               ├── TextBlock → EngineEvent::TextDelta → UI
    │               ├── PreToolUse → EngineEvent::ToolCallStarted → UI
    │               ├── RequestPermission → EngineEvent::WaitingForHuman → UI
    │               │       └── 用户选择 → stdin ← PermissionData
    │               ├── PostToolUse → EngineEvent::ToolCallCompleted → UI
    │               └── TaskComplete → DelegateResult
    │
    └── ToolResult { output: "重构完成...", is_error: false }
            │
            ▼
    决策 Agent 继续循环（可能再调其他工具或直接回答用户）
```

---

## 10. 支持的外部 Agent 配置清单

| Agent | 启动命令 | 发现策略 | 备注 |
|-------|---------|---------|------|
| **Claude Code** | `claude-code-acp` | PATH 查找 | 需用户安装 Claude Code |
| **Qwen Code** | `qwen` (或 `node .../.bin/qwen`) | PATH 查找 → 内置路径 | 支持 App 内置包 |
| **Codex CLI** | `codex-acp` 或 `npx @zed-industries/codex-acp` | PATH 查找 → npx fallback | 需 Node.js 环境 |
| **Gemini CLI** | `gemini` 或 `gemini-cli` | PATH 查找（两个名字都尝试） | 需用户安装 Gemini CLI |
| **通用 ACP** | 用户自定义命令字符串 | 直接 PATH 查找 | 任意实现 ACP 协议的 CLI |

---

## 11. Cargo.toml 新增依赖

在 `srow-engine/Cargo.toml` 中追加（Sub-3 专用，其余依赖已在 Sub-2 中定义）：

```toml
[dependencies]
# 已有（Sub-2）：tokio, serde, serde_json, uuid, chrono, thiserror, tracing, tokio-rusqlite

# Sub-3 新增
dirs = "5"       # 跨平台应用数据目录（内置包路径）
```

`tokio::process` 已包含在 `tokio = { features = ["full"] }` 中，无需额外添加。

---

## 12. 模块公开 API（acp/mod.rs 导出）

```rust
// src/adapters/acp/mod.rs

pub mod protocol;
pub mod process;
pub mod session;
pub mod storage;
pub mod delegate;

// 错误类型
mod error;
pub use error::AcpError;

// 核心公开类型
pub use protocol::bootstrap::{BootstrapPayload, ModelConfig, SandboxLevel};
pub use protocol::message::{AcpInboundMessage, AcpOutboundMessage};
pub use protocol::permission::{PermissionData, PermissionOption, PermissionRequest, RiskLevel};

pub use process::discovery::ExternalAgentKind;
pub use process::handle::ProcessState;
pub use process::manager::{AcpProcessManager, ProcessManagerConfig};

pub use session::session::{AcpSession, AcpSessionState};
pub use session::permission_manager::PermissionManager;

pub use delegate::{AgentDelegate, AcpAgentDelegate, AcpDelegateTool, DelegateResult, DelegateFinishReason};
```

---

## 13. 测试策略

### 13.1 单元测试

| 测试文件 | 测试内容 |
|---------|---------|
| `protocol/message.rs` | `AcpInboundMessage` / `AcpOutboundMessage` 的 JSON 序列化/反序列化（涵盖所有变体） |
| `protocol/bootstrap.rs` | `BootstrapPayload` 序列化，`SandboxLevel` 枚举值 |
| `protocol/permission.rs` | `PermissionOption` 四选项序列化 |
| `session/permission_manager.rs` | `check_cached` 命中/未命中，`record` 持久化逻辑 |
| `process/discovery.rs` | Mock PATH 环境变量，验证各 Agent 的发现逻辑和 fallback |

```rust
// tests/acp_protocol_serde.rs

#[test]
fn test_inbound_task_complete_serde() {
    let json = r#"{
        "acp_event_type": "task_complete",
        "data": {
            "task_id": "t-001",
            "finish_reason": "complete",
            "summary": "Done"
        }
    }"#;
    let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, AcpInboundMessage::TaskComplete { .. }));
}

#[test]
fn test_outbound_permission_response_serde() {
    let msg = AcpOutboundMessage::PermissionResponse {
        request_id: "req-001".to_string(),
        data: PermissionData { option: PermissionOption::AllowOnce, reason: None },
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("allow_once"));
}
```

### 13.2 集成测试（Echo Agent）

使用一个简单的 shell 脚本或 Rust 二进制模拟 ACP 外部 Agent（"echo agent"），验证完整握手流程：

```rust
// tests/acp_integration.rs

/// 模拟 ACP Agent：读取 bootstrap → 发送 task_start → echo prompt → task_complete
/// 可以用 Python/Shell 脚本实现，放在 tests/fixtures/echo_agent.py
#[tokio::test]
async fn test_acp_full_lifecycle_with_echo_agent() {
    // 1. 启动 echo agent（tests/fixtures/echo_agent.sh）
    // 2. 发送 bootstrap + prompt
    // 3. 验证收到 TaskStart → TextBlock（echo）→ TaskComplete
    // 4. 验证 DelegateResult.output == prompt
}

#[tokio::test]
async fn test_permission_request_allow_once() {
    // 1. echo agent 在收到 prompt 后发送 RequestPermission
    // 2. 验证 AcpSession 状态变为 WaitingForPermission
    // 3. 调用 resolve_permission(AllowOnce)
    // 4. 验证 PermissionData 被回传，agent 继续执行
}
```

### 13.3 压力测试

```rust
#[tokio::test]
async fn test_concurrent_acp_processes() {
    // 同时 spawn 5 个 echo agent，验证进程池和广播路由正确隔离消息
}
```

---

## 14. 实现优先级与里程碑

| 里程碑 | 内容 | 验收标准 |
|--------|------|---------|
| M1 | 协议类型定义（`protocol/` 全部模块） | 所有消息类型 serde 往返测试通过 |
| M2 | `AcpProcessHandle` — spawn + JSON-line 读写 | Echo Agent 集成测试：bootstrap + prompt + task_complete |
| M3 | `AcpProcessManager` — 进程池 + 广播路由 | 并发 3 个 echo agent，消息不串台 |
| M4 | `AcpSession` — 状态机 + 权限拦截 | 权限请求 → 用户选择 → 回传 → 继续执行 |
| M5 | `AgentDelegate` + `AcpDelegateTool` | Claude Code 真实接入：一次完整编码委托 |
| M6 | `AcpSqliteStorage` — 消息持久化 | 工具调用 / 权限记录写入 DB，可查询历史 |
| M7 | Qwen Code / Codex CLI / Gemini CLI 接入 | 各 Agent 独立 E2E 测试 |
| M8 | 孤儿进程清理 | 模拟 Srow 崩溃重启，验证孤儿子进程被 kill |

---

## 15. 参考资料

- Wukong 逆向分析（ACP 协议完整分析）: `../../../smallraw-skills/docs/dump/Wukong.analysis/REPORT-spark-acp.md`
- Sub-2 Agent 引擎规格（trait 定义 / EngineEvent / ToolRegistry）: `./sub-2-agent-engine.md`
- 总体架构: `../ARCHITECTURE.md`
