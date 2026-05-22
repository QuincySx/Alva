# alva-llm-wire + 协议网关 设计文档

> 状态：草案 v2.1（两轮 agent 技术 review，第二轮判定 READY TO PLAN；已折入 3 处精度修正）
> 日期：2026-05-22
> 参考：[ZhiYi-R/moon-bridge](https://github.com/ZhiYi-R/moon-bridge)（Go 协议转换网关）
>
> **v2 修订（依据 agent review，均已对照代码核实）**：①撤销 §12 usage 误判（usage 在 `Message.usage`，没丢）；②依赖补 `uuid`/`chrono`（非"只 serde"）；③`ContentBlock`/`ModelConfig` 下沉前需先拆 `execution.rs`/`model/mod.rs`（新增 Phase 0）；④新增 `StreamEvent::Stop{reason}`——原 `StreamEvent` 无终止原因，流式无法重建合规终止帧；⑤多模态列为 v1 非目标且显式 400（现 `encode_messages` 不处理 image，会静默丢）；⑥reasoning `signature` 跨协议往返规则；⑦`encode_tools` 影响面更正为 8 调用点+签名+测试；⑧网关需自有配置层（`ProviderConfig` 无 `listen`/`api_key_env`）。

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
- **不做多模态（图片/音频）入站转发**（v1 非目标）。`ContentBlock::Image` 存在（`content.rs:13`）但**没有任何 adapter 的 `encode_messages` 处理它**——直接转发会被静默丢弃。v1 的 `decode_request` 遇到 image block 应**显式报错（400）而非静默丢**；要支持得先给三个出站 adapter 补 image 编码（单列任务）。

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
NEW  alva-llm-wire        L1 基础库 · 依赖 serde / serde_json / uuid / chrono（按需 schemars）· wasm-clean
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

**Cargo 依赖**：`serde`（derive）、`serde_json`、**`uuid`**、**`chrono`**（`Message::user/system` 构造器与 `decode_response` 用 `uuid::Uuid::new_v4()` + `chrono::Utc::now()` 生成 id/timestamp——见 `message.rs:49,54`、`anthropic.rs:282`，故"只 serde"是错的；二者均 wasm-safe）；仅当下沉类型确有 `#[derive(JsonSchema)]` 时才加 `schemars`。**不得**依赖 `alva-kernel-bus` / `alva-macros` / 任何框架 crate。

**下沉的类型**（从 `alva-kernel-abi` 移入，原处改为 `pub use alva_llm_wire::...` re-export）：

- `base/content.rs` → `ContentBlock`（及其内部类型）
- `base/message.rs` → `Message` / `MessageRole` / `UsageMetadata`
- `base/stream.rs` → `StreamEvent`
- `model` 中的 `ModelConfig` / `ReasoningEffort`
- `tool/types.rs` 中的 `ToolDefinition`（纯值类型 `{ name, description, parameters }`）

> **耦合现实（已核对，比初稿想的多两处文件拆分）**：
> - `message` → `content`、`stream` → `message`：干净。
> - **`content` 不是无依赖**：`ContentBlock::ToolResult` 持有 `Vec<crate::tool::execution::ToolContent>`（`content.rs:35`），而 `tool/execution.rs:11` `use alva_kernel_bus::BusHandle`。`ToolContent`/`ToolOutput` 本身是纯 serde 类型（`execution.rs:46-92`），但必须**先把它们从 bus 耦合的 `execution.rs` 拆出**到一个 serde-clean 模块，`ContentBlock` 才能下沉。
> - **`ModelConfig`/`ReasoningEffort` 不在独立文件**：与 `LanguageModel`（`model/mod.rs:161`）、`#[crate::bus_cap] TokenCounter`（`:203`）、`use crate::tool::Tool`（`:11`）同住 `model/mod.rs`。需**拆 `model/mod.rs`**，把这两个值类型剥到独立模块再下沉。
>
> 这两处拆分是下沉的**前置必做项**（见 §11 Phase 0），不是纯搜索替换。**留在 kernel-abi 不动**的是：`Tool` trait（可执行，依赖 bus/执行环境）、`LanguageModel`/`Provider`/`ProviderRegistry`、`AgentError`、`ToolExecutionContext`、session/scope/runtime 等框架契约。

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

**必须给 `StreamEvent` 增加终止原因（关键，否则流式不可用）**：现 `StreamEvent`（`stream.rs:9-45`）只有 `Start/TextDelta/ReasoningDelta/ReasoningBlock/ToolCall*/Usage/Done/Error`——**没有 stop/finish 原因**。`decode_stream_event` 读到 `finish_reason` 后就地消费、不入事件（`openai_chat.rs:277-287`）。但每个入站协议的终止帧都**必须**带它（Chat `finish_reason`、Anthropic `message_delta.stop_reason`、Responses `response.completed.status`）。所以新增：

```rust
pub enum StopReason { EndTurn, ToolUse, MaxTokens, StopSequence, Other(String) }
// StreamEvent 增加： Stop { reason: StopReason },   // 在 Done 之前发
```

四个出站 adapter 的 `decode_stream_event` 同步改为把 `finish_reason`/`stop_reason` 映射成 `StopReason` 并发 `StreamEvent::Stop`。这是一处 kernel-abi（下沉后在 wire crate）的**跨切面改动**，影响所有 `StreamEvent` 消费者（`run.rs` 等）的穷尽 match——见 §11。`DecodedRequest` 文档补一句：遇 `ContentBlock::Image` 返回 `AdapterError`（v1 不转发多模态，见 §2）。

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
            # 终止帧由 StreamEvent::Stop{reason} 映射成各协议 finish_reason/stop_reason
```

**关键点**：上游那一截（`encode_* → HTTP → decode_*`）就是现有 `LanguageModel` 实现，**网关一行上游代码都不写**。新代码只在两端的 `IN_adapter` 入站方向。

**流式只能做到"基本对称"，不是全无损**（见 §7）：纯文本 / reasoning delta / usage 可镜像；`StopReason` 需新增变体才能传（见 §5.1）；Responses 要求的 `output_index`/`content_index`/`sequence_number` 在扁平的 `StreamEvent` 里没有，须由 `StreamEncodeState` 合成——单文本块没问题，文本+工具+reasoning 交错时较脆，须专门测（§10）。

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

**跨协议有损点（必须显式处理，不能静默）**：

- **StopReason 映射表**（`StreamEvent::Stop` ↔ 各协议）：

  | StopReason | Chat finish_reason | Anthropic stop_reason | Responses status |
  |---|---|---|---|
  | EndTurn | `stop` | `end_turn` | `completed` |
  | ToolUse | `tool_calls` | `tool_use` | `completed`(有 function_call) |
  | MaxTokens | `length` | `max_tokens` | `incomplete` |
  | StopSequence | `stop` | `stop_sequence` | `completed` |

- **reasoning `signature` 往返不对称**：`ContentBlock::Reasoning.signature`（`content.rs`）是 Anthropic 的 attestation，**下一轮必须原样回传**，否则 Anthropic 400。跨协议时：上游≠Anthropic 时网关**剥掉 signature**；**不要**把无 signature 的 reasoning block 转发给 Anthropic 上游（会触发 400）。OpenAI reasoning 无 signature 槽，反向也无法补。
- **多模态**：见 §2——v1 `decode_request` 遇 image 直接 400，不静默丢。

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

**注意 `ProviderConfig` 字段对不上**：它只有 `api_key`/`model`/`base_url`/`max_tokens`/`custom_headers`/`kind`（`config.rs:23-37`），**没有** `listen` / `api_key_env`。所以网关需要一层自己的配置结构：

```rust
struct GatewayConfig { listen: String, routes: HashMap<String, RouteConfig> }
struct RouteConfig { kind: String, base_url: String, api_key_env: String, model: String, max_tokens: Option<u32> }
// 加载时 RouteConfig → 解析 api_key_env 取环境变量 → 构造 ProviderConfig → 塞进 AliasRouter
```

入站协议由 HTTP 路径决定，与 route 无关——任意入站协议都可路由到任意 `kind` 上游。

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
- **StopReason 映射**：每协议终止帧的 `finish_reason`/`stop_reason`/`status` 对照 §7 表逐项断言（含 MaxTokens、ToolUse）。Responses 的 `ToolUse` **不是靠 status 表达**（status 仍 `completed`），编码端要补 `function_call` output item——单测要覆盖这点。
- **事件顺序**：`decode_stream_event` 对一个带 `finish_reason` 的上游 chunk，断言产出顺序为 `[Usage?, Stop{reason}, Done]`（`Stop` 在 `Done` 前、`Usage` 后）——网关终止帧正确性依赖这个顺序。
- **交错流式（最脆，必测）**：构造 文本→tool_call→文本→reasoning 交错的上游流，断言 Responses 入站重建的 `output_index`/`content_index`/`sequence_number` 单调且合法——这是合成索引最易出错处。
- **集成（`alva-app-gateway`）**：复用 `alva-app-core/tests/e2e_http_test.rs` 的 mock 上游 server 模式——起网关 + mock 上游，发一个 Responses 请求，断言 (a) 上游收到的是翻译后的 Chat 请求 (b) 客户端收到的是 Responses 格式响应；流式与非流式各一条。
- **image 拒绝**：带 image block 的入站请求断言返回 400（非静默丢）。
- **wasm**：`alva-llm-wire` 进 `cargo check --target wasm32` 名单。

---

## 11. 迁移与影响面

- **下沉的 blast radius**：靠 kernel-abi re-export，全工作区 `use alva_kernel_abi::base::message::Message`、`use alva_kernel_abi::adapter::...` 等路径不变（已核对：provider 全走 `alva_kernel_abi::adapter::*`，rename + re-export 后不需改 import）。
- **`encode_tools` 改签名的真实影响面（比初稿大）**：`&[&dyn Tool]` → `&[ToolDefinition]` 涉及 **8 个生产调用点**（4 provider × complete/stream 各一：`anthropic.rs:201,302`、`openai_responses.rs:117,191`、`openai_chat.rs:181,262`、`gemini.rs:181,255`）+ **5 处 trait/impl 签名**（`adapter/{mod,anthropic,openai_chat,openai_responses,gemini}.rs`）+ 各 adapter 的 in-file 测试（构造 `&dyn Tool` 的辅助需改成 `ToolDefinition`）。不是"4 行"。`LanguageModel::complete/stream` 仍收 `&[&dyn Tool]`，`.definition()` 转换在每个 provider 内做。
- **`StreamEvent::Stop` 的真实影响面（已全工作区核对）**：穷尽 match（无 `_ =>`）只有**一处**——`alva-kernel-core/src/run.rs:369-454`，须加一臂（放进现有 `Start|Done|...` 边界组即可，除非要用 reason）。其余消费者都有 wildcard（`app-cli/ui/app.rs:1219`、`e2e_http_test.rs` 的 `_ => {}`）或只是构造 `StreamEvent`（engine-adapter 的 `parse_stream_delta` 返回 `_ => None`）。另需改 **4 个 `decode_stream_event`** 去 emit `Stop` + 它们现有的 finish_reason 测试（如 `openai_chat.rs:472-477`）。`run.rs` 不会用到 reason（`Message` 无 stop_reason 字段，`message.rs:34-44`）——`Stop` 只服务网关/流式编码路径；**不要**为此给 `Message` 加字段（YAGNI，无消费者）。
- **改名**：`ToolAdapter` → `ProtocolAdapter`，保留 deprecated 别名一个周期。注意 `McpToolAdapter`（`alva-protocol-mcp` / `alva-app-core`）是**无关**的 `Tool` impl，不是这个 trait——rename 用 grep 时别误伤。
- **分阶段**（每阶段可独立编译 + 测试通过）：
  0. **类型隔离（前置必做）**：拆 `tool/execution.rs` 把 `ToolContent`/`ToolOutput` 移到 serde-clean 模块；拆 `model/mod.rs` 把 `ModelConfig`/`ReasoningEffort` 剥出。两步都在 kernel-abi 内完成，纯重排，全绿。**验收标准**：拆出的子模块必须在**旧的扁平路径和模块路径都 re-export**——多数消费者走扁平 `alva_kernel_abi::ModelConfig`（`lib.rs:41/45`，path-stable），但有 5 处走模块路径 `alva_kernel_abi::model::ModelConfig`（`app-tauri/src/provider_api.rs:13`、`host-wasm/src/{agent.rs:27,smoke.rs:28,stub.rs:21}`、`app-core/tests/scope_integration.rs:14`），故 `model/mod.rs` 必须 `pub use` 新值类型模块内容，否则这 5 处断。
  1. 新增 `StreamEvent::Stop{reason}` + `StopReason` + 改 4 个 `decode_stream_event` 填充 + 全工作区 match 补臂，全绿（出站行为增强，无回归）。
  2. 抽 `alva-llm-wire`：下沉 content/message/stream/ModelConfig/ReasoningEffort/ToolDefinition + adapter 模块 + 改名，kernel-abi re-export，全绿（纯搬运）。
  3. `encode_tools` 改签名 + 上述 8 调用点 + 测试适配，全绿。
  4. 给 3 个协议实现 `decode_request` / `encode_response` / `encode_stream_event`（含 StopReason 映射、image 拒绝、signature 处理）+ 单元/golden 测试。
  5. `AliasRouter` + `GatewayConfig` 包装层（alva-llm-provider / gateway）。
  6. `alva-app-gateway`：HTTP 壳 + RawTool + 串线 + 集成测试（含交错流式用例）。
  7.（可选）`alva-app-tauri`/`cli` embed 入口。

---

## 12. 开放问题

- `RawTool` 的 `parameters_schema()` 直接返回 `ToolDefinition.parameters`——确认现有 `Tool::definition()` 与各出站 `encode_tools` 对 schema 的归一化（`normalize_llm_tool_schema`）在"定义来自外部客户端"时是否仍适用（外部 schema 可能不规范）。
- 是否需要在 v1 暴露 `/v1/models`（列出别名）供客户端发现。倾向：做，成本低。
- 配置热重载：v1 先不做，重启生效。
- ~~usage 透传需补字段~~ **（已撤销——原判断有误）**：usage 没有丢。`decode_response` 把它写进 `Message.usage`（`message.rs:42`，由 `anthropic.rs:285` / `openai_chat.rs:206` 填充），`CompletionResponse.message.usage` 直接可读。网关非流式从这里取用量即可，**无需**给 `CompletionResponse` 加字段。流式 usage 走 `StreamEvent::Usage` 帧。
