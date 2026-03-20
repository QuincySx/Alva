# Sub-2: Agent 引擎技术规格

> Crate: `srow-engine` | 依赖: 无 | 被依赖: Sub-3, Sub-4, Sub-5

---

## 1. 目标与范围

Sub-2 实现 Srow Agent 的核心引擎层。所有后续子项目（ACP 协议、Skill 系统、编排层）都以本 crate 为基础。

**包含：**
- Agent 引擎核心循环（prompt → LLM → tool_call → 执行 → 循环）
- LLM 适配层（基于 rig，支持 OpenAI / Claude / Gemini / DeepSeek）
- 内置工具集（execute_shell, create_file, file_edit, grep_search, list_files, ask_human）
- 上下文管理（token 计数 + 上下文压缩）
- 会话管理（Session 创建 / 恢复 / 持久化，SQLite）
- 消息模型（完整 Rust 类型定义）

**不包含（后续子项目）：**
- MCP 集成（Sub-4）
- Skill 系统（Sub-4）
- ACP 协议 / 外部 Agent 接入（Sub-3）
- 浏览器自动化（Sub-6）
- Agent 编排层（Sub-5）
- 沙箱隔离 / HITL（Sub-7）

---

## 2. 模块架构

参考 AllSpark 的 DDD 分层，但合并为单 crate，通过 Rust 模块边界代替 crate 边界，避免过早拆分的构建负担。

```
srow-engine/
├── Cargo.toml
└── src/
    ├── lib.rs                          # pub use 统一导出
    │
    ├── domain/                         # 领域模型：纯类型，零依赖
    │   ├── mod.rs
    │   ├── message.rs                  # LLMMessage, LLMContent, Role
    │   ├── tool.rs                     # ToolCall, ToolResult, ToolDefinition
    │   ├── session.rs                  # Session, SessionStatus
    │   └── agent.rs                    # AgentConfig, AgentState
    │
    ├── application/                    # 应用服务：业务逻辑编排
    │   ├── mod.rs
    │   ├── engine.rs                   # AgentEngine — 核心循环主体
    │   ├── context_manager.rs          # 上下文管理 + 压缩
    │   └── session_service.rs          # Session CRUD 服务
    │
    ├── ports/                          # 端口：对外抽象 trait
    │   ├── mod.rs
    │   ├── llm_provider.rs             # LLMProvider trait
    │   ├── tool.rs                     # Tool trait
    │   └── storage.rs                  # SessionStorage trait
    │
    ├── adapters/                       # 适配器：trait 具体实现
    │   ├── mod.rs
    │   ├── llm/
    │   │   ├── mod.rs
    │   │   ├── openai.rs               # rig OpenAI 适配
    │   │   ├── anthropic.rs            # rig Claude 适配
    │   │   ├── gemini.rs               # rig Gemini 适配
    │   │   └── deepseek.rs             # rig DeepSeek 适配（兼容 OpenAI API）
    │   ├── tools/
    │   │   ├── mod.rs
    │   │   ├── execute_shell.rs
    │   │   ├── create_file.rs
    │   │   ├── file_edit.rs
    │   │   ├── grep_search.rs
    │   │   ├── list_files.rs
    │   │   └── ask_human.rs
    │   └── storage/
    │       ├── mod.rs
    │       └── sqlite.rs               # SQLite 实现 SessionStorage
    │
    └── error.rs                        # 统一错误类型
```

### 分层依赖规则

```
domain ← ports ← application ← adapters
```

- `domain`：无任何外部依赖，只有标准库和 serde
- `ports`：依赖 domain，定义 async trait
- `application`：依赖 domain + ports，不依赖 adapters
- `adapters`：依赖 ports + domain，实现具体 I/O

---

## 3. 核心 Trait 定义

### 3.1 LLMProvider

```rust
// src/ports/llm_provider.rs

use crate::domain::message::{LLMMessage, LLMContent};
use crate::domain::tool::{ToolDefinition, ToolCall};
use crate::error::EngineError;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// LLM 调用结果（非流式）
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Vec<LLMContent>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// LLM 正常结束，无工具调用
    EndTurn,
    /// LLM 要求执行工具
    ToolUse,
    /// 达到 max_tokens
    MaxTokens,
    /// 上游停止信号
    StopSequence,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
}

/// 流式 chunk 事件
#[derive(Debug, Clone)]
pub enum StreamChunk {
    TextDelta(String),
    ToolCallDelta { id: String, name: String, input_delta: String },
    Done(LLMResponse),
}

/// LLM 请求参数
#[derive(Debug, Clone)]
pub struct LLMRequest {
    pub messages: Vec<LLMMessage>,
    pub tools: Vec<ToolDefinition>,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub model_override: Option<String>,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// 模型标识符（用于 token 计数器选择）
    fn model_id(&self) -> &str;

    /// 非流式调用
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, EngineError>;

    /// 流式调用，通过 channel 发送 chunk
    async fn complete_stream(
        &self,
        request: LLMRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError>;

    /// 估算 token 数（本地快速估算，不精确，用于裁剪决策）
    fn estimate_tokens(&self, messages: &[LLMMessage]) -> u32;
}
```

### 3.2 Tool

```rust
// src/ports/tool.rs

use crate::domain::tool::{ToolCall, ToolResult, ToolDefinition};
use crate::error::EngineError;
use async_trait::async_trait;
use serde_json::Value;

/// 工具执行上下文（注入引擎级信息）
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// 当前 session id
    pub session_id: String,
    /// 工作目录（workspace 根路径）
    pub workspace: std::path::PathBuf,
    /// 是否允许危险操作（沙箱外执行）— Sub-7 会收紧
    pub allow_dangerous: bool,
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名，对应 ToolCall.name
    fn name(&self) -> &str;

    /// OpenAI/Claude function calling schema（JSON Schema）
    fn definition(&self) -> ToolDefinition;

    /// 执行工具，返回 JSON 格式结果
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, EngineError>;
}

/// 工具注册表 — 引擎初始化时注入
pub struct ToolRegistry {
    tools: std::collections::HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Default::default() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }
}
```

### 3.3 SessionStorage

```rust
// src/ports/storage.rs

use crate::domain::session::Session;
use crate::domain::message::LLMMessage;
use crate::domain::tool::{ToolCall, ToolResult};
use crate::error::EngineError;
use async_trait::async_trait;

#[async_trait]
pub trait SessionStorage: Send + Sync {
    // Session 生命周期
    async fn create_session(&self, session: &Session) -> Result<(), EngineError>;
    async fn get_session(&self, id: &str) -> Result<Option<Session>, EngineError>;
    async fn update_session_status(
        &self,
        id: &str,
        status: crate::domain::session::SessionStatus,
    ) -> Result<(), EngineError>;
    async fn list_sessions(&self, workspace: &str) -> Result<Vec<Session>, EngineError>;
    async fn delete_session(&self, id: &str) -> Result<(), EngineError>;

    // 消息持久化
    async fn append_message(&self, session_id: &str, msg: &LLMMessage) -> Result<(), EngineError>;
    async fn get_messages(&self, session_id: &str) -> Result<Vec<LLMMessage>, EngineError>;

    // 工具调用记录
    async fn record_tool_call(&self, session_id: &str, call: &ToolCall) -> Result<(), EngineError>;
    async fn record_tool_result(
        &self,
        session_id: &str,
        result: &ToolResult,
    ) -> Result<(), EngineError>;
    async fn get_tool_calls(&self, session_id: &str) -> Result<Vec<ToolCall>, EngineError>;
}
```

---

## 4. 领域类型（Domain Types）

### 4.1 消息模型

```rust
// src/domain/message.rs

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::domain::tool::{ToolCall, ToolResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 消息内容块（对应 Anthropic Content Block 模型）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        /// 已解析的 JSON 参数
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Image {
        /// base64 或 URL
        source: ImageSource,
        media_type: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageSource {
    Base64(String),
    Url(String),
}

/// 单条消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    pub id: String,          // UUID
    pub role: Role,
    pub content: Vec<LLMContent>,
    pub created_at: DateTime<Utc>,
    /// 所属轮次（同一 turn 的多条消息 turn_index 相同）
    pub turn_index: u32,
    /// token 数（可选，LLM 返回后填充）
    pub token_count: Option<u32>,
}

impl LLMMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            content: vec![LLMContent::Text { text: text.into() }],
            created_at: Utc::now(),
            turn_index: 0,
            token_count: None,
        }
    }

    pub fn assistant(content: Vec<LLMContent>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content,
            created_at: Utc::now(),
            turn_index: 0,
            token_count: None,
        }
    }

    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>, is_error: bool) -> Self {
        let id_str = tool_use_id.into();
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Tool,
            content: vec![LLMContent::ToolResult {
                tool_use_id: id_str,
                content: content.into(),
                is_error,
            }],
            created_at: Utc::now(),
            turn_index: 0,
            token_count: None,
        }
    }
}
```

### 4.2 工具模型

```rust
// src/domain/tool.rs

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// 工具调用（从 LLM 响应中解析）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// 工具执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    pub is_error: bool,
    pub duration_ms: u64,
    pub created_at: DateTime<Utc>,
}

/// 工具定义（用于 function calling schema）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing parameters
    pub parameters: serde_json::Value,
}
```

### 4.3 会话模型

```rust
// src/domain/session.rs

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// 空闲，等待输入
    Idle,
    /// 正在运行 Agent 循环
    Running,
    /// 等待用户输入（ask_human 工具）
    WaitingForHuman,
    /// 成功完成
    Completed,
    /// 已取消
    Cancelled,
    /// 出错停止
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workspace: String,
    /// Agent 配置快照（创建时固化）
    pub agent_config_snapshot: serde_json::Value,
    pub status: SessionStatus,
    /// 当前已用 token（累计）
    pub total_tokens: u32,
    /// 执行轮次计数
    pub iteration_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 4.4 Agent 配置

```rust
// src/domain/agent.rs

use serde::{Deserialize, Serialize};

/// LLM 提供商标识
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LLMProviderKind {
    OpenAI,
    Anthropic,
    Gemini,
    DeepSeek,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    pub provider: LLMProviderKind,
    pub model: String,
    pub api_key: String,
    /// 覆盖默认 base_url（用于代理 / DeepSeek）
    pub base_url: Option<String>,
    pub max_tokens: u32,
    /// 是否启用 extended thinking（Claude only）
    pub thinking: bool,
    pub temperature: Option<f32>,
}

/// Agent 实例配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub llm: LLMConfig,
    pub workspace: std::path::PathBuf,
    /// 允许使用的工具名列表（None = 使用全部注册工具）
    pub allowed_tools: Option<Vec<String>>,
    /// 最大循环轮次，防止无限循环
    pub max_iterations: u32,
    /// 触发上下文压缩的 token 阈值（0 = 不压缩）
    pub compaction_threshold: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Agent".to_string(),
            system_prompt: String::new(),
            llm: LLMConfig {
                provider: LLMProviderKind::Anthropic,
                model: "claude-opus-4-5".to_string(),
                api_key: String::new(),
                base_url: None,
                max_tokens: 8192,
                thinking: false,
                temperature: None,
            },
            workspace: std::path::PathBuf::from("."),
            allowed_tools: None,
            max_iterations: 50,
            compaction_threshold: 150_000,
        }
    }
}
```

---

## 5. Agent 引擎核心循环

### 5.1 状态机

```
                    ┌─────────────────────────────────────────┐
                    │           AgentEngine                    │
                    └─────────────────────────────────────────┘

  Start
    │
    ▼
┌─────────────┐
│  Idle       │◄──────────────────────────────────┐
└──────┬──────┘                                    │
       │ run(prompt)                               │
       ▼                                           │
┌─────────────┐                                    │
│  Preparing  │  组装消息上下文                      │
│             │  system + history + user + tool_results│
└──────┬──────┘                                    │
       │                                           │
       ▼                                           │
┌─────────────┐  ContextManager 检查 token 数       │
│  Compacting │──────────────────────────────────► │（仅超阈值时进入）
│  (可选)     │                                    │
└──────┬──────┘                                    │
       │ / token 数正常，直接跳过                    │
       ▼                                           │
┌─────────────┐                                    │
│  Calling    │  调用 LLMProvider::complete_stream  │
│  LLM        │                                    │
└──────┬──────┘                                    │
       │                                           │
       ├── StopReason::EndTurn ──────────────────► Completed
       │
       ├── StopReason::MaxTokens ───────────────► Error(MaxTokens)
       │
       └── StopReason::ToolUse
                    │
                    ▼
           ┌─────────────┐
           │  Executing  │  并发执行所有 ToolCall
           │  Tools      │  （ask_human 工具会挂起到 WaitingForHuman）
           └──────┬──────┘
                  │ 所有工具完成
                  │
                  ├── iteration_count >= max_iterations ──► Error(MaxIterations)
                  │
                  └── 继续 ──────────────────────────────► Preparing (下一轮)

  外部信号:
    cancel() ──────────────────────────────────────────► Cancelled
```

### 5.2 AgentEngine 实现

```rust
// src/application/engine.rs

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, watch};
use crate::{
    domain::{
        agent::AgentConfig,
        message::{LLMMessage, LLMContent, Role},
        session::{Session, SessionStatus},
        tool::ToolCall,
    },
    ports::{
        llm_provider::{LLMProvider, LLMRequest, StopReason, StreamChunk},
        tool::{Tool, ToolRegistry, ToolContext},
        storage::SessionStorage,
    },
    application::{
        context_manager::ContextManager,
        session_service::SessionService,
    },
    error::EngineError,
};

/// 引擎向外广播的事件（UI 层订阅）
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// 流式文本 delta
    TextDelta { session_id: String, text: String },
    /// 工具调用开始
    ToolCallStarted { session_id: String, tool_name: String, tool_call_id: String },
    /// 工具调用完成
    ToolCallCompleted { session_id: String, tool_call_id: String, output: String, is_error: bool },
    /// ask_human：引擎等待用户输入
    WaitingForHuman { session_id: String, question: String, ask_id: String },
    /// 循环完成
    Completed { session_id: String },
    /// 出错
    Error { session_id: String, error: String },
    /// token 使用量更新
    TokenUsage { session_id: String, input: u32, output: u32, total: u32 },
}

/// 驱动单个 Session 的 Agent 循环
pub struct AgentEngine {
    config: AgentConfig,
    llm: Arc<dyn LLMProvider>,
    tools: Arc<ToolRegistry>,
    storage: Arc<dyn SessionStorage>,
    context_manager: ContextManager,
    /// 外部可 subscribe 的事件流
    event_tx: mpsc::Sender<EngineEvent>,
    /// 取消信号
    cancel_rx: watch::Receiver<bool>,
}

impl AgentEngine {
    pub fn new(
        config: AgentConfig,
        llm: Arc<dyn LLMProvider>,
        tools: Arc<ToolRegistry>,
        storage: Arc<dyn SessionStorage>,
        event_tx: mpsc::Sender<EngineEvent>,
        cancel_rx: watch::Receiver<bool>,
    ) -> Self {
        let context_manager = ContextManager::new(config.compaction_threshold);
        Self { config, llm, tools, storage, context_manager, event_tx, cancel_rx }
    }

    /// 启动/恢复 Agent 循环
    /// session_id: 已由 SessionService 创建
    /// initial_message: 当前轮次用户输入
    pub async fn run(
        &mut self,
        session_id: &str,
        initial_message: LLMMessage,
    ) -> Result<(), EngineError> {
        // 1. 更新 session 状态 → Running
        self.storage.update_session_status(session_id, SessionStatus::Running).await?;
        self.storage.append_message(session_id, &initial_message).await?;

        // 2. 加载历史消息
        let mut history = self.storage.get_messages(session_id).await?;

        let tool_ctx = ToolContext {
            session_id: session_id.to_string(),
            workspace: self.config.workspace.clone(),
            allow_dangerous: false,
        };

        let mut iteration = 0u32;

        loop {
            // 取消检查
            if *self.cancel_rx.borrow() {
                self.storage.update_session_status(session_id, SessionStatus::Cancelled).await?;
                let _ = self.event_tx.send(EngineEvent::Completed { session_id: session_id.to_string() }).await;
                return Ok(());
            }

            if iteration >= self.config.max_iterations {
                let _ = self.storage.update_session_status(session_id, SessionStatus::Error).await;
                let _ = self.event_tx.send(EngineEvent::Error {
                    session_id: session_id.to_string(),
                    error: format!("max iterations ({}) reached", self.config.max_iterations),
                }).await;
                return Err(EngineError::MaxIterationsReached(self.config.max_iterations));
            }

            // 3. 上下文压缩（超阈值时）
            if self.context_manager.needs_compaction(&history, &self.config.system_prompt) {
                history = self.context_manager.compact(
                    history,
                    &self.config.system_prompt,
                    self.llm.as_ref(),
                ).await?;
            }

            // 4. 获取当前可用工具定义（按 allowed_tools 过滤）
            let tool_defs = self.filtered_tool_definitions();

            // 5. 构建 LLM 请求
            let request = LLMRequest {
                messages: history.clone(),
                tools: tool_defs,
                system: Some(self.config.system_prompt.clone()),
                max_tokens: self.config.llm.max_tokens,
                temperature: self.config.llm.temperature,
                model_override: None,
            };

            // 6. 流式调用 LLM
            let (chunk_tx, mut chunk_rx) = mpsc::channel(256);
            let llm = self.llm.clone();
            let req = request.clone();
            tokio::spawn(async move {
                let _ = llm.complete_stream(req, chunk_tx).await;
            });

            let mut llm_response = None;
            while let Some(chunk) = chunk_rx.recv().await {
                match chunk {
                    StreamChunk::TextDelta(text) => {
                        let _ = self.event_tx.send(EngineEvent::TextDelta {
                            session_id: session_id.to_string(),
                            text,
                        }).await;
                    }
                    StreamChunk::ToolCallDelta { .. } => { /* 累积处理，忽略 delta */ }
                    StreamChunk::Done(resp) => {
                        // 更新 token 统计
                        let _ = self.event_tx.send(EngineEvent::TokenUsage {
                            session_id: session_id.to_string(),
                            input: resp.usage.input_tokens,
                            output: resp.usage.output_tokens,
                            total: resp.usage.input_tokens + resp.usage.output_tokens,
                        }).await;
                        llm_response = Some(resp);
                    }
                }
            }

            let response = llm_response.ok_or(EngineError::LLMStreamInterrupted)?;

            // 7. 将 assistant 消息入历史 + 持久化
            let assistant_msg = LLMMessage::assistant(response.content.clone());
            self.storage.append_message(session_id, &assistant_msg).await?;
            history.push(assistant_msg);

            // 8. 判断停止原因
            match response.stop_reason {
                StopReason::EndTurn | StopReason::StopSequence => {
                    self.storage.update_session_status(session_id, SessionStatus::Completed).await?;
                    let _ = self.event_tx.send(EngineEvent::Completed {
                        session_id: session_id.to_string(),
                    }).await;
                    return Ok(());
                }
                StopReason::MaxTokens => {
                    self.storage.update_session_status(session_id, SessionStatus::Error).await?;
                    let _ = self.event_tx.send(EngineEvent::Error {
                        session_id: session_id.to_string(),
                        error: "max_tokens reached".to_string(),
                    }).await;
                    return Err(EngineError::MaxTokensReached);
                }
                StopReason::ToolUse => {
                    // 9. 提取所有 ToolCall 并并发执行
                    let tool_calls: Vec<ToolCall> = response.content.iter()
                        .filter_map(|c| {
                            if let LLMContent::ToolUse { id, name, input } = c {
                                Some(ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                    created_at: chrono::Utc::now(),
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    let tool_results = self.execute_tools_parallel(
                        &tool_calls,
                        &tool_ctx,
                        session_id,
                    ).await?;

                    // 10. 将 tool_result 消息加入历史
                    for result in &tool_results {
                        let msg = LLMMessage::tool_result(
                            &result.tool_call_id,
                            &result.output,
                            result.is_error,
                        );
                        self.storage.append_message(session_id, &msg).await?;
                        history.push(msg);
                    }

                    iteration += 1;
                    // → 继续循环
                }
            }
        }
    }

    /// 并发执行所有工具（ask_human 串行等待）
    async fn execute_tools_parallel(
        &self,
        calls: &[ToolCall],
        ctx: &ToolContext,
        session_id: &str,
    ) -> Result<Vec<crate::domain::tool::ToolResult>, EngineError> {
        use futures::future::join_all;
        use std::time::Instant;

        let futures: Vec<_> = calls.iter().map(|call| {
            let tools = self.tools.clone();
            let ctx = ctx.clone();
            let event_tx = self.event_tx.clone();
            let call = call.clone();
            let sid = session_id.to_string();

            async move {
                let _ = event_tx.send(EngineEvent::ToolCallStarted {
                    session_id: sid.clone(),
                    tool_name: call.name.clone(),
                    tool_call_id: call.id.clone(),
                }).await;

                let start = Instant::now();
                let result = match tools.get(&call.name) {
                    Some(tool) => tool.execute(call.input.clone(), &ctx).await,
                    None => Err(EngineError::ToolNotFound(call.name.clone())),
                };

                let duration_ms = start.elapsed().as_millis() as u64;

                let tool_result = match result {
                    Ok(r) => r,
                    Err(e) => crate::domain::tool::ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        output: format!("Error: {e}"),
                        is_error: true,
                        duration_ms,
                        created_at: chrono::Utc::now(),
                    },
                };

                let _ = event_tx.send(EngineEvent::ToolCallCompleted {
                    session_id: sid,
                    tool_call_id: tool_result.tool_call_id.clone(),
                    output: tool_result.output.clone(),
                    is_error: tool_result.is_error,
                }).await;

                tool_result
            }
        }).collect();

        Ok(join_all(futures).await)
    }

    fn filtered_tool_definitions(&self) -> Vec<crate::domain::tool::ToolDefinition> {
        match &self.config.allowed_tools {
            None => self.tools.definitions(),
            Some(allowed) => self.tools.definitions()
                .into_iter()
                .filter(|d| allowed.contains(&d.name))
                .collect(),
        }
    }
}
```

---

## 6. 上下文管理（ContextManager）

### 6.1 设计目标

参考 AllSpark 的 `compaction.rs`，在 token 超过阈值时进行历史消息压缩，保留关键信息的同时缩减上下文长度。

### 6.2 接口定义

```rust
// src/application/context_manager.rs

use crate::{
    domain::message::LLMMessage,
    ports::llm_provider::LLMProvider,
    error::EngineError,
};

pub struct ContextManager {
    /// 触发压缩的 token 阈值
    threshold: u32,
    /// 压缩后保留最近消息数量（sliding window）
    keep_recent: usize,
}

impl ContextManager {
    pub fn new(threshold: u32) -> Self {
        Self { threshold, keep_recent: 20 }
    }

    /// 估算当前上下文 token 数，判断是否需要压缩
    pub fn needs_compaction(&self, history: &[LLMMessage], system: &str) -> bool {
        if self.threshold == 0 { return false; }
        let estimated: u32 = history.iter()
            .filter_map(|m| m.token_count)
            .sum::<u32>()
            + (system.len() / 4) as u32;  // 粗估 4 char/token
        estimated >= self.threshold
    }

    /// 执行压缩：
    /// 策略 A（简单）: 截取最近 keep_recent 条消息，丢弃早期历史
    /// 策略 B（摘要）: 调用 LLM 对早期历史生成摘要，注入为 system message
    ///
    /// 当前实现策略 A，后续可升级为 B
    pub async fn compact(
        &self,
        history: Vec<LLMMessage>,
        _system: &str,
        _llm: &dyn LLMProvider,
    ) -> Result<Vec<LLMMessage>, EngineError> {
        if history.len() <= self.keep_recent {
            return Ok(history);
        }
        // 策略 A：保留最近 N 条，保证首条为 user role（不破坏 role 交替规则）
        let start = history.len().saturating_sub(self.keep_recent);
        let mut truncated = history[start..].to_vec();

        // 确保第一条是 user 消息（LLM API 要求）
        while !truncated.is_empty()
            && truncated[0].role != crate::domain::message::Role::User
        {
            truncated.remove(0);
        }

        Ok(truncated)
    }
}
```

---

## 7. 内置工具集

所有工具实现 `Tool` trait，定义在 `src/adapters/tools/`。

### 7.1 工具一览

| 工具名 | 危险级别 | 说明 |
|--------|---------|------|
| `execute_shell` | HIGH | 执行 shell 命令（Sub-7 会加沙箱） |
| `create_file` | MEDIUM | 创建或覆写文件 |
| `file_edit` | MEDIUM | 对文件做 str_replace / insert / delete 编辑 |
| `grep_search` | LOW | 正则搜索文件内容 |
| `list_files` | LOW | 列出目录文件树 |
| `ask_human` | LOW | 向用户提问并等待回答 |

### 7.2 execute_shell

```rust
// src/adapters/tools/execute_shell.rs

/// 输入 schema
#[derive(Debug, Deserialize)]
pub struct ExecuteShellInput {
    pub command: String,
    /// 执行目录，缺省为 workspace
    pub cwd: Option<String>,
    /// 超时秒数，缺省 30
    pub timeout_secs: Option<u64>,
}

/// 输出
#[derive(Debug, Serialize)]
pub struct ExecuteShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}
```

**参数 JSON Schema（写入 ToolDefinition.parameters）：**
```json
{
  "type": "object",
  "required": ["command"],
  "properties": {
    "command": { "type": "string", "description": "Shell command to execute" },
    "cwd": { "type": "string", "description": "Working directory, defaults to workspace root" },
    "timeout_secs": { "type": "integer", "description": "Timeout in seconds, default 30" }
  }
}
```

### 7.3 create_file

```rust
#[derive(Debug, Deserialize)]
pub struct CreateFileInput {
    /// 相对于 workspace 的路径
    pub path: String,
    pub content: String,
    /// 父目录不存在时是否自动创建，缺省 true
    pub create_dirs: Option<bool>,
}
```

### 7.4 file_edit

参考 Claude Code 的 str_replace_based_edit_tool 设计：

```rust
#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum FileEditInput {
    StrReplace {
        path: String,
        old_str: String,
        new_str: String,
    },
    Insert {
        path: String,
        /// 在此行号后插入（0 = 文件开头）
        insert_line: usize,
        new_str: String,
    },
    DeleteLines {
        path: String,
        start_line: usize,
        end_line: usize,
    },
    View {
        path: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
    },
}
```

### 7.5 grep_search

```rust
#[derive(Debug, Deserialize)]
pub struct GrepSearchInput {
    pub pattern: String,
    /// 搜索根目录，缺省 workspace
    pub path: Option<String>,
    /// glob 过滤，如 "**/*.rs"
    pub file_pattern: Option<String>,
    /// 是否大小写不敏感
    pub case_insensitive: Option<bool>,
    /// 最大返回结果数
    pub max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub content: String,
}
```

### 7.6 list_files

```rust
#[derive(Debug, Deserialize)]
pub struct ListFilesInput {
    pub path: Option<String>,
    pub recursive: Option<bool>,
    pub max_depth: Option<usize>,
    /// 是否显示隐藏文件（. 开头）
    pub show_hidden: Option<bool>,
}
```

### 7.7 ask_human

ask_human 是特殊工具：执行时通过 `EngineEvent::WaitingForHuman` 事件挂起循环，调用者调用 `AgentEngine::resume_with_human_input(ask_id, answer)` 来恢复。

```rust
#[derive(Debug, Deserialize)]
pub struct AskHumanInput {
    pub question: String,
    /// 可选的选项列表（UI 层可渲染为按钮）
    pub options: Option<Vec<String>>,
}

// 内部通过 oneshot channel 实现挂起/恢复
// engine 持有 ask_id → oneshot::Sender<String> 的 Map
// resume_with_human_input 通过 ask_id 找到 sender 并发送答案
```

---

## 8. LLM 适配层

### 8.1 rig 框架集成策略

rig 提供了 `rig::providers::*` 的原生客户端，以及 `rig::agent::Agent` 和 `rig::completion::*` trait。

**集成方式**：在 `adapters/llm/` 中将 rig 的 completion trait 包装为 `LLMProvider` trait，而不是直接在 engine 中使用 rig 的 `Agent` 抽象（后者过于高层，不适合我们需要细粒度控制 tool_call 循环的场景）。

### 8.2 Anthropic 适配器示例

```rust
// src/adapters/llm/anthropic.rs

use rig::providers::anthropic::{self, CLAUDE_OPUS_4_5};
use rig::completion::CompletionModel;

pub struct AnthropicProvider {
    client: anthropic::Client,
    model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, model: impl Into<String>) -> Self {
        let client = anthropic::Client::new(api_key);
        Self { client, model: model.into() }
    }
}

#[async_trait::async_trait]
impl LLMProvider for AnthropicProvider {
    fn model_id(&self) -> &str { &self.model }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, EngineError> {
        // 将 LLMMessage[] → rig 的消息格式
        // 将 ToolDefinition[] → rig 的 tool schema
        // 调用 rig completion API
        // 将响应转换回 LLMResponse
        todo!("convert rig response to LLMResponse")
    }

    async fn complete_stream(
        &self,
        request: LLMRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError> {
        // 使用 rig 的 stream API，将 chunk 转发到 tx
        todo!("rig stream → StreamChunk")
    }

    fn estimate_tokens(&self, messages: &[LLMMessage]) -> u32 {
        // 粗估：每条消息 content 字符数 / 4
        messages.iter()
            .flat_map(|m| &m.content)
            .map(|c| match c {
                LLMContent::Text { text } => text.len() as u32 / 4,
                LLMContent::ToolUse { input, .. } => input.to_string().len() as u32 / 4,
                LLMContent::ToolResult { content, .. } => content.len() as u32 / 4,
                LLMContent::Image { .. } => 500, // 固定估算
            })
            .sum()
    }
}
```

### 8.3 DeepSeek 适配器

DeepSeek 兼容 OpenAI API，使用 rig 的 OpenAI 提供商配置自定义 base_url：

```rust
// src/adapters/llm/deepseek.rs

pub struct DeepSeekProvider {
    // 复用 OpenAI 适配器，覆盖 base_url 为 https://api.deepseek.com
    inner: OpenAIProvider,
}

impl DeepSeekProvider {
    pub fn new(api_key: &str, model: impl Into<String>) -> Self {
        let inner = OpenAIProvider::with_base_url(
            api_key,
            "https://api.deepseek.com/v1",
            model,
        );
        Self { inner }
    }
}

// 透传 LLMProvider impl 给 inner
```

---

## 9. 会话服务

```rust
// src/application/session_service.rs

use crate::{
    domain::{agent::AgentConfig, session::{Session, SessionStatus}},
    ports::storage::SessionStorage,
    error::EngineError,
};
use std::sync::Arc;

pub struct SessionService {
    storage: Arc<dyn SessionStorage>,
}

impl SessionService {
    pub fn new(storage: Arc<dyn SessionStorage>) -> Self {
        Self { storage }
    }

    /// 创建新 Session
    pub async fn create(&self, config: &AgentConfig) -> Result<Session, EngineError> {
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            workspace: config.workspace.to_string_lossy().to_string(),
            agent_config_snapshot: serde_json::to_value(config)
                .map_err(|e| EngineError::Serialization(e.to_string()))?,
            status: SessionStatus::Idle,
            total_tokens: 0,
            iteration_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        self.storage.create_session(&session).await?;
        Ok(session)
    }

    /// 恢复已有 Session（校验状态合法性）
    pub async fn resume(&self, session_id: &str) -> Result<Session, EngineError> {
        let session = self.storage.get_session(session_id).await?
            .ok_or_else(|| EngineError::SessionNotFound(session_id.to_string()))?;

        match session.status {
            SessionStatus::Running => Err(EngineError::SessionAlreadyRunning),
            SessionStatus::Completed | SessionStatus::Cancelled | SessionStatus::Error | SessionStatus::Idle | SessionStatus::WaitingForHuman => Ok(session),
        }
    }

    pub async fn list(&self, workspace: &str) -> Result<Vec<Session>, EngineError> {
        self.storage.list_sessions(workspace).await
    }
}
```

---

## 10. SQLite Schema

```sql
-- migrations/001_init.sql

CREATE TABLE IF NOT EXISTS sessions (
    id                      TEXT PRIMARY KEY,
    workspace               TEXT NOT NULL,
    agent_config_snapshot   TEXT NOT NULL,  -- JSON
    status                  TEXT NOT NULL DEFAULT 'idle',
    total_tokens            INTEGER NOT NULL DEFAULT 0,
    iteration_count         INTEGER NOT NULL DEFAULT 0,
    created_at              TEXT NOT NULL,  -- ISO 8601
    updated_at              TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_workspace ON sessions(workspace);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);

-- -------------------------------------------------------

CREATE TABLE IF NOT EXISTS messages (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role        TEXT NOT NULL,                  -- system/user/assistant/tool
    content     TEXT NOT NULL,                  -- JSON: Vec<LLMContent>
    turn_index  INTEGER NOT NULL DEFAULT 0,
    token_count INTEGER,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

-- -------------------------------------------------------

CREATE TABLE IF NOT EXISTS tool_calls (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    input       TEXT NOT NULL,  -- JSON
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tool_results (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tool_call_id    TEXT NOT NULL REFERENCES tool_calls(id),
    tool_name       TEXT NOT NULL,
    output          TEXT NOT NULL,
    is_error        INTEGER NOT NULL DEFAULT 0,  -- BOOLEAN
    duration_ms     INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);
CREATE INDEX IF NOT EXISTS idx_tool_results_session ON tool_results(session_id);
```

---

## 11. 错误类型

```rust
// src/error.rs

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("LLM provider error: {0}")]
    LLMProvider(String),

    #[error("LLM stream interrupted unexpectedly")]
    LLMStreamInterrupted,

    #[error("Max tokens reached")]
    MaxTokensReached,

    #[error("Max iterations ({0}) reached")]
    MaxIterationsReached(u32),

    #[error("Tool '{0}' not found in registry")]
    ToolNotFound(String),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Session '{0}' not found")]
    SessionNotFound(String),

    #[error("Session is already running")]
    SessionAlreadyRunning,

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Context compaction failed: {0}")]
    Compaction(String),

    #[error("Operation cancelled")]
    Cancelled,
}
```

---

## 12. Cargo.toml

```toml
[package]
name = "srow-engine"
version = "0.1.0"
edition = "2021"
description = "Srow Agent core engine — LLM loop, tools, session management"

[lib]
name = "srow_engine"
path = "src/lib.rs"

[dependencies]
# LLM 框架
rig-core = { version = "0.9", features = ["all"] }

# 异步运行时
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
futures = "0.3"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 数据库
tokio-rusqlite = "0.5"
rusqlite = { version = "0.31", features = ["bundled"] }

# ID / 时间
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }

# 错误处理
thiserror = "1"
anyhow = "1"

# 工具实现辅助
walkdir = "2"           # list_files
regex = "1"             # grep_search
tokio-process = "1"     # execute_shell（或直接用 tokio::process）

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
mockall = "0.12"

[features]
default = []
# 启用所有内置工具
builtin-tools = []
```

---

## 13. 公开 API（lib.rs 导出）

```rust
// src/lib.rs

pub mod domain {
    pub mod message;
    pub mod tool;
    pub mod session;
    pub mod agent;
}

pub mod ports {
    pub mod llm_provider;
    pub mod tool;
    pub mod storage;
}

pub mod application {
    pub mod engine;
    pub mod context_manager;
    pub mod session_service;
}

pub mod adapters {
    pub mod llm {
        pub mod openai;
        pub mod anthropic;
        pub mod gemini;
        pub mod deepseek;
    }
    pub mod tools {
        pub mod execute_shell;
        pub mod create_file;
        pub mod file_edit;
        pub mod grep_search;
        pub mod list_files;
        pub mod ask_human;
    }
    pub mod storage {
        pub mod sqlite;
    }
}

pub mod error;

// 便捷 re-export
pub use application::engine::{AgentEngine, EngineEvent};
pub use application::session_service::SessionService;
pub use domain::agent::{AgentConfig, LLMConfig, LLMProviderKind};
pub use domain::message::{LLMMessage, LLMContent, Role};
pub use domain::session::{Session, SessionStatus};
pub use domain::tool::{ToolCall, ToolResult, ToolDefinition};
pub use error::EngineError;
pub use ports::llm_provider::LLMProvider;
pub use ports::tool::{Tool, ToolRegistry, ToolContext};
pub use ports::storage::SessionStorage;

/// 快速构建引擎的 Builder（隐藏 Arc 手动组装的繁琐）
pub struct EngineBuilder {
    config: AgentConfig,
    llm: Option<std::sync::Arc<dyn LLMProvider>>,
    extra_tools: Vec<Box<dyn ports::tool::Tool>>,
    storage: Option<std::sync::Arc<dyn SessionStorage>>,
}

impl EngineBuilder {
    pub fn new(config: AgentConfig) -> Self {
        Self { config, llm: None, extra_tools: vec![], storage: None }
    }

    pub fn with_llm(mut self, llm: impl LLMProvider + 'static) -> Self {
        self.llm = Some(std::sync::Arc::new(llm));
        self
    }

    pub fn with_tool(mut self, tool: impl ports::tool::Tool + 'static) -> Self {
        self.extra_tools.push(Box::new(tool));
        self
    }

    pub fn with_storage(mut self, storage: impl SessionStorage + 'static) -> Self {
        self.storage = Some(std::sync::Arc::new(storage));
        self
    }

    /// 使用默认 SQLite storage（数据库路径 = workspace/.srow/engine.db）
    pub async fn with_default_sqlite_storage(self) -> Result<Self, EngineError> {
        let db_path = self.config.workspace.join(".srow").join("engine.db");
        let storage = adapters::storage::sqlite::SqliteStorage::open(&db_path).await?;
        Ok(self.with_storage(storage))
    }

    pub fn build(
        self,
        event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<AgentEngine, EngineError> {
        let llm = self.llm.ok_or_else(|| EngineError::LLMProvider("no LLM configured".to_string()))?;
        let storage = self.storage.ok_or_else(|| EngineError::Storage("no storage configured".to_string()))?;

        let mut registry = ToolRegistry::new();

        // 注册内置工具
        registry.register(Box::new(adapters::tools::execute_shell::ExecuteShellTool));
        registry.register(Box::new(adapters::tools::create_file::CreateFileTool));
        registry.register(Box::new(adapters::tools::file_edit::FileEditTool));
        registry.register(Box::new(adapters::tools::grep_search::GrepSearchTool));
        registry.register(Box::new(adapters::tools::list_files::ListFilesTool));
        registry.register(Box::new(adapters::tools::ask_human::AskHumanTool::new(event_tx.clone())));

        // 注册额外工具（Sub-4 Skill / Sub-3 MCP 会在这里注入）
        for tool in self.extra_tools {
            registry.register(tool);
        }

        Ok(AgentEngine::new(
            self.config,
            llm,
            std::sync::Arc::new(registry),
            storage,
            event_tx,
            cancel_rx,
        ))
    }
}
```

---

## 14. 与上层系统的集成接口

### 14.1 Tauri Command 调用方式

Sub-2 作为独纯 Rust crate，由 Tauri 主进程调用：

```rust
// tauri src-tauri/src/commands/agent.rs（示意，不在 srow-engine 内）

#[tauri::command]
async fn start_agent_session(
    config: AgentConfig,
    prompt: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (event_tx, mut event_rx) = mpsc::channel(256);
    let (cancel_tx, cancel_rx) = watch::channel(false);

    let engine = EngineBuilder::new(config)
        .with_llm(AnthropicProvider::new(&api_key, "claude-opus-4-5"))
        .with_default_sqlite_storage().await?
        .build(event_tx, cancel_rx)?;

    let session = SessionService::new(engine.storage()).create(&engine.config()).await?;
    let session_id = session.id.clone();

    // 事件转发到 Tauri 前端
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            window.emit("engine-event", &event).ok();
        }
    });

    // 在后台运行引擎循环
    tokio::spawn(async move {
        engine.run(&session_id, LLMMessage::user(prompt)).await.ok();
    });

    Ok(session.id)
}
```

### 14.2 Sub-3 / Sub-4 扩展点

后续子项目的扩展点已预留：

- **Sub-3 ACP**：实现 `LLMProvider` trait，将 Claude Code / Qwen Code 的 stdin/stdout 协议封装为 LLM 提供商
- **Sub-4 Skill / MCP**：实现 `Tool` trait，通过 `EngineBuilder::with_tool()` 注入
- **Sub-5 编排层**：直接调用 `EngineBuilder` + `SessionService`，管理多个 `AgentEngine` 实例
- **Sub-7 安全层**：在 `ToolContext.allow_dangerous` 和 `execute_shell` 工具内部加入沙箱检查

---

## 15. 测试策略

### 15.1 单元测试

- `domain/` 层：纯 Rust 类型，直接测试序列化 / 反序列化
- `application/context_manager.rs`：测试 compaction 阈值和截断逻辑
- `adapters/tools/`：使用 `tempfile` 创建临时目录测试文件操作

### 15.2 集成测试

```rust
// tests/engine_loop.rs

#[tokio::test]
async fn test_engine_completes_without_tools() {
    // Mock LLMProvider 返回 EndTurn，验证循环正常退出
}

#[tokio::test]
async fn test_engine_executes_tool_and_continues() {
    // Mock LLMProvider 第一次返回 ToolUse，第二次返回 EndTurn
    // 验证工具被调用，消息被正确追加
}

#[tokio::test]
async fn test_engine_respects_max_iterations() {
    // Mock LLMProvider 永远返回 ToolUse
    // 验证 MaxIterationsReached 错误
}

#[tokio::test]
async fn test_session_persistence_and_resume() {
    // 创建 Session → 运行部分 → 模拟重启 → resume → 验证历史消息完整
}
```

### 15.3 mockall 用法

```rust
use mockall::automock;

#[automock]
#[async_trait]
pub trait LLMProvider: Send + Sync {
    // ... （在 ports/llm_provider.rs 加 #[automock]）
}
```

---

## 16. 实现优先级与里程碑

| 里程碑 | 内容 | 验收标准 |
|--------|------|---------|
| M1 | 领域类型 + trait 定义 | cargo build 无错误，所有类型可序列化 |
| M2 | SQLite storage 实现 | 集成测试：Session CRUD + 消息持久化 |
| M3 | Anthropic LLM 适配器 | 真实 API 调用，流式 chunk 正确输出 |
| M4 | 核心循环（无工具） | 单轮对话，EndTurn 正常退出 |
| M5 | 内置工具（file/shell/grep） | 工具在循环内被正确调用和记录 |
| M6 | ask_human 工具 | 挂起/恢复机制，Tauri event 联通 |
| M7 | 上下文压缩 | 超阈值后 history 被正确截断，循环继续 |
| M8 | OpenAI / Gemini / DeepSeek 适配器 | 各适配器可切换，行为一致 |
