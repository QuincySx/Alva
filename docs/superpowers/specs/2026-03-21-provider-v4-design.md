# Provider V4 完整对齐设计

> 将 alva-app-core 的 provider 接口完全对齐 AI SDK `@ai-sdk/provider` V4 规格，去掉 rig-core 依赖，实现 Anthropic + OpenAI provider。

---

## 1. 概述

**一句话**：把 `ports/` 和 `adapters/llm/` 整个重写，1:1 翻译 `@ai-sdk/provider` 的所有 V4 类型到 Rust，然后实现 Anthropic 和 OpenAI 两个 provider。

**改动规模**：
- 重写 `ports/llm_provider.rs` → 拆分为多个文件的 `ports/provider/` 模块
- 重写 `domain/message.rs` 中的 LLMContent
- 删除 `adapters/llm/openai_compat.rs`（rig 包装）
- 新建 `adapters/llm/openai.rs` + `adapters/llm/anthropic.rs` + `adapters/llm/http.rs`
- 去掉 `rig-core` 依赖
- 更新所有消费方（engine, generate_text, stream_text, cli, app）

---

## 2. 新的 ports/provider/ 模块结构

```
src/alva-app-core/src/ports/provider/
├── mod.rs                    # 所有 re-exports
├── language_model.rs         # LanguageModelV4 trait + CallOptions + Result + StreamPart
├── embedding_model.rs        # EmbeddingModelV4 trait
├── image_model.rs            # ImageModelV4 trait
├── speech_model.rs           # SpeechModelV4 trait
├── transcription_model.rs    # TranscriptionModelV4 trait
├── video_model.rs            # VideoModelV4 trait
├── reranking_model.rs        # RerankingModelV4 trait
├── provider_registry.rs      # ProviderV4 trait（工厂接口）
├── middleware.rs             # LanguageModelV4Middleware 等
├── types.rs                  # 共享类型 (Warning, ProviderMetadata, ProviderOptions, Headers)
├── content.rs                # LanguageModelV4Content + 所有 Part 类型
├── prompt.rs                 # LanguageModelV4Prompt + Message + 所有输入 Part
├── errors.rs                 # 14 种错误类型
└── tool_types.rs             # FunctionTool, ProviderTool, ToolChoice, ToolResult 等
```

---

## 3. 核心类型翻译

### 3.1 共享类型 (`types.rs`)

```rust
pub type ProviderHeaders = HashMap<String, String>;
pub type ProviderMetadata = HashMap<String, serde_json::Map<String, serde_json::Value>>;
pub type ProviderOptions = HashMap<String, serde_json::Map<String, serde_json::Value>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderWarning {
    Unsupported { feature: String, details: Option<String> },
    Compatibility { feature: String, details: Option<String> },
    Other { message: String },
}
```

### 3.2 LanguageModelV4 trait (`language_model.rs`)

```rust
#[async_trait]
pub trait LanguageModel: Send + Sync {
    fn specification_version(&self) -> &str { "v4" }
    fn provider(&self) -> &str;
    fn model_id(&self) -> &str;

    async fn do_generate(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError>;

    async fn do_stream(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, ProviderError>;
}
```

### 3.3 CallOptions (`language_model.rs`)

```rust
pub struct LanguageModelCallOptions {
    pub prompt: Vec<LanguageModelMessage>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop_sequences: Option<Vec<String>>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub response_format: Option<ResponseFormat>,
    pub seed: Option<u64>,
    pub tools: Option<Vec<LanguageModelTool>>,
    pub tool_choice: Option<ToolChoice>,
    pub reasoning: Option<ReasoningLevel>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

#[derive(Debug, Clone)]
pub enum ResponseFormat {
    Text,
    Json {
        schema: Option<serde_json::Value>,
        name: Option<String>,
        description: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum ReasoningLevel {
    ProviderDefault,
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}
```

### 3.4 Prompt 消息类型 (`prompt.rs`)

```rust
#[derive(Debug, Clone)]
pub enum LanguageModelMessage {
    System {
        content: String,
        provider_options: Option<ProviderOptions>,
    },
    User {
        content: Vec<UserContentPart>,
        provider_options: Option<ProviderOptions>,
    },
    Assistant {
        content: Vec<AssistantContentPart>,
        provider_options: Option<ProviderOptions>,
    },
    Tool {
        content: Vec<ToolContentPart>,
        provider_options: Option<ProviderOptions>,
    },
}

// User message parts
#[derive(Debug, Clone)]
pub enum UserContentPart {
    Text { text: String, provider_options: Option<ProviderOptions> },
    File { data: DataContent, media_type: String, filename: Option<String>, provider_options: Option<ProviderOptions> },
}

// Assistant message parts
#[derive(Debug, Clone)]
pub enum AssistantContentPart {
    Text { text: String, provider_options: Option<ProviderOptions> },
    File { data: DataContent, media_type: String, provider_options: Option<ProviderOptions> },
    Reasoning { text: String, provider_options: Option<ProviderOptions> },
    ReasoningFile { data: DataContent, media_type: String, provider_options: Option<ProviderOptions> },
    ToolCall { tool_call_id: String, tool_name: String, input: serde_json::Value, provider_options: Option<ProviderOptions> },
    ToolResult { tool_call_id: String, tool_name: String, output: ToolResultOutput, provider_options: Option<ProviderOptions> },
    Custom { kind: String, provider_options: Option<ProviderOptions> },
}

// Tool message parts
#[derive(Debug, Clone)]
pub enum ToolContentPart {
    ToolResult { tool_call_id: String, tool_name: String, output: ToolResultOutput, provider_options: Option<ProviderOptions> },
    ToolApprovalResponse { approval_id: String, approved: bool, reason: Option<String>, provider_options: Option<ProviderOptions> },
}

#[derive(Debug, Clone)]
pub enum DataContent {
    Bytes(Vec<u8>),
    Base64(String),
    Url(String),
}

#[derive(Debug, Clone)]
pub enum ToolResultOutput {
    Text { value: String },
    Json { value: serde_json::Value },
    ExecutionDenied { reason: Option<String> },
    ErrorText { value: String },
    ErrorJson { value: serde_json::Value },
    Content { value: Vec<ToolResultContentItem> },
}

#[derive(Debug, Clone)]
pub enum ToolResultContentItem {
    Text { text: String },
    FileData { data: String, media_type: String, filename: Option<String> },
    FileUrl { url: String },
    ImageData { data: String, media_type: String },
    ImageUrl { url: String },
    Custom { provider_options: Option<ProviderOptions> },
}
```

### 3.5 输出内容类型 (`content.rs`)

```rust
#[derive(Debug, Clone)]
pub enum LanguageModelContent {
    Text { text: String, provider_metadata: Option<ProviderMetadata> },
    Reasoning { text: String, provider_metadata: Option<ProviderMetadata> },
    File { media_type: String, data: Vec<u8>, provider_metadata: Option<ProviderMetadata> },
    ReasoningFile { media_type: String, data: Vec<u8>, provider_metadata: Option<ProviderMetadata> },
    ToolCall { tool_call_id: String, tool_name: String, input: String, provider_metadata: Option<ProviderMetadata> },
    ToolResult { tool_call_id: String, tool_name: String, result: serde_json::Value, is_error: Option<bool>, provider_metadata: Option<ProviderMetadata> },
    ToolApprovalRequest { approval_id: String, tool_call_id: String, provider_metadata: Option<ProviderMetadata> },
    Source(LanguageModelSource),
    Custom { kind: String, provider_metadata: Option<ProviderMetadata> },
}

#[derive(Debug, Clone)]
pub enum LanguageModelSource {
    Url { id: String, url: String, title: Option<String>, provider_metadata: Option<ProviderMetadata> },
    Document { id: String, media_type: String, title: String, filename: Option<String>, provider_metadata: Option<ProviderMetadata> },
}
```

### 3.6 工具类型 (`tool_types.rs`)

```rust
#[derive(Debug, Clone)]
pub enum LanguageModelTool {
    Function(FunctionTool),
    Provider(ProviderTool),
}

#[derive(Debug, Clone)]
pub struct FunctionTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value, // JSON Schema
    pub strict: Option<bool>,
    pub provider_options: Option<ProviderOptions>,
}

#[derive(Debug, Clone)]
pub struct ProviderTool {
    pub id: String,     // "provider.tool_name" 格式
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool { tool_name: String },
}
```

### 3.7 GenerateResult (`language_model.rs`)

```rust
pub struct LanguageModelGenerateResult {
    pub content: Vec<LanguageModelContent>,
    pub finish_reason: FinishReason,
    pub usage: LanguageModelUsage,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
    pub response: Option<ResponseMetadata>,
}

#[derive(Debug, Clone)]
pub struct FinishReason {
    pub unified: UnifiedFinishReason,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnifiedFinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
    Error,
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct LanguageModelUsage {
    pub input_tokens: UsageInputTokens,
    pub output_tokens: UsageOutputTokens,
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageInputTokens {
    pub total: Option<u32>,
    pub no_cache: Option<u32>,
    pub cache_read: Option<u32>,
    pub cache_write: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageOutputTokens {
    pub total: Option<u32>,
    pub text: Option<u32>,
    pub reasoning: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ResponseMetadata {
    pub id: Option<String>,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub model_id: Option<String>,
    pub headers: Option<ProviderHeaders>,
}
```

### 3.8 StreamPart (`language_model.rs`)

```rust
#[derive(Debug, Clone)]
pub enum LanguageModelStreamPart {
    // Text
    TextStart { id: String },
    TextDelta { id: String, delta: String },
    TextEnd { id: String },
    // Reasoning
    ReasoningStart { id: String },
    ReasoningDelta { id: String, delta: String },
    ReasoningEnd { id: String },
    // Tool input
    ToolInputStart { id: String, tool_name: String, title: Option<String> },
    ToolInputDelta { id: String, delta: String },
    ToolInputEnd { id: String },
    // Tool objects
    ToolCall(LanguageModelContent), // reuse ToolCall variant
    ToolResult(LanguageModelContent),
    ToolApprovalRequest(LanguageModelContent),
    // Files and sources
    File(LanguageModelContent),
    ReasoningFile(LanguageModelContent),
    Source(LanguageModelSource),
    Custom(LanguageModelContent),
    // Control
    StreamStart { warnings: Vec<ProviderWarning> },
    ResponseMetadata(ResponseMetadata),
    Finish { usage: LanguageModelUsage, finish_reason: FinishReason, provider_metadata: Option<ProviderMetadata> },
    // Raw / error
    Raw { value: serde_json::Value },
    Error { error: String },
}
```

### 3.9 StreamResult

```rust
pub struct LanguageModelStreamResult {
    pub stream: Pin<Box<dyn Stream<Item = LanguageModelStreamPart> + Send>>,
    pub response: Option<ResponseMetadata>,
}
```

### 3.10 其他模型 trait

每个模型 trait 独立文件，对齐 AI SDK：

```rust
// embedding_model.rs
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    fn specification_version(&self) -> &str { "v4" }
    fn provider(&self) -> &str;
    fn model_id(&self) -> &str;
    fn max_embeddings_per_call(&self) -> Option<u32>;
    fn supports_parallel_calls(&self) -> bool;
    async fn do_embed(&self, options: EmbeddingModelCallOptions) -> Result<EmbeddingModelResult, ProviderError>;
}

// image_model.rs — ImageModel trait
// speech_model.rs — SpeechModel trait
// transcription_model.rs — TranscriptionModel trait
// video_model.rs — VideoModel trait
// reranking_model.rs — RerankingModel trait
```

### 3.11 ProviderV4 工厂 (`provider_registry.rs`)

```rust
pub trait Provider: Send + Sync {
    fn specification_version(&self) -> &str { "v4" }
    fn language_model(&self, model_id: &str) -> Result<Box<dyn LanguageModel>, ProviderError>;
    fn embedding_model(&self, model_id: &str) -> Result<Box<dyn EmbeddingModel>, ProviderError>;
    fn image_model(&self, model_id: &str) -> Result<Box<dyn ImageModel>, ProviderError>;
    // Optional
    fn transcription_model(&self, _model_id: &str) -> Result<Box<dyn TranscriptionModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("transcription".into()))
    }
    fn speech_model(&self, _model_id: &str) -> Result<Box<dyn SpeechModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("speech".into()))
    }
    fn reranking_model(&self, _model_id: &str) -> Result<Box<dyn RerankingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("reranking".into()))
    }
}
```

### 3.12 错误类型 (`errors.rs`)

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    #[error("API call error: HTTP {status_code}: {message}")]
    ApiCall { message: String, url: String, status_code: Option<u16>, response_body: Option<String>, is_retryable: bool },
    #[error("Empty response body")]
    EmptyResponseBody,
    #[error("Invalid argument '{argument}': {message}")]
    InvalidArgument { argument: String, message: String },
    #[error("Invalid prompt: {message}")]
    InvalidPrompt { message: String },
    #[error("Invalid response data: {message}")]
    InvalidResponseData { message: String },
    #[error("JSON parse error: {message}")]
    JsonParse { message: String, text: String },
    #[error("API key error: {message}")]
    LoadApiKey { message: String },
    #[error("Setting error: {message}")]
    LoadSetting { message: String },
    #[error("No content generated")]
    NoContentGenerated,
    #[error("No such {model_type}: {model_id}")]
    NoSuchModel { model_id: String, model_type: String },
    #[error("Too many embedding values: {count} > {max}")]
    TooManyEmbeddingValues { count: usize, max: usize },
    #[error("Type validation error: {message}")]
    TypeValidation { message: String },
    #[error("Unsupported: {functionality}")]
    UnsupportedFunctionality(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Rate limited")]
    RateLimited { retry_after_ms: Option<u64> },
}
```

---

## 4. Adapters — Provider 实现

### 4.1 新的 adapters/llm/ 结构

```
src/alva-app-core/src/adapters/llm/
├── mod.rs
├── http.rs              # 共用 HTTP 层（SSE 解析、重试）
├── openai.rs            # OpenAI Chat Completions
└── anthropic.rs         # Anthropic Messages API
```

### 4.2 每个 provider 实现 `LanguageModel` trait

```rust
// openai.rs
pub struct OpenAILanguageModel { ... }
impl LanguageModel for OpenAILanguageModel { ... }

// anthropic.rs
pub struct AnthropicLanguageModel { ... }
impl LanguageModel for AnthropicLanguageModel { ... }
```

---

## 5. 消费方适配

所有引用旧 `LLMProvider`/`LLMRequest`/`LLMResponse`/`StreamChunk` 的代码都需要迁移：

| 消费方 | 改动 |
|--------|------|
| `agent/runtime/engine/engine.rs` | `LLMProvider` → `LanguageModel`, `LLMRequest` → `LanguageModelCallOptions`, `LLMResponse` → `LanguageModelGenerateResult`, `StreamChunk` → `LanguageModelStreamPart` |
| `generate/generate_text.rs` | 同上 |
| `generate/stream_text.rs` | 同上 |
| `bin/cli.rs` | 更新 provider 构造 |
| `alva-app` views | 更新 provider 构造 |
| `ui_message/convert.rs` | `LLMMessage`/`LLMContent` → 用新的 `LanguageModelMessage`/`LanguageModelContent` 或添加转换层 |

### 关于 LLMMessage 的处理

有两种策略：

**策略 A**：保留 `domain/message.rs` 的 `LLMMessage`/`LLMContent` 作为内部会话历史格式，在 engine 和 provider 之间做转换。

**策略 B**：直接用 `LanguageModelMessage` 替代 `LLMMessage`，删除旧的 domain/message.rs。

**选择策略 B** — AI SDK 内部也是直接用 `LanguageModelV4Message` 作为消息格式，没有中间层。这样最干净。

但 `LanguageModelMessage` 比 `LLMMessage` 复杂很多（多种 Part 类型），存储层 (`SessionStorage`) 和序列化会更复杂。所以实际做法是：

**策略 C（推荐）**：保留 `LLMMessage` 用于存储/序列化（简单扁平格式），在调用 provider 时做 `LLMMessage ↔ LanguageModelMessage` 转换。这和 AI SDK 的做法一致——AI SDK 内部有 `standardizePrompt()` 和 `convertToLanguageModelPrompt()` 两层转换。

---

## 6. 删除旧类型

| 删除 | 替代 |
|------|------|
| `ports/llm_provider.rs` 整个文件 | `ports/provider/` 模块 |
| `LLMProvider` trait | `LanguageModel` trait |
| `LLMRequest` | `LanguageModelCallOptions` |
| `LLMResponse` | `LanguageModelGenerateResult` |
| `StreamChunk` | `LanguageModelStreamPart` |
| `StopReason` | `FinishReason` |
| `TokenUsage` (in llm_provider) | `LanguageModelUsage` |
| `ToolChoice` (in llm_provider) | `ToolChoice` (in tool_types) |
| `ResponseFormat` (in llm_provider) | `ResponseFormat` (in language_model) |
| `adapters/llm/openai_compat.rs` | `adapters/llm/openai.rs` + `adapters/llm/anthropic.rs` |
| `rig-core` 依赖 | `reqwest` 直接 HTTP |
