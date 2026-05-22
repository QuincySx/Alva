# alva-llm-wire + 协议网关 设计文档

> 状态：草案（待 review）
> 日期：2026-05-22
> 参考：[ZhiYi-R/moon-bridge](https://github.com/ZhiYi-R/moon-bridge)（Go 协议转换网关）

---

## 1. 背景与目标

我们已经有一套**客户端侧**的 LLM 线格式转换层（`alva-kernel-abi::adapter::ToolAdapter`，4 个 provider：anthropic / openai-chat / openai-responses / gemini），但它只做两个方向：

- 出站：规范化类型 → provider 请求（`encode_messages` / `encode_tools`）
- 入站响应：provider 响应 → 规范化类型（`decode_response` / `decode_stream_event`）

它**不能**作为网关使用——缺少"接收外部请求 → 翻译"的另一半（moon-bridge 的 `ClientAdapter`），也没有 HTTP server / 别名路由。

本设计要达成两件事：

1. **可对外提供的转换库**：把转换层抽成独立 crate `alva-llm-wire`，只依赖 serde，与 agent 框架解耦，别人 `cargo add alva-llm-wire` 即可拿到 4 协议的**双向**转换。
2. **协议网关**：新建 `alva-app-gateway`，对外暴露 OpenAI Responses / OpenAI Chat / Anthropic Messages 三种入站协议，按 model 别名路由到任意上游 provider 并在协议间翻译——典型场景：某上游只支持 Chat Completions，客户端用 GPT 新模式（Responses API）打进来，网关翻译转发。

### 设计取舍记录（为什么是这套）

- **复用 `Message` 而非新造 `CoreRequest`**：moon-bridge 在 Go 里没有共享中间层，被迫造了 `CoreMessage`/`CoreRequest`。我们的 `Message`/`ContentBlock`/`StreamEvent` 本就是全框架共用的中间表示，下沉它即可做到**单一表示、无映射层**。
- **不全量重写现有 adapter**：出站逻辑（每家 provider 的线格式怪癖）已验证、有测试，保留。只补对称的入站半边 + 改名 + 抽 crate。
- **与可执行 `Tool` trait 解耦**：转换只需 `name/description/parameters_schema`，即已有的纯值类型 `ToolDefinition`。把 `encode_tools` 的入参从 `&[&dyn Tool]` 改成 `&[ToolDefinition]`，转换层就不再拖 bus / `ToolExecutionContext` / `ToolFs`，才能独立发布。

---

## 2. 非目标（YAGNI）

- **不做** Gemini 入站（没有客户端用 Gemini 原生格式打我们；Gemini 仍是出站-only 上游）。
- **不做** moon-bridge 那套 `CorePluginHooks` 插件体系（deepseek thinking 重放、apply_patch 代理展开、websearch/visual 扩展）。先把核心管线打通；钩子留待后续按需。
- **不做** 计费 / 配额 / 多租户鉴权。网关 v1 只做协议翻译 + 路由 + 转发。
- **不在网关里执行工具**。网关是纯中转：把入站请求里的 tool 定义透传给上游，把上游回吐的 tool_call 透传回客户端。

---

## 3. 术语

| 术语 | 含义 |
|------|------|
| 入站协议 (inbound) | 客户端用来**发请求进来**的线格式：`/v1/responses`、`/v1/chat/completions`、`/v1/messages` |
| 上游协议 (upstream) | 网关**转发出去**用的线格式（同样四选一） |
| 规范化类型 | `Message` / `ContentBlock` / `StreamEvent` / `ToolDefinition` / `ModelConfig`——转换层的中立中间表示（即本设计要下沉的 wire 类型） |
| 别名 (alias) | 客户端传入的 `model` 字段值；网关据此查路由表决定上游 |

---

## 4. 架构总览

依赖只能往下指（遵守 AGENTS.md Rule 17：SDK 不得依赖 app/host）：

```
NEW  alva-llm-wire        L1 基础库 · 依赖仅 serde / serde_json（按需 schemars）· wasm-clean
      ├─ 类型: Message / ContentBlock / StreamEvent / UsageMetadata
      │        ToolDefinition / ModelConfig / ReasoningEffort
      ├─ trait ProtocolAdapter (4 协议 × 双向)
      └─ 适配器: anthropic / openai_chat / openai_responses / gemini
        ▲ depends + re-export      ▲ depends                ▲ depends
   alva-kernel-abi (SDK, L1)   alva-llm-provider (L5)   alva-app-gateway (L6, 新)
   · re-export wire 全部类型     · provider HTTP 外壳复用    · axum HTTP server
   · Tool trait 留在这里         · encode_tools 传            · 3 个入站路由
     (新增 definition() 桥接)      ToolDefinition (改一行)     · AliasRouter
   · LanguageModel/Provider     · 新增 AliasRouter           · RawTool 透传
     /ProviderRegistry 留这里                                · 瘦二进制 + 库
        ▲                                                      ▲ 可被
   全框架其余 crate (靠 re-export 不改)              alva-app-tauri / cli embed
```

**外部用户视角**：只依赖 `alva-llm-wire`，得到 4 协议双向转换 + 全部中立类型，零 agent 框架包袱。

**Rule 17 校验**：`alva-llm-wire` 无 app/host 依赖；`alva-app-gateway` 是 L6 应用层，依赖 L5 `alva-llm-provider`（native, reqwest）合规。`alva-llm-wire` 必须进 `scripts/ci-check-deps.sh` 的 wasm32-clean 名单。

---

## 5. 组件设计

### 5.1 `alva-llm-wire`（新 crate）

**Cargo 依赖**：`serde`（derive）、`serde_json`；仅当下沉的某个类型确有 `#[derive(JsonSchema)]` 时才加 `schemars`。**不得**依赖 `alva-kernel-bus` / `alva-macros` / 任何框架 crate（搬运阶段需确认 `ToolDefinition` 等是否带 schemars 派生，有则一并迁入并保持 wire crate 自洽）。

**下沉的类型**（从 `alva-kernel-abi` 移入，原处改为 `pub use alva_llm_wire::...` re-export）：

- `base/content.rs` → `ContentBlock`（及其内部类型）
- `base/message.rs` → `Message` / `MessageRole` / `UsageMetadata`
- `base/stream.rs` → `StreamEvent`
- `model` 中的 `ModelConfig` / `ReasoningEffort`
- `tool/types.rs` 中的 `ToolDefinition`（纯值类型 `{ name, description, parameters }`）

> 这些类型当前的耦合：`content` 无内部依赖；`message` → `content`；`stream` → `message`；`ModelConfig`/`ReasoningEffort`/`ToolDefinition` 仅依赖 serde。故整体可干净下沉。**留在 kernel-abi 不动**的是：`Tool` trait（可执行，依赖 bus/执行环境）、`LanguageModel`/`Provider`/`ProviderRegistry`、`AgentError`、session/scope/runtime 等框架契约。

**`ProtocolAdapter` trait**（由现 `ToolAdapter` 改名 + 扩充）：

```rust
pub trait ProtocolAdapter: Send + Sync {
    fn protocol(&self) -> &'static str;   // "anthropic" | "openai-chat" | "openai-responses" | "gemini"

    // ── 出站（已有逻辑，仅 encode_tools 改签名）────────────────────────
    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value>;   // was &[&dyn Tool]
    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages;
    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError>;
    fn decode_stream_event(&self, event: &Value, state: &mut StreamDecodeState)
        -> Result<Vec<StreamEvent>, AdapterError>;

    // ── 入站（新增；默认返回 Unsupported，只给 3 个协议实现）──────────
    fn decode_request(&self, body: &Value) -> Result<DecodedRequest, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.protocol()))
    }
    fn encode_response(&self, resp: &DecodedResponse) -> Result<Value, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.protocol()))
    }
    fn encode_stream_event(&self, event: &StreamEvent, state: &mut StreamEncodeState)
        -> Result<Vec<SseFrame>, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.protocol()))
    }
}
```

**新增辅助类型**：

```rust
/// decode_request 的产出：把入站线格式请求拆成网关能转发的规范化件。
pub struct DecodedRequest {
    pub model: String,                 // 客户端传的别名
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,    // 仅定义，不可执行
    pub config: ModelConfig,           // temperature / top_p / max_tokens / reasoning_effort / ...
    pub stream: bool,
}

/// 一条要写给客户端的 SSE 帧（event 名可选 + data JSON）。
pub struct SseFrame { pub event: Option<String>, pub data: Value }

/// encode_stream_event 的跨事件状态（出站 SSE 分帧需要：response id、
/// 递增 seq、当前 content block index 等，按协议各自维护）。
#[derive(Default)]
pub struct StreamEncodeState { /* 协议私有缓冲 */ }
```

`AdapterError` 增加 `InboundUnsupported(&'static str)` 变体。

**适配器矩阵**（✓ = 本设计实现）：

| 协议 | 出站(已有) | 入站 decode_request | 入站 encode_response | 入站 encode_stream_event |
|------|:--:|:--:|:--:|:--:|
| openai-responses | ✓ | ✓ | ✓ | ✓ |
| openai-chat | ✓ | ✓ | ✓ | ✓ |
| anthropic | ✓ | ✓ | ✓ | ✓ |
| gemini | ✓ | 默认 Unsupported | 默认 Unsupported | 默认 Unsupported |

### 5.2 `alva-kernel-abi` 改动

1. 依赖新增 `alva-llm-wire`；`base/content.rs`、`base/message.rs`、`base/stream.rs`、`ModelConfig`/`ReasoningEffort`、`ToolDefinition` 改为 `pub use alva_llm_wire::...`（re-export，保证全工作区其余代码 import 路径不变）。
2. `adapter` 模块整体迁出到 `alva-llm-wire`；kernel-abi 保留 `pub use alva_llm_wire::adapter as adapter`（或直接 re-export `ProtocolAdapter` 等），并保留 `pub use ... ProtocolAdapter as ToolAdapter` 一个 deprecated 别名一个版本周期，平滑过渡。
3. `Tool` trait 已有 `fn definition(&self) -> ToolDefinition`——即"可执行 Tool → 纯定义"的桥接点，无需新增。

### 5.3 `alva-llm-provider` 改动

1. 4 个 provider 里 `adapter.encode_tools(tools)` 的调用点：`tools` 当前是 `&[&dyn Tool]`，改为 `&tools.iter().map(|t| t.definition()).collect::<Vec<_>>()`。其余不动。
2. **新增 `AliasRouter`**（路由内核，复用现有 provider 构造逻辑）：

```rust
/// 别名 → 上游配置 的多条路由表。复用 ConfigProviderAdapter 的
/// kind→provider 构造 switch；这是现有单 provider registry 的多别名版。
pub struct AliasRouter {
    routes: HashMap<String, ProviderConfig>,   // alias -> {kind, base_url, api_key, model, ...}
}
impl AliasRouter {
    pub fn resolve(&self, alias: &str) -> Option<Arc<dyn LanguageModel>>;  // 内部走 ConfigProviderAdapter
    pub fn upstream_protocol(&self, alias: &str) -> Option<&str>;          // = config.kind (默认 openai-chat)
}
```

> 现 `build_provider_registry` 只注册单 active provider（代码注释明确说 multi-provider out of scope）。`AliasRouter` 不改 `ProviderRegistry`，而是并列提供"按别名取 `LanguageModel`"，复用 `ConfigProviderAdapter::language_model` 内部的 `kind → 具体 provider` 分支。

### 5.4 `alva-app-gateway`（新 crate：库 + 瘦二进制）

**依赖**：`alva-llm-wire`（入站编解码）、`alva-llm-provider`（`AliasRouter` + `LanguageModel`）、`axum` / `tokio` / `tower`、`serde_yaml`（配置）。

**职责**（只有这些是真新代码）：

- HTTP 路由：`POST /v1/responses`、`POST /v1/chat/completions`、`POST /v1/messages`，各自绑定对应 `ProtocolAdapter` 作为**入站** adapter。
- 串线：见 §6。
- `RawTool`：实现 `Tool` 但 `execute()` 直接返回错误（永不被调用），只携带 `name/description/parameters_schema`，用于把 `DecodedRequest.tools`（`ToolDefinition`）喂给 `LanguageModel::complete/stream` 的 `&[&dyn Tool]` 形参。
- 配置加载（§8）。
- 库入口 `serve(config, addr) -> impl Future`，供 `alva-app-tauri`/`cli` embed；瘦二进制 `main.rs` 仅解析 args + 调 `serve`。

---

## 6. 数据流（请求生命周期）

入站协议 `IN`、上游协议 `OUT` 由别名路由决定，二者独立。

```
POST /v1/{responses|chat/completions|messages}   (协议 IN)
  │
  ├─ IN_adapter.decode_request(body) → DecodedRequest { model=别名, messages, tools, config, stream }
  ├─ router.resolve(别名) → Arc<dyn LanguageModel>      (上游 OUT 由 config.kind 决定)
  ├─ tools 包成 Vec<RawTool> → &[&dyn Tool]
  │
  ├─ 非流式 (stream=false):
  │     lm.complete(messages, &tools, config) → CompletionResponse
  │       └─(内部) OUT_adapter.encode_* → HTTP → OUT_adapter.decode_response  ← 全复用，零新代码
  │     IN_adapter.encode_response(DecodedResponse) → Value → HTTP 200 JSON
  │
  └─ 流式 (stream=true):
        lm.stream(messages, &tools, config) → Stream<StreamEvent>
          └─(内部) OUT_adapter.decode_stream_event 把上游 SSE 转成 StreamEvent  ← 全复用
        for ev in stream:
            IN_adapter.encode_stream_event(ev, &mut enc_state) → Vec<SseFrame>
            写入 axum SSE 响应
```

**关键点**：上游那一截（`encode_* → HTTP → decode_*`）就是现有 `LanguageModel` 实现，**网关一行上游代码都不写**。新代码只在两端的 `IN_adapter` 入站方向。

---

## 7. 入站方法契约

三个入站方法分别是三个已有出站方法的**镜像**：

| 入站方法 | 镜像的出站方法 | 输入 → 输出 |
|------|------|------|
| `decode_request` | `encode_messages` + `encode_tools` | 入站请求 JSON → `DecodedRequest` |
| `encode_response` | `decode_response` | `DecodedResponse`(规范化) → 入站响应 JSON |
| `encode_stream_event` | `decode_stream_event` | `StreamEvent` → `Vec<SseFrame>`(入站 SSE) |

**实现要点**：

- **Responses 入站**：请求体 `input[]` / `instructions` → `Message[]`；`tools[]` → `ToolDefinition[]`；`reasoning.effort` → `ReasoningEffort`。响应/流要发 `response.created` / `response.output_text.delta` / `response.completed` 等 named SSE，故 `StreamEncodeState` 需维护 response id + 递增 `sequence_number` + output item/content index。
- **Chat 入站**：`messages[]`（system 内联）→ `Message[]`；`tools[].function` → `ToolDefinition[]`；`reasoning_effort` → effort。流式发 `chat.completion.chunk`（`choices[].delta`），`StreamEncodeState` 维护 chunk id + role 首帧。
- **Anthropic 入站**：`system` 拆出 + `messages[]` → `Message[]`；`tools[]` → `ToolDefinition[]`；`thinking.budget_tokens` → effort（反向用 `suggested_token_budget` 的就近映射）。流式发 `message_start` / `content_block_delta(input_json_delta)` / `message_delta` / `message_stop`，`StreamEncodeState` 维护 block index。

> 出入站共享同一份"该协议线格式知识"集中在同一个 adapter 文件里（分「出站区 / 入站区」两节），降低漂移风险——比 moon-bridge 把 Client/Provider 拆两个文件更内聚。

---

## 8. 路由与配置

独立二进制读 YAML；embed 模式可直接传入 `AliasRouter`。

```yaml
# gateway.yml
listen: "127.0.0.1:8787"
routes:
  # 客户端传的 model 别名      → 上游
  gpt-5-via-deepseek:
    kind: openai-chat               # 上游协议（= ProviderConfig.kind）
    base_url: "https://api.deepseek.com/v1"
    api_key_env: DEEPSEEK_API_KEY   # 从环境变量取，不落盘
    model: deepseek-chat            # 真正发给上游的模型名
  claude-passthrough:
    kind: anthropic
    base_url: "https://api.anthropic.com"
    api_key_env: ANTHROPIC_API_KEY
    model: claude-sonnet-4-6
```

每条 route 反序列化成一个 `ProviderConfig` 塞进 `AliasRouter`。入站协议由 HTTP 路径决定，与 route 无关——任意入站协议都可路由到任意 `kind` 上游。

---

## 9. 错误处理

- 上游错误 / `AgentError`：由 **入站** adapter 的 `encode_error`（随 `encode_response` 一并实现）翻译成该入站协议的错误信封：
  - Responses/Chat：`{ "error": { "message", "type", "code" } }`
  - Anthropic：`{ "type": "error", "error": { "type", "message" } }`
- 别名未命中路由：HTTP 404 + 对应协议错误信封。
- `decode_request` 失败（体不合法）：HTTP 400。
- 流式中途上游报错：发一帧该协议的 error 事件再终止 SSE。

---

## 10. 测试策略

- **单元（`alva-llm-wire`）**：每个入站 adapter 的 `decode_request` round-trip——构造一个真实的入站请求 JSON（取自各家官方示例），解码成 `DecodedRequest`，断言 messages/tools/config 正确；再 `encode_response`/`encode_stream_event` 的 golden 测试（规范化结果 → 线格式，比对期望 JSON）。
- **跨协议矩阵**：Responses 入 → Chat 出、Anthropic 入 → Responses 出 等组合，断言 `DecodedRequest` 经上游 adapter 出站后体形正确。
- **集成（`alva-app-gateway`）**：复用 `alva-app-core/tests/e2e_http_test.rs` 的 mock 上游 server 模式——起网关 + mock 上游，发一个 Responses 请求，断言 (a) 上游收到的是翻译后的 Chat 请求 (b) 客户端收到的是 Responses 格式响应；流式与非流式各一条。
- **wasm**：`alva-llm-wire` 进 `cargo check --target wasm32` 名单。

---

## 11. 迁移与影响面

- **下沉的 blast radius**：靠 kernel-abi re-export，全工作区 `use alva_kernel_abi::base::message::Message` 等路径不变。唯一真改的是 `ProtocolAdapter::encode_tools` 签名（`&[&dyn Tool]` → `&[ToolDefinition]`）及其 4 个 provider 调用点（各一行 `.map(|t| t.definition())`）。
- **改名**：`ToolAdapter` → `ProtocolAdapter`，保留 deprecated 别名一个周期。
- **分阶段**（每阶段可独立编译 + 测试通过）：
  1. 抽 `alva-llm-wire`：下沉类型 + 改名 + re-export，全绿（纯搬运，零行为变化）。
  2. `encode_tools` 改签名 + provider 调用点适配，全绿。
  3. 给 3 个协议实现 `decode_request` / `encode_response` / `encode_stream_event` + 单元测试。
  4. `AliasRouter`（alva-llm-provider）。
  5. `alva-app-gateway`：HTTP 壳 + RawTool + 串线 + 集成测试。
  6.（可选）`alva-app-tauri`/`cli` embed 入口。

---

## 12. 开放问题

- `RawTool` 的 `parameters_schema()` 直接返回 `ToolDefinition.parameters`——确认现有 `Tool::definition()` 与各出站 `encode_tools` 对 schema 的归一化（`normalize_llm_tool_schema`）在"定义来自外部客户端"时是否仍适用（外部 schema 可能不规范）。
- 是否需要在 v1 暴露 `/v1/models`（列出别名）供客户端发现。倾向：做，成本低。
- 配置热重载：v1 先不做，重启生效。
- **usage 透传**：现 `CompletionResponse { message, raw }` 不带独立 usage 字段（provider 的 `complete()` 在 `decode_response` 后只取 message + raw，丢了 `DecodedResponse.usage`）。网关非流式要回吐 token 用量，需补一条路：要么给 `CompletionResponse` 加 `usage: Option<UsageMetadata>`，要么网关用上游 adapter 对 `raw` 重跑 `decode_response` 取 usage。倾向前者（顺手修这个既有信息丢失）。流式路径 usage 走 `StreamEvent` 末帧，无此问题。
