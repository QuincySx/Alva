# 去掉 rig-core，自建 OpenAI + Anthropic Provider

> 用直接 HTTP 调用替代 rig-core 依赖，对齐 AI SDK 的 provider 设计

---

## 1. 改动范围

### 删除
- `src/alva-app-core/src/adapters/llm/openai_compat.rs` (593 行 rig 包装)
- `Cargo.toml` 中的 `rig-core` 依赖

### 新建
- `src/alva-app-core/src/adapters/llm/http.rs` — 共用 HTTP 工具（SSE 解析、重试、错误处理）
- `src/alva-app-core/src/adapters/llm/openai.rs` — OpenAI Chat Completions API 直接实现
- `src/alva-app-core/src/adapters/llm/anthropic.rs` — Anthropic Messages API 直接实现

### 修改
- `src/alva-app-core/src/adapters/llm/mod.rs` — 替换模块声明
- `src/alva-app-core/Cargo.toml` — 去 rig-core，确保 reqwest + serde 足够
- `src/alva-app-core/src/bin/cli.rs` — 更新 provider 构造
- `src/alva-app/src/views/chat_panel/input_box.rs` — 更新 provider 构造

---

## 2. 共用 HTTP 层 (`http.rs`)

```rust
/// Provider-level SSE 事件（不同于 ui_message_stream 的 UIMessageChunk SSE）
/// 这是原始的 SSE 事件，包含 event type 和 data
pub struct SseEvent {
    pub event: Option<String>,  // Anthropic 用 "message_start" 等，OpenAI 没有
    pub data: String,
}

/// 解析原始 SSE 字节流为 SseEvent 流
pub fn parse_raw_sse(
    byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send,
) -> impl Stream<Item = Result<SseEvent, ProviderError>> + Send

/// 带重试的 POST 请求
pub async fn post_json_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    body: &serde_json::Value,
    max_retries: u32,
) -> Result<reqwest::Response, ProviderError>

/// Provider 级别错误
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP {status}: {body}")]
    HttpError { status: u16, body: String },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON parse error: {0}")]
    JsonParse(String),
    #[error("SSE parse error: {0}")]
    SseParse(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },
}
```

关键：这个 SSE 解析和 `ui_message_stream/sse.rs` **不同**。那个解析的是 UIMessageChunk JSON。这个解析的是 provider 原始 SSE 事件（OpenAI/Anthropic 各有不同格式）。

---

## 3. OpenAI Provider (`openai.rs`)

对标 `@ai-sdk/openai` 的 Chat Completions 部分。

### 3.1 构造

```rust
pub struct OpenAIProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,       // 默认 "https://api.openai.com/v1"
    model: String,
    default_headers: Vec<(String, String)>,
}

impl OpenAIProvider {
    pub fn new(api_key: &str, model: &str) -> Self
    pub fn with_base_url(api_key: &str, base_url: &str, model: &str) -> Self
}
```

### 3.2 请求格式（LLMRequest → OpenAI JSON）

```json
{
  "model": "gpt-4o",
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": [
      {"type": "text", "text": "describe this"},
      {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
    ]},
    {"role": "assistant", "content": "...", "tool_calls": [...]},
    {"role": "tool", "tool_call_id": "...", "content": "..."}
  ],
  "tools": [{"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}],
  "tool_choice": "auto" | "none" | "required" | {"type": "function", "function": {"name": "..."}},
  "max_tokens": 8192,
  "temperature": 0.7,
  "top_p": 0.9,
  "frequency_penalty": 0.0,
  "presence_penalty": 0.0,
  "stop": ["..."],
  "seed": 42,
  "response_format": {"type": "text"} | {"type": "json_schema", "json_schema": {"name": "...", "schema": {...}}},
  "stream": true,
  "stream_options": {"include_usage": true}
}
```

转换逻辑：
- `LLMRequest.system` → messages 数组第一条 `{"role": "system"}`
- `LLMContent::Text` → `{"type": "text", "text": "..."}`
- `LLMContent::Image` → `{"type": "image_url", "image_url": {"url": "..."}}`（URL）或 `{"url": "data:{media_type};base64,{data}"}`（base64）
- `LLMContent::ToolUse` → assistant message 的 `tool_calls` 数组
- `LLMContent::ToolResult` → `{"role": "tool", "tool_call_id": "...", "content": "..."}`
- `LLMContent::Reasoning` → 跳过（OpenAI 没有）
- `ToolChoice::Auto` → `"auto"`, `None` → `"none"`, `Required` → `"required"`, `Tool(name)` → specific
- `ResponseFormat::Json { schema }` → `{"type": "json_schema", ...}`

### 3.3 响应解析（OpenAI JSON → LLMResponse）

非流式：
```json
{
  "choices": [{"message": {"role": "assistant", "content": "...", "tool_calls": [...]}, "finish_reason": "stop"}],
  "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
}
```

流式 SSE：
```
data: {"choices":[{"delta":{"content":"Hello"}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_xxx","function":{"name":"search","arguments":""}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"q\":"}}]}}]}
data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20}}
data: [DONE]
```

解析为 `StreamChunk`：
- `delta.content` → `StreamChunk::TextDelta`
- `delta.tool_calls[i]` (首次出现 id) → 记录新 tool call
- `delta.tool_calls[i].function.arguments` → `StreamChunk::ToolCallDelta`
- `finish_reason == "stop"` → 构建 `StreamChunk::Done(LLMResponse)`
- `usage` → 提取 token 统计放入 Done

---

## 4. Anthropic Provider (`anthropic.rs`)

对标 `@ai-sdk/anthropic` 的 Messages API 部分。

### 4.1 构造

```rust
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,       // 默认 "https://api.anthropic.com/v1"
    model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, model: &str) -> Self
    pub fn with_base_url(api_key: &str, base_url: &str, model: &str) -> Self
}
```

### 4.2 请求格式（LLMRequest → Anthropic JSON）

```json
{
  "model": "claude-sonnet-4-5",
  "max_tokens": 8192,
  "system": [{"type": "text", "text": "..."}],
  "messages": [
    {"role": "user", "content": [
      {"type": "text", "text": "describe this"},
      {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "..."}}
    ]},
    {"role": "assistant", "content": [
      {"type": "text", "text": "..."},
      {"type": "tool_use", "id": "toolu_xxx", "name": "search", "input": {...}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_xxx", "content": "..."}
    ]}
  ],
  "tools": [{"name": "...", "description": "...", "input_schema": {...}}],
  "tool_choice": {"type": "auto"},
  "temperature": 0.7,
  "top_k": 40,
  "stop_sequences": ["..."],
  "stream": true
}
```

**关键差异**：
- system 是顶级字段，是 content block 数组（不是字符串）
- tool result 在 user message 中（不是独立 role=tool 消息）
- 连续同角色消息必须合并
- tool schema 字段叫 `input_schema`（不是 `parameters`）
- 图片格式不同（`source.type` = "base64"/"url"）
- 无 frequency_penalty/presence_penalty/seed

### 4.3 消息合并逻辑

Anthropic 要求严格 user/assistant 交替。转换时：
1. 连续的 user + tool_result 消息 → 合并为一个 user 消息
2. 连续的 assistant 消息 → 合并为一个 assistant 消息
3. 第一条消息必须是 user

### 4.4 响应解析

非流式直接解析 JSON body。

流式 SSE（Anthropic 用 named events）：
```
event: message_start
data: {"type":"message_start","message":{"id":"msg_xxx","model":"claude-sonnet-4-5","usage":{"input_tokens":25}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}

event: message_stop
data: {"type":"message_stop"}
```

解析为 `StreamChunk`：
- `content_block_delta` + `text_delta` → `StreamChunk::TextDelta`
- `content_block_delta` + `thinking_delta` → `StreamChunk::ThinkingDelta`
- `content_block_start` + `tool_use` → 记录新 tool call
- `content_block_delta` + `input_json_delta` → `StreamChunk::ToolCallDelta`
- `message_delta` (stop_reason) + `message_stop` → `StreamChunk::Done`
- `message_start` usage (input) + `message_delta` usage (output) → TokenUsage

### 4.5 Thinking 支持

当 `LLMRequest` 有 `provider_options` 包含 thinking 配置时：
```json
{"thinking": {"type": "enabled", "budget_tokens": 10000}}
```

添加到请求 body。响应中的 `thinking` block 转为 `LLMContent::Reasoning`。

### 4.6 cache_control 支持

通过 `provider_options` 传递：
```json
{"cache_control": {"type": "ephemeral"}}
```

应用到 system message 和最后一个 user message 的最后一个 content block。

---

## 5. 测试策略

| 测试 | 方式 |
|------|------|
| 消息格式转换 | 单元测试 — LLMRequest → JSON → 验证结构 |
| 响应解析 | 单元测试 — 固定 JSON → LLMResponse → 验证字段 |
| SSE 流解析 | 单元测试 — 固定 SSE 文本 → StreamChunk 序列 |
| Token usage 提取 | 单元测试 |
| 消息合并（Anthropic） | 单元测试 |
| 图片内容转换 | 单元测试 |
| HTTP 集成 | 手动测试（需要真实 API key） |
