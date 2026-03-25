# AI SDK React 完整复刻设计规格

> 将 Vercel AI SDK (`ai` core + `@ai-sdk/react`) 的架构一比一复刻为 Rust 实现，
> 适配 GPUI 桌面应用。

---

## 1. Crate 映射

```
AI SDK                          Rust crate
─────────────────────────────────────────────
@ai-sdk/provider         →     alva-app-core::ports       (已有)
@ai-sdk/provider-utils   →     alva-app-core 新增模块      (JSON/schema 工具)
ai (core)                →     srow-ai                 (新 crate)
@ai-sdk/react            →     alva-app                (改造)
```

```toml
# Cargo.toml (workspace)
[workspace]
members = [
    "src/alva-app",
    "src/alva-app-core",
    "src/srow-ai",
]
```

依赖方向：`alva-app → srow-ai → alva-app-core`

> **命名说明**：新 crate 命名为 `srow-ai` 而非 `srow-ui`，因为该 crate 包含的是框架无关的
> AI 交互业务逻辑（Chat/Transport/Stream），不包含任何 UI 代码。

---

## 2. alva-app-core 改造：数据模型 + 流协议

### 2.1 新增模块结构

```
src/alva-app-core/src/
├── ui_message/
│   ├── mod.rs              # UIMessage, UIMessagePart, UIMessageRole, re-exports
│   ├── parts.rs            # 所有 Part 变体和状态枚举
│   └── convert.rs          # UIMessage ↔ LLMMessage 转换
├── ui_message_stream/
│   ├── mod.rs              # UIMessageChunk, FinishReason, ChatStatus, re-exports
│   ├── processor.rs        # process_ui_message_stream 等价逻辑
│   ├── state.rs            # StreamingUIMessageState
│   ├── writer.rs           # UIMessageStreamWriter (服务端写入)
│   └── sse.rs              # SSE 解析/序列化工具
└── (现有模块保留)
```

### 2.2 UIMessage + UIMessagePart

```rust
// ui_message/mod.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UIMessage {
    pub id: String,
    pub role: UIMessageRole,
    pub parts: Vec<UIMessagePart>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UIMessageRole {
    System,
    User,
    Assistant,
}
```

> **注意**：`UIMessageRole` 没有 `Tool` 变体。工具调用结果作为 `UIMessagePart::Tool`
> 嵌入在 `Assistant` 消息内部。转换为 `LLMMessage` 时，需要将 `Tool` parts 拆分为
> 独立的 `Role::Tool` 消息（见 2.6 节）。

```rust
// ui_message/parts.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UIMessagePart {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<TextPartState>,
    },
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<TextPartState>,
    },
    Tool {
        id: String,
        tool_name: String,
        input: serde_json::Value,
        state: ToolState,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    File {
        media_type: String,
        data: String,
    },
    SourceUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    SourceDocument {
        id: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_type: Option<String>,
    },
    StepStart,
    Custom {
        id: String,
        data: serde_json::Value,
    },
    Data {
        name: String,
        data: serde_json::Value,
    },
}

/// 文本/推理 part 的流式状态
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextPartState {
    Streaming,
    Done,
}

/// 工具调用 7 态生命周期
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolState {
    InputStreaming,
    InputAvailable,
    ApprovalRequested,
    ApprovalResponded,
    OutputAvailable,
    OutputError,
    OutputDenied,
}
```

### 2.3 UIMessageChunk（流协议，替代 EngineEvent）

```rust
// ui_message_stream/mod.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UIMessageChunk {
    // — 生命周期 —
    Start {
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_metadata: Option<serde_json::Value>,
    },
    Finish {
        finish_reason: FinishReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },

    // — 文本 —
    TextStart { id: String },
    TextDelta { id: String, delta: String },
    TextEnd { id: String },

    // — 推理 —
    ReasoningStart { id: String },
    ReasoningDelta { id: String, delta: String },
    ReasoningEnd { id: String },

    // — 工具调用 —
    ToolInputStart {
        id: String,
        tool_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    ToolInputDelta { id: String, delta: String },
    ToolInputAvailable { id: String, input: serde_json::Value },
    ToolInputError { id: String, error: String },
    ToolApprovalRequest { id: String },
    ToolOutputAvailable { id: String, output: serde_json::Value },
    ToolOutputError { id: String, error: String },
    ToolOutputDenied { id: String },

    // — 文件 —
    File { id: String, media_type: String, data: String },
    ReasoningFile { id: String, media_type: String, data: String },

    // — 引用 —
    SourceUrl { id: String, url: String, title: Option<String> },
    SourceDocument { id: String, title: String, source_type: Option<String> },

    // — 自定义 —
    Custom { id: String, data: serde_json::Value },

    // — 数据 —
    Data { name: String, data: serde_json::Value },

    // — 步骤 —
    StartStep,
    FinishStep,

    // — 元数据 —
    MessageMetadata { metadata: serde_json::Value },

    // — Token 使用量 —
    TokenUsage { usage: TokenUsage },

    // — 错误 / 中止 —
    Error { error: String },
    Abort,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    MaxTokens,
    Error,
    Abort,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatStatus {
    Ready,
    Submitted,
    Streaming,
    Error,
}
```

> **TokenUsage 决定**：`TokenUsage` 同时作为独立 chunk 变体（引擎每次迭代可发射）和
> `Finish` 的可选字段（最终汇总）。这样既保留了每次迭代的用量数据，又在 Finish 时提供汇总。

### 2.4 StreamingUIMessageState

```rust
// ui_message_stream/state.rs

use std::collections::HashMap;

pub struct StreamingUIMessageState {
    pub message: UIMessage,
    pub active_text_parts: HashMap<String, usize>,
    pub active_reasoning_parts: HashMap<String, usize>,
    pub partial_tool_calls: HashMap<String, PartialToolCall>,
    pub finish_reason: Option<FinishReason>,
}

pub struct PartialToolCall {
    pub text: String,
    pub index: usize,
    pub tool_name: String,
    pub title: Option<String>,
}
```

### 2.5 process_ui_message_stream

```rust
// ui_message_stream/processor.rs

/// 将 UIMessageChunk 流转换为 UIMessage 状态更新。
/// 对应 AI SDK 的 processUIMessageStream。
///
/// 每次状态变化通过 update_tx 发送最新的 UIMessage 快照。
/// 调用方从 update_rx 消费更新并同步到 UI。
/// 这避免了在 async 上下文中直接调用 GPUI 的 cx.notify()。
pub async fn process_ui_message_stream(
    stream: impl Stream<Item = Result<UIMessageChunk, StreamError>> + Send,
    initial_message: UIMessage,
    update_tx: mpsc::UnboundedSender<UIMessageStreamUpdate>,
) -> Result<StreamingUIMessageState, StreamError> {
    // chunk-by-chunk 处理逻辑，和 AI SDK 完全一致：
    // - TextStart → 新建 Text part (state: Streaming), send update
    // - TextDelta → 累积文本, send update
    // - TextEnd → 标记 state: Done, send update
    // - ToolInputStart → 新建 Tool part (state: InputStreaming), send update
    // - ToolInputDelta → 累积 JSON 文本
    // - ToolInputAvailable → 解析 JSON，设 state: InputAvailable, send update
    // - ToolApprovalRequest → 设 state: ApprovalRequested, send update
    // - ToolOutputAvailable → 设 output + state: OutputAvailable, send update
    // - Data → 追加 Data part, send update
    // - StartStep/FinishStep → 重置 active parts
    // - Start → 设 message_id
    // - Finish → 设 finish_reason
    // ...
}

/// 状态更新事件
pub enum UIMessageStreamUpdate {
    /// 消息内容已变化（需要重渲染）
    MessageChanged(UIMessage),
    /// 这是首次写入（status 应从 Submitted → Streaming）
    FirstWrite(UIMessage),
    /// 流完成
    Finished(StreamingUIMessageState),
}
```

### 2.6 UIMessage ↔ LLMMessage 转换

```rust
// ui_message/convert.rs

/// UIMessage[] → LLMMessage[]（发给 LLM API 时用）
///
/// 转换规则：
/// - UIMessage { role: User, parts: [Text] }  → LLMMessage { role: User, content: [Text] }
/// - UIMessage { role: Assistant, parts: [Text, Tool, Tool] } →
///     LLMMessage { role: Assistant, content: [Text, ToolUse, ToolUse] }
///   + LLMMessage { role: Tool, content: [ToolResult] }  (如果 Tool.output 存在)
///   + LLMMessage { role: Tool, content: [ToolResult] }
///
/// 即：Assistant 消息中的 Tool parts 拆分为：
///   1. ToolUse 放在 Assistant 消息的 content 中
///   2. ToolResult 生成为独立的 Role::Tool 消息（紧跟在 Assistant 后）
pub fn ui_messages_to_llm_messages(messages: &[UIMessage]) -> Vec<LLMMessage> { ... }

/// LLMMessage 流式响应 → UIMessageChunk 流（DirectTransport 内部用）
///
/// StreamChunk::TextDelta → TextStart(once) + TextDelta
/// StreamChunk::ThinkingDelta → ReasoningStart(once) + ReasoningDelta
/// StreamChunk::ToolCallDelta → ToolInputStart(once) + ToolInputDelta
/// StreamChunk::Done → TextEnd/ReasoningEnd + ToolInputAvailable + Finish
pub fn llm_stream_to_ui_chunks(
    chunks: impl Stream<Item = StreamChunk> + Send,
) -> impl Stream<Item = UIMessageChunk> + Send { ... }
```

### 2.7 SSE 解析/序列化

```rust
// ui_message_stream/sse.rs

/// 解析 SSE 文本流为 UIMessageChunk 流
/// 格式: "data: {json}\n\n"，终止: "data: [DONE]\n\n"
pub fn parse_sse_stream(
    byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send,
) -> impl Stream<Item = Result<UIMessageChunk, StreamError>> + Send { ... }

/// 将 UIMessageChunk 序列化为 SSE 格式
pub fn chunk_to_sse(chunk: &UIMessageChunk) -> String {
    format!("data: {}\n\n", serde_json::to_string(chunk).unwrap())
}
```

### 2.8 删除 EngineEvent

现有 `EngineEvent` 被 `UIMessageChunk` 完全替代。`AgentEngine` 的 `event_tx` 类型从
`mpsc::Sender<EngineEvent>` 改为 `mpsc::Sender<UIMessageChunk>`。

---

## 3. srow-ai 新 crate：Chat 业务逻辑层

### 3.1 模块结构

```
src/srow-ai/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── chat/
    │   ├── mod.rs
    │   ├── abstract_chat.rs      # AbstractChat — 核心聊天逻辑
    │   ├── chat_state.rs         # ChatState trait
    │   └── chat_options.rs       # ChatInit, SendOptions, etc.
    ├── transport/
    │   ├── mod.rs
    │   ├── traits.rs             # ChatTransport trait
    │   ├── direct.rs             # DirectChatTransport (进程内)
    │   ├── http_sse.rs           # HttpSseChatTransport (HTTP SSE)
    │   └── text_stream.rs        # TextStreamChatTransport (纯文本)
    ├── completion/
    │   ├── mod.rs
    │   └── completion.rs         # Completion 逻辑 (对应 useCompletion)
    ├── object/
    │   ├── mod.rs
    │   └── object_generation.rs  # ObjectGeneration 逻辑 (对应 useObject)
    └── util/
        ├── mod.rs
        ├── serial_job_executor.rs
        ├── throttle.rs
        └── abort.rs              # AbortHandle / AbortController
```

### 3.2 AbortHandle（取消机制）

```rust
// util/abort.rs

/// 取消控制器，对应 Web API 的 AbortController。
/// 内部组合了 tokio watch channel（协作式取消）和 JoinHandle（强制取消）。
pub struct AbortController {
    cancel_tx: watch::Sender<bool>,
}

/// 取消句柄，可 clone 传递给多个消费方。
#[derive(Clone)]
pub struct AbortHandle {
    cancel_rx: watch::Receiver<bool>,
}

impl AbortController {
    pub fn new() -> (Self, AbortHandle) {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        (Self { cancel_tx }, AbortHandle { cancel_rx })
    }

    /// 发出取消信号
    pub fn abort(&self) {
        let _ = self.cancel_tx.send(true);
    }
}

impl AbortHandle {
    /// 检查是否已取消
    pub fn is_aborted(&self) -> bool {
        *self.cancel_rx.borrow()
    }

    /// 等待取消信号
    pub async fn cancelled(&mut self) {
        while !*self.cancel_rx.borrow() {
            if self.cancel_rx.changed().await.is_err() {
                return; // sender dropped
            }
        }
    }
}
```

### 3.3 ChatState trait

框架无关的状态管理接口。**不要求 `Send + Sync`**，因为 GPUI 的 `Context` 不是
`Send + Sync`。`AbstractChat` 通过 channel 桥接异步任务和状态更新。

```rust
// chat/chat_state.rs

/// 框架无关的聊天状态接口。
/// 不要求 Send + Sync — 框架绑定层负责线程安全。
pub trait ChatState {
    fn messages(&self) -> Vec<UIMessage>;  // 返回 owned 拷贝，避免借用冲突
    fn set_messages(&mut self, messages: Vec<UIMessage>);
    fn push_message(&mut self, message: UIMessage);
    fn pop_message(&mut self) -> Option<UIMessage>;
    fn replace_message(&mut self, index: usize, message: UIMessage);

    fn status(&self) -> ChatStatus;
    fn set_status(&mut self, status: ChatStatus);

    fn error(&self) -> Option<ChatError>;  // 返回 owned Clone
    fn set_error(&mut self, error: Option<ChatError>);

    /// 通知 UI 框架重新渲染
    fn notify_messages_changed(&mut self);
    fn notify_status_changed(&mut self);
    fn notify_error_changed(&mut self);
}
```

### 3.4 ChatInit

```rust
// chat/chat_options.rs

pub struct ChatInit<S: ChatState> {
    pub id: String,
    pub state: S,
    pub transport: Box<dyn ChatTransport>,
    pub runtime_handle: tokio::runtime::Handle,
    pub generate_id: Option<Box<dyn Fn() -> String + Send + Sync>>,
    pub initial_messages: Vec<UIMessage>,
    pub on_tool_call: Option<AsyncToolCallHandler>,
    pub on_finish: Option<Box<dyn Fn(FinishInfo) + Send + Sync>>,
    pub on_error: Option<Box<dyn Fn(ChatError) + Send + Sync>>,
    pub send_automatically_when: Option<Box<dyn Fn(&UIMessage) -> bool + Send + Sync>>,
}

/// 异步工具调用回调（因为工具执行本质上是异步的）
pub type AsyncToolCallHandler = Box<
    dyn Fn(ToolCallInfo) -> Pin<Box<dyn Future<Output = ToolCallResult> + Send>>
        + Send
        + Sync,
>;

pub struct SendOptions {
    pub metadata: Option<serde_json::Value>,
}

pub struct RegenerateOptions {
    pub metadata: Option<serde_json::Value>,
}

pub struct ToolCallInfo {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

pub enum ToolCallResult {
    Output(serde_json::Value),
    Error(String),
    /// 不处理，让服务端/引擎自行执行
    Unhandled,
}

pub struct FinishInfo {
    pub message: UIMessage,
    pub finish_reason: FinishReason,
    pub usage: Option<TokenUsage>,
}
```

### 3.5 ChatTransport trait

```rust
// transport/traits.rs

#[async_trait]
pub trait ChatTransport: Send + Sync {
    /// 发送消息，返回 UIMessageChunk 流
    async fn send_messages(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>, TransportError>;

    /// 尝试恢复中断的流（断线重连）
    async fn reconnect(
        &self,
        chat_id: &str,
    ) -> Result<Option<Pin<Box<dyn Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>, TransportError>;
}

pub struct ChatRequest {
    pub chat_id: String,
    pub messages: Vec<UIMessage>,
    pub abort_handle: AbortHandle,
}
```

### 3.6 DirectChatTransport

进程内直接调用 AgentEngine，将引擎输出转为 UIMessageChunk 流。

```rust
// transport/direct.rs

pub struct DirectChatTransport {
    llm: Arc<dyn LLMProvider>,
    tools: Arc<ToolRegistry>,
    storage: Arc<dyn SessionStorage>,
    config: Arc<AgentConfig>,  // Arc 包装，满足 'static Stream 要求
    approval_tx: mpsc::UnboundedSender<ApprovalResponse>,
    approval_rx: Arc<Mutex<mpsc::UnboundedReceiver<ApprovalResponse>>>,
}

/// 审批响应（从 UI 回传到引擎）
pub struct ApprovalResponse {
    pub tool_call_id: String,
    pub approved: bool,
}

#[async_trait]
impl ChatTransport for DirectChatTransport {
    async fn send_messages(&self, request: ChatRequest)
        -> Result<Pin<Box<dyn Stream<...> + Send>>, TransportError>
    {
        let (chunk_tx, chunk_rx) = mpsc::unbounded_channel::<UIMessageChunk>();
        let llm = self.llm.clone();
        let tools = self.tools.clone();
        let storage = self.storage.clone();
        let config = self.config.clone();
        let abort = request.abort_handle.clone();
        let messages = request.messages;
        let approval_rx = self.approval_rx.clone();

        // 在 tokio runtime 上运行引擎
        tokio::spawn(async move {
            // 1. UIMessage → LLMMessage
            // 2. 创建 AgentEngine（event_tx = chunk_tx）
            // 3. 引擎循环中：
            //    - 工具执行前检查 SecurityGuard
            //    - 需要审批时发 ToolApprovalRequest chunk，等待 approval_rx
            //    - 审批通过 → 执行工具 → ToolOutputAvailable
            //    - 审批拒绝 → ToolOutputDenied
            // 4. 引擎完成时发 Finish chunk
        });

        Ok(Box::pin(UnboundedReceiverStream::new(chunk_rx)))
    }
}
```

### 3.7 HttpSseChatTransport

```rust
// transport/http_sse.rs

pub struct HttpSseChatTransport {
    api_url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

#[async_trait]
impl ChatTransport for HttpSseChatTransport {
    async fn send_messages(&self, request: ChatRequest)
        -> Result<Pin<Box<dyn Stream<...> + Send>>, TransportError>
    {
        // 1. POST messages JSON to api_url
        // 2. 获取 Response body byte stream
        // 3. 通过 parse_sse_stream 解析为 UIMessageChunk 流
        // 4. 返回 Stream
    }
}
```

### 3.8 TextStreamChatTransport

```rust
// transport/text_stream.rs

pub struct TextStreamChatTransport {
    api_url: String,
    client: reqwest::Client,
}

// 将纯文本流转为 Start → TextStart → TextDelta* → TextEnd → Finish 的 chunk 流
```

### 3.9 AbstractChat

核心聊天逻辑，对应 AI SDK 的 `AbstractChat` 类。

**内部可变性策略**：所有可变状态放在 `Arc<Mutex<ChatInner<S>>>`，使 `AbstractChat`
的公开方法可以用 `&self`。`SerialJobExecutor` 保证同一时刻只有一个 job 在执行，
所以 Mutex 争用极低。

```rust
// chat/abstract_chat.rs

pub struct AbstractChat<S: ChatState> {
    inner: Arc<Mutex<ChatInner<S>>>,
    transport: Arc<dyn ChatTransport>,
    job_executor: SerialJobExecutor,
    generate_id: Arc<dyn Fn() -> String + Send + Sync>,

    // 回调（通过 Arc 在异步任务中可用）
    on_tool_call: Option<AsyncToolCallHandler>,
    on_finish: Option<Arc<dyn Fn(FinishInfo) + Send + Sync>>,
    on_error: Option<Arc<dyn Fn(ChatError) + Send + Sync>>,
    send_automatically_when: Option<Arc<dyn Fn(&UIMessage) -> bool + Send + Sync>>,
}

struct ChatInner<S: ChatState> {
    id: String,
    state: S,
    active_abort: Option<AbortController>,
}

impl<S: ChatState> AbstractChat<S> {
    pub fn new(init: ChatInit<S>) -> Self { ... }

    // --- 公开 API (全部 &self) ---

    pub async fn send_message(&self, parts: Vec<UIMessagePart>, options: SendOptions) {
        // 1. 构建 UIMessage { role: User, parts }
        // 2. inner.lock → state.push_message → notify
        // 3. make_request(trigger: SubmitMessage)
    }

    pub async fn regenerate(&self, options: RegenerateOptions) {
        // 1. inner.lock → pop last assistant message → notify
        // 2. make_request(trigger: Regenerate)
    }

    pub async fn stop(&self) {
        // inner.lock → active_abort.take()?.abort()
        // → AbortController 发信号 → cancel_rx 触发 → 引擎停止
        // → 设 status = Ready
    }

    pub async fn resume_stream(&self) {
        // transport.reconnect(chat_id)
        // 如果有流 → consume_stream
    }

    pub async fn add_tool_output(&self, tool_call_id: &str, output: ToolOutput) {
        // 1. inner.lock → 找到 Tool part → 更新 state + output → notify
        // 2. 如果 active_response 存在，同步更新流构建状态
        // 3. 检查 send_automatically_when
    }

    pub async fn add_tool_approval_response(&self, tool_call_id: &str, approved: bool) {
        // 1. inner.lock → 更新 Tool part state → ApprovalResponded → notify
        // 2. 通过 transport 的 approval channel 回传到引擎
        // 3. 如果 approved && send_automatically_when → 自动续发
    }

    pub fn clear_error(&self) {
        // inner.lock → state.set_error(None) → set_status(Ready) → notify
    }

    // --- 内部方法 ---

    async fn make_request(&self, trigger: RequestTrigger) {
        self.job_executor.run(|| async {
            // 1. 创建 AbortController, 存入 inner.active_abort
            // 2. inner.lock → set_status(Submitted) → notify
            // 3. transport.send_messages(ChatRequest { messages, abort_handle })
            // 4. consume_stream(stream)
            // 5. inner.lock → set_status(Ready) → notify
            // 6. on_finish 回调
        }).await;
    }

    async fn consume_stream(&self, stream: impl Stream<...>) {
        // 1. 创建 update channel
        // 2. spawn process_ui_message_stream(stream, initial_msg, update_tx)
        // 3. 消费 update_rx:
        //    - FirstWrite → inner.lock → set_status(Streaming) → push_message → notify
        //    - MessageChanged → inner.lock → replace_message(last) → notify
        //    - Finished → 处理工具回调、自动续发等
    }
}
```

### 3.10 SerialJobExecutor

```rust
// util/serial_job_executor.rs

/// 串行任务执行器。
/// 接收一个 `runtime::Handle`，避免在非 tokio 线程上 panic。
pub struct SerialJobExecutor {
    tx: mpsc::UnboundedSender<Job>,
}

type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

impl SerialJobExecutor {
    pub fn new(handle: &tokio::runtime::Handle) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        handle.spawn(async move {
            while let Some(job) = rx.recv().await {
                job.await;
            }
        });
        Self { tx }
    }

    pub async fn run<F>(&self, job: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(Box::pin(async move {
            job.await;
            let _ = done_tx.send(());
        }));
        let _ = done_rx.await;
    }
}
```

### 3.11 Completion（对应 useCompletion）

> **设计说明**：`Completion` 和 `ObjectGeneration` 故意不使用 `ChatTransport` 抽象。
> 它们对应 AI SDK 中更简单的 `callCompletionApi` 和直接 HTTP 请求模式，只需要 HTTP
> 能力，不需要完整的聊天 Transport 语义。这与 AI SDK 的设计一致。

```rust
// completion/completion.rs

pub struct Completion {
    api_url: String,
    client: reqwest::Client,
    headers: HashMap<String, String>,
    completion: String,
    is_loading: bool,
    error: Option<ChatError>,
    abort: Option<AbortController>,
    on_finish: Option<Box<dyn Fn(&str, &str) + Send + Sync>>,
}

impl Completion {
    pub async fn complete(&mut self, prompt: &str) -> Result<String, ChatError> {
        // 1. POST to api_url with prompt
        // 2. 解析 SSE 流，只提取 text-delta 的 delta
        // 3. 累积到 self.completion
        // 4. 返回完整文本
    }
    pub fn stop(&mut self) { ... }
    pub fn completion(&self) -> &str { &self.completion }
    pub fn is_loading(&self) -> bool { self.is_loading }
}
```

### 3.12 ObjectGeneration（对应 useObject）

```rust
// object/object_generation.rs

pub struct ObjectGeneration<T: DeserializeOwned> {
    api_url: String,
    client: reqwest::Client,
    headers: HashMap<String, String>,
    object: Option<serde_json::Value>,
    is_loading: bool,
    error: Option<ChatError>,
    abort: Option<AbortController>,
    _phantom: PhantomData<T>,
}

impl<T: DeserializeOwned> ObjectGeneration<T> {
    pub async fn submit(&mut self, input: serde_json::Value) {
        // 1. POST to api_url
        // 2. Stream JSON text
        // 3. 使用 partial JSON parser 逐步解析
        // 4. 深度比较避免不必要的更新
        // 5. 完成时做 schema 验证
    }

    pub fn object(&self) -> Option<&serde_json::Value> { self.object.as_ref() }
    pub fn typed_object(&self) -> Option<T> {
        self.object.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    pub fn stop(&mut self) { ... }
    pub fn clear(&mut self) { ... }
}
```

---

## 4. alva-app 改造：GPUI 绑定层

### 4.1 删除的文件

- `src/alva-app/src/types/message.rs` — `Message`, `MessageContent`, `MessageRole`
- `src/alva-app/src/models/chat_model.rs` — `ChatModel`, `ChatModelEvent`
- `src/alva-app/src/engine_bridge/` — 整个模块

### 4.2 新增模块

```
src/alva-app/src/
├── chat/                           # 新增
│   ├── mod.rs
│   ├── gpui_chat.rs                # GpuiChat — GPUI Entity 包装
│   └── gpui_chat_state.rs          # GpuiChatState — 实现 ChatState trait
├── models/
│   ├── chat_model.rs               # 重写：管理多 session 的 GpuiChat
│   └── ...
└── views/
    └── chat_panel/
        ├── chat_panel.rs           # 改造
        ├── message_list.rs         # 改造：渲染 UIMessage parts
        ├── input_box.rs            # 改造
        └── tool_approval.rs        # 新增：内联审批 UI
```

### 4.3 GpuiChatState

**GPUI 通知机制**：不使用回调，而是通过 channel 发送通知信号。GpuiChat Entity
在 GPUI 前台 spawn 一个消费者，收到信号后调用 `cx.notify()`。

```rust
// chat/gpui_chat_state.rs

pub struct GpuiChatState {
    messages: Vec<UIMessage>,
    status: ChatStatus,
    error: Option<ChatError>,
    notify_tx: futures::channel::mpsc::UnboundedSender<NotifyKind>,
}

enum NotifyKind {
    Messages,
    Status,
    Error,
}

impl ChatState for GpuiChatState {
    fn messages(&self) -> Vec<UIMessage> {
        self.messages.clone()
    }

    fn set_messages(&mut self, messages: Vec<UIMessage>) {
        self.messages = messages;
    }

    fn push_message(&mut self, message: UIMessage) {
        self.messages.push(message);
    }

    fn pop_message(&mut self) -> Option<UIMessage> {
        self.messages.pop()
    }

    fn replace_message(&mut self, index: usize, message: UIMessage) {
        if index < self.messages.len() {
            self.messages[index] = message;
        }
    }

    fn status(&self) -> ChatStatus { self.status.clone() }
    fn set_status(&mut self, status: ChatStatus) { self.status = status; }

    fn error(&self) -> Option<ChatError> { self.error.clone() }
    fn set_error(&mut self, error: Option<ChatError>) { self.error = error; }

    fn notify_messages_changed(&mut self) {
        let _ = self.notify_tx.unbounded_send(NotifyKind::Messages);
    }
    fn notify_status_changed(&mut self) {
        let _ = self.notify_tx.unbounded_send(NotifyKind::Status);
    }
    fn notify_error_changed(&mut self) {
        let _ = self.notify_tx.unbounded_send(NotifyKind::Error);
    }
}
```

### 4.4 GpuiChat

```rust
// chat/gpui_chat.rs

pub struct GpuiChat {
    inner: AbstractChat<GpuiChatState>,
    runtime: tokio::runtime::Runtime,  // 独立 tokio runtime
}

impl GpuiChat {
    pub fn new(config: GpuiChatConfig, cx: &mut Context<Self>) -> Self {
        // 1. 创建 tokio runtime
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all().build().unwrap();

        // 2. 创建 notify channel
        let (notify_tx, mut notify_rx) = futures::channel::mpsc::unbounded();

        // 3. 创建 GpuiChatState
        let state = GpuiChatState { messages: vec![], ..., notify_tx };

        // 4. 创建 Transport（根据配置选 Direct / HttpSse）
        let transport = match config.transport_kind {
            TransportKind::Direct { .. } => ...,
            TransportKind::HttpSse { url, .. } => ...,
        };

        // 5. 创建 AbstractChat
        let chat = AbstractChat::new(ChatInit {
            id: config.session_id,
            state,
            transport,
            runtime_handle: runtime.handle().clone(),
            ..
        });

        // 6. spawn GPUI 前台任务消费 notify channel → cx.notify()
        cx.spawn(async move |_this, cx| {
            while let Some(kind) = notify_rx.next().await {
                cx.update(|cx| cx.notify()).ok();
            }
        }).detach();

        Self { inner: chat, runtime }
    }

    // 委托方法（通过 runtime.spawn 转发到 tokio）
    pub fn send_message(&self, text: &str, cx: &mut Context<Self>) {
        let chat = self.inner.clone(); // AbstractChat is Clone (Arc inside)
        let parts = vec![UIMessagePart::Text {
            text: text.to_string(),
            state: None,
        }];
        self.runtime.spawn(async move {
            chat.send_message(parts, SendOptions::default()).await;
        });
    }

    pub fn stop(&self) {
        let chat = self.inner.clone();
        self.runtime.spawn(async move { chat.stop().await; });
    }

    pub fn add_tool_approval_response(&self, tool_call_id: &str, approved: bool) {
        let chat = self.inner.clone();
        let id = tool_call_id.to_string();
        self.runtime.spawn(async move {
            chat.add_tool_approval_response(&id, approved).await;
        });
    }

    // 读取方法（直接读 inner.state，无需 async）
    pub fn messages(&self) -> Vec<UIMessage> {
        self.inner.lock_state(|s| s.messages())
    }
    pub fn status(&self) -> ChatStatus {
        self.inner.lock_state(|s| s.status())
    }
}
```

### 4.5 新的 ChatModel

```rust
// models/chat_model.rs (重写)

pub struct ChatModel {
    chats: HashMap<String, Entity<GpuiChat>>,
    drafts: HashMap<String, String>,
}

pub enum ChatModelEvent {
    ChatCreated { session_id: String },
}

impl EventEmitter<ChatModelEvent> for ChatModel {}

impl ChatModel {
    pub fn get_or_create_chat(
        &mut self,
        session_id: &str,
        config: GpuiChatConfig,
        cx: &mut Context<Self>,
    ) -> Entity<GpuiChat> {
        self.chats.entry(session_id.to_string())
            .or_insert_with(|| {
                cx.new(|cx| GpuiChat::new(config, cx))
            })
            .clone()
    }
}
```

### 4.6 Views 改造

**MessageList** — 渲染 `UIMessage.parts`：

```rust
// 伪代码
for message in chat.messages() {
    for part in &message.parts {
        match part {
            UIMessagePart::Text { text, state } => render_text(text, state),
            UIMessagePart::Reasoning { text, state } => render_thinking(text, state),
            UIMessagePart::Tool { id, tool_name, state, output, error, .. } => {
                render_tool_call(tool_name, state);
                if *state == ToolState::ApprovalRequested {
                    render_approval_buttons(id, chat_entity);
                }
                if let Some(output) = output {
                    render_tool_output(output);
                }
                if let Some(error) = error {
                    render_tool_error(error);
                }
            }
            UIMessagePart::File { media_type, data } => render_file(media_type, data),
            UIMessagePart::StepStart => render_step_divider(),
            _ => {}
        }
    }
}
```

**tool_approval.rs** — 内联审批组件：

```rust
// 审批按钮点击后
fn on_approve(tool_call_id: &str, chat: &Entity<GpuiChat>, cx: &mut Context<Self>) {
    chat.update(cx, |chat, _cx| {
        chat.add_tool_approval_response(tool_call_id, true);
    });
}
fn on_deny(tool_call_id: &str, chat: &Entity<GpuiChat>, cx: &mut Context<Self>) {
    chat.update(cx, |chat, _cx| {
        chat.add_tool_approval_response(tool_call_id, false);
    });
}
```

---

## 5. SecurityGuard 集成与工具审批流程

### 5.1 完整审批数据流

```
用户发送消息
  │
  ▼
AbstractChat.send_message()
  → Transport.send_messages()
    → DirectChatTransport: spawns AgentEngine
      │
      ▼
  AgentEngine loop:
    LLM 返回 ToolUse
      │
      ├── SecurityGuard.check_tool_call(tool_name, input)
      │     │
      │     ├── Allowed → 直接执行，emit ToolOutputAvailable
      │     │
      │     └── NeedsApproval →
      │           ├── emit ToolApprovalRequest { id } chunk
      │           ├── 等待 approval_rx.recv() (阻塞当前迭代)
      │           │
      │           │ ─── chunk 流到达 AbstractChat ───
      │           │ ─── consume_stream 处理 ToolApprovalRequest ───
      │           │ ─── 更新 Tool part state → ApprovalRequested ───
      │           │ ─── UI 渲染审批按钮 ───
      │           │
      │           │ ─── 用户点击 Approve ───
      │           │ ─── GpuiChat.add_tool_approval_response(id, true) ───
      │           │ ─── AbstractChat → DirectTransport.approval_tx.send() ───
      │           │
      │           ├── approval_rx 收到 approved=true
      │           │     → 执行工具
      │           │     → emit ToolOutputAvailable { id, output }
      │           │
      │           └── approval_rx 收到 approved=false
      │                 → emit ToolOutputDenied { id }
      │                 → 构造拒绝消息告诉 LLM
      │
      ▼
  继续 agent loop 或 Finish
```

### 5.2 引擎侧改造

```rust
// AgentEngine.execute_tools() 改造伪代码

for call in &tool_calls {
    let decision = security_guard.check_tool_call(&call.name, &call.input);

    match decision {
        SecurityDecision::Allow => {
            // 直接执行
            let result = tools.get(&call.name)?.execute(call.input.clone(), &ctx).await;
            chunk_tx.send(ToolOutputAvailable { id: call.id, output: result.output });
        }
        SecurityDecision::NeedsApproval { reason } => {
            // 发审批请求 chunk
            chunk_tx.send(ToolApprovalRequest { id: call.id });

            // 等待审批响应（从 approval channel）
            let response = approval_rx.lock().await.recv().await;
            match response {
                Some(ApprovalResponse { approved: true, .. }) => {
                    chunk_tx.send(UIMessageChunk::ToolInputAvailable { .. });
                    let result = tools.get(&call.name)?.execute(...).await;
                    chunk_tx.send(ToolOutputAvailable { id, output });
                }
                Some(ApprovalResponse { approved: false, .. }) => {
                    chunk_tx.send(ToolOutputDenied { id: call.id });
                }
                None => {
                    // channel closed, abort
                    chunk_tx.send(Abort);
                    return;
                }
            }
        }
        SecurityDecision::Deny { reason } => {
            chunk_tx.send(ToolOutputDenied { id: call.id });
        }
    }
}
```

---

## 6. AgentEngine 改造

### 6.1 输出改为 UIMessageChunk

```rust
pub struct AgentEngine {
    // ...
    chunk_tx: mpsc::Sender<UIMessageChunk>,  // 替代 event_tx: mpsc::Sender<EngineEvent>
    cancel_rx: watch::Receiver<bool>,        // 不变
    security_guard: Option<SecurityGuard>,    // 新增
}
```

### 6.2 发射 chunk 的时机

| 原 EngineEvent | 新 UIMessageChunk 序列 |
|---|---|
| （循环开始） | `Start { message_id }` |
| `ThinkingDelta` | `ReasoningStart` → `ReasoningDelta*` → `ReasoningEnd` |
| `TextDelta` | `TextStart` → `TextDelta*` → `TextEnd` |
| `ToolCallStarted` | `FinishStep` + `ToolInputStart` |
| （工具输入构建中） | `ToolInputDelta*` |
| （工具输入完成） | `ToolInputAvailable` |
| `SecurityGuard.NeedsApproval` | `ToolApprovalRequest` |
| `ToolCallCompleted` (success) | `ToolOutputAvailable` |
| `ToolCallCompleted` (error) | `ToolOutputError` |
| （审批拒绝） | `ToolOutputDenied` |
| `TokenUsage` | `TokenUsage` chunk |
| `Completed` | `Finish { finish_reason: Stop }` |
| `Error` | `Error` |

---

## 7. 错误处理

```rust
// alva-app-core

#[derive(Debug, Clone, thiserror::Error)]
pub enum ChatError {
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Stream error: {0}")]
    Stream(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Engine error: {0}")]
    Engine(String),
}

// srow-ai

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Engine error: {0}")]
    Engine(#[from] EngineError),
    #[error("Connection refused")]
    ConnectionRefused,
}

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Invalid SSE: {0}")]
    InvalidSse(String),
    #[error("Invalid chunk JSON: {0}")]
    InvalidChunk(String),
    #[error("Stream interrupted")]
    Interrupted,
    #[error("Aborted")]
    Aborted,
}
```

> **注意**：`ChatError` 需要 `Clone`（因为 `ChatState::error()` 返回 owned），
> 所以不直接 wrap 其他 error 类型，而是存 String 描述。

---

## 8. 与现有模块的关系

### 保留不变
- `alva-app-core::domain::message::LLMMessage` — LLM API 通信用
- `alva-app-core::ports::*` — LLMProvider, Tool, SessionStorage trait
- `alva-app-core::adapters::*` — OpenAICompatProvider, MemoryStorage
- `alva-app-core::agent::runtime::tools::*` — 所有工具实现
- `alva-app-core::agent::runtime::security::*` — 安全层
- `alva-app-core::mcp::*` — MCP 协议层
- `alva-app-core::skills::*` — 技能系统
- `alva-app-core::environment::*` — 环境管理

### 改造
- `alva-app-core::agent::runtime::engine::engine` — `EngineEvent` → `UIMessageChunk`，集成 SecurityGuard
- `alva-app::models::chat_model` — 重写
- `alva-app::engine_bridge` — 删除
- `alva-app::types::message` — 删除
- `alva-app::views::chat_panel::*` — 基于新模型改造

### 新增
- `alva-app-core::ui_message` — 数据模型
- `alva-app-core::ui_message_stream` — 流协议 + 处理逻辑
- `srow-ai` — 整个新 crate
- `alva-app::chat` — GPUI Chat 绑定

---

## 9. 新增依赖

### srow-ai/Cargo.toml
```toml
[package]
name = "srow-ai"
version = "0.1.0"
edition = "2021"
description = "Srow AI interaction layer — chat, transport, completion, object generation"

[dependencies]
alva-app-core = { path = "../alva-app-core" }
async-trait = "0.1"
tokio = { version = "1", features = ["sync", "rt"] }
futures = "0.3"
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
pin-project-lite = "0.2"
uuid = { version = "1", features = ["v4"] }
thiserror = "2"
tracing = "0.1"
tokio-stream = "0.1"
```

### alva-app-core 新增依赖
无新增（已有 serde, serde_json, tokio, futures, bytes）。

---

## 10. 测试策略

| 层 | 测试类型 | 关注点 |
|---|---|---|
| `alva-app-core::ui_message` | 单元测试 | serde round-trip, UIMessage ↔ LLMMessage 转换（含 Tool parts → Role::Tool） |
| `alva-app-core::ui_message_stream` | 单元测试 | process_ui_message_stream 对各种 chunk 序列的状态构建 |
| `alva-app-core::ui_message_stream::sse` | 单元测试 | SSE 文本解析为 UIMessageChunk |
| `srow-ai::chat` | 集成测试 | AbstractChat + MockTransport 的完整交互（发送、流式、工具、审批） |
| `srow-ai::transport::direct` | 集成测试 | DirectTransport + 真实 AgentEngine 端到端 |
| `srow-ai::transport::http_sse` | 集成测试 | HttpSseTransport + mock HTTP server |
| `srow-ai::util` | 单元测试 | SerialJobExecutor 串行保证, AbortHandle 取消语义 |
| `alva-app` | 手动测试 | GPUI 渲染 + 交互 |
