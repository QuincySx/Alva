# Alva 架构设计

## 身份

| 属性 | 值 |
|------|-----|
| 项目代号 | Alva |
| 三仓库 | alva-sandbox（沙箱）+ alva-agent（框架）+ alva-app（产品） |
| 技术栈 | 纯 Rust — GPUI (Zed GPU UI 框架) + 自研 Agent 引擎 |
| 目标平台 | macOS（首发）/ Windows / Linux |

## 核心理念

Alva 是一个**分层解耦的 AI Agent 平台**，三大组件完全独立：

- **alva-sandbox** — 通用沙箱基础设施，不知道谁跑在里面
- **alva-agent** — 通用 Agent 框架，不知道自己跑在哪
- **alva-app** — 产品应用，把 sandbox + agent 组合成桌面工具

与 Claude Code / Codex 的核心区别：**Skill 和 MCP 按 Agent 模板定义加载，不是全局的。** 不同的 Agent 有不同的能力集。

## 三仓库架构

```
┌──────────────────────────────────────────────────────────────────┐
│  alva-app（产品层）                                               │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  alva-app          — GPUI 桌面 GUI                       │    │
│  │  alva-app-cli      — 终端 CLI                            │    │
│  │  alva-app-core     — BaseAgent + Extension 系统           │    │
│  │  alva-app-eval   — Agent 测试/评测 playground          │    │
│  │  alva-app-debug    — 调试 HTTP API + traced! 宏          │    │
│  │  alva-app-devtools-mcp — MCP 开发工具服务器              │    │
│  └──────────────────────────────────────────────────────────┘    │
│          依赖 ↓                           依赖 ↓                 │
│  ┌───────────────────────┐  ┌───────────────────────────────┐    │
│  │  alva-agent（框架层）  │  │  alva-sandbox（沙箱层）       │    │
│  └───────────────────────┘  └───────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

---

## alva-agent 框架层

### 依赖层级图

```
┌─ alva-kernel-bus ─────────────────────────────────────────────────┐
│  Bus, BusHandle, Caps, EventBus, BusEvent, StateCell             │
│  （跨层协调总线，零 workspace 依赖）                                │
└──────────────────────────────────────────────────────────────────┘
              ↑ 唯一依赖
┌─ alva-kernel-abi ─────────────────────────────────────────────────────┐
│  Message, ContentBlock, Tool, LanguageModel, ToolFs              │
│  AgentMessage, StreamEvent, ToolCall, ToolOutput                 │
│  TokenCounter, ToolExecutionContext（含 bus() 方法）              │
│  TokenBudgetExceeded, ContextCompacted, MemoryExtracted          │
│  Bus, BusHandle, BusEvent, StateCell（re-export）                │
│  （共享词汇表 — 所有 crate 通过依赖 alva-kernel-abi 自动获得 bus）       │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 功能层（并行，互不依赖）─────────────────────────────────────────┐
│  alva-agent-context  — 上下文管理 Hooks + ContextStore + 四层模型 │
│  alva-kernel-core     — Agent 循环引擎 + Middleware 洋葱模型       │
│  alva-agent-tools    — 工具实现 + tool_presets 分组               │
│  alva-agent-security — SecurityGuard + PermissionManager          │
│  alva-agent-memory   — FTS + 向量搜索 + MemoryBackend trait       │
│  alva-agent-graph    — StateGraph + Pregel + Channel + SubAgent   │
│  alva-agent-scope    — SpawnScope + Blackboard                    │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 组装层 ─────────────────────────────────────────────────────────┐
│  alva-host-native  — 中间件实现（Security/Compaction/Checkpoint/│
│                         PlanMode）+ AgentRuntimeBuilder           │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 引擎层 ─────────────────────────────────────────────────────────┐
│  alva-engine-runtime       — EngineRuntime trait（统一引擎接口）   │
│  alva-engine-adapter-alva  — 本地 Agent 适配器（直接 Rust 调用）  │
│  alva-engine-adapter-claude — Claude SDK 适配器（Node.js bridge） │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 协议层（独立，不依赖 alva-app）─────────────────────────────────┐
│  alva-protocol-skill — Skill 三级加载：metadata → body → resource │
│  alva-protocol-mcp   — MCP 客户端：连接、工具发现、McpToolAdapter  │
│  alva-protocol-acp   — Agent Client Protocol：消息、会话、进程     │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ LLM 提供者层 ───────────────────────────────────────────────────┐
│  alva-llm-provider                                               │
│  ├─ OpenAIChatProvider     — /v1/chat/completions                │
│  ├─ OpenAIResponsesProvider — /v1/responses (Responses API)      │
│  ├─ AnthropicProvider      — /v1/messages                        │
│  ├─ ProviderConfig         — api_key + base_url + custom_headers │
│  └─ auth.rs                — resolve_auth_headers() 统一认证      │
└──────────────────────────────────────────────────────────────────┘
```

---

## alva-app-core 模块布局

经过 Extension 系统收敛，app-core 只保留下面这些模块：

```
crates/alva-app-core/src/
├── lib.rs          — facade re-exports + 模块声明
├── base_agent/     — BaseAgent + BaseAgentBuilder + PermissionMode
├── extension/      — Extension trait + HostAPI + 内置 Extensions
├── plugins/        — agent_spawn（SubAgent）、evaluation（SprintContract）
├── skills/         — SkillStore + SkillLoader + SkillInjector
├── mcp/            — MCP client / server manager（供 McpExtension 使用）
├── hooks/          — HookExecutor（供 HooksExtension 使用）
├── settings/       — Settings + HooksSettings
├── paths/          — AlvaPaths（workspace/global 路径解析）
├── utils/          — estimate_cost_usd + format_token_count
└── error.rs        — EngineError
```

**已删除的旧模块**（pre-Extension 时代遗留）：
- ❌ `agent/` — ACP 旧副本 + sqlite 持久化（被 alva-protocol-acp 取代）
- ❌ `state/` — AppState/Selectors（只有 2 个 util 真被用，搬到 utils）
- ❌ `swarm/` — 旧的多 agent coordinator（被 Extension 系统取代）
- ❌ `domain/` + `ports/` + `adapters/` — DDD 三件套（只服务于已删除的 sqlite 持久化）
- ❌ `base/` — 14 行的 process_manager re-export 壳
- ❌ `analytics/` + `auth/` + `lsp/` — 从未被调用的空壳
- ❌ `types/` — 4 行 legacy placeholder

---

## Extension 系统（alva-app-core 层）

**Extension 是 BaseAgent 的唯一公开扩展点。** 所有能力——工具、中间件、子 agent、
plan mode、skills、MCP、hooks——都通过 Extension 注入。Builder 本身只负责纯配置
（workspace / system_prompt / max_iterations）。

### 设计原则

1. **单一入口**：所有扩展能力通过 `.extension(Box::new(...))` 注册
2. **构建期 + 运行期分离**：Extension 既参与 build，也在 build 完成后通过 HostAPI
   持续与 agent 交互
3. **错误隔离**：一个 Extension 的 handler panic 不会影响其他 Extension
4. **可组合**：内置 19 个 Extension，可以自由组合，也可以继续添加自定义

### 层级定位

```
Extension trait 不在 agent-core（那里只有 Tool + Middleware 原语），
也不在 agent-tools（那里只有 tool 实现）。

Extension 是 app-core 层的组合概念——把 agent-core/agent-tools/agent-runtime 的
原语组合成有意义的能力包。

┌─ alva-app-core ──────────────────────────────────────────────────┐
│                                                                  │
│  extension/                                                      │
│  ├── mod.rs       — Extension trait 定义                          │
│  ├── context.rs   — ExtensionContext / FinalizeContext            │
│  ├── events.rs    — ExtensionEvent / EventResult                  │
│  ├── host.rs      — ExtensionHost / HostAPI                       │
│  ├── bridge.rs    — ExtensionBridgeMiddleware                     │
│  └── builtins.rs  — 内置 Extension 实现                           │
│                                                                  │
│  base_agent/                                                     │
│  ├── builder.rs   — .extension() API + build() 生命周期           │
│  └── agent.rs     — BaseAgent 运行时                              │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### Extension Trait

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }

    // 构建期四阶段：
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    fn activate(&self, _api: &HostAPI) {}               // 注册 middleware + 事件 + 命令
    async fn configure(&self, _ctx: &ExtensionContext) {}  // bus/workspace 配置
    async fn finalize(&self, _ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
}

pub struct ExtensionContext {
    pub bus: BusHandle,
    pub bus_writer: BusWriter,      // 允许 Extension 往 bus 注册服务
    pub workspace: PathBuf,
    pub tool_names: Vec<String>,
}

pub struct FinalizeContext {
    pub bus: BusHandle,
    pub bus_writer: BusWriter,
    pub workspace: PathBuf,
    pub model: Arc<dyn LanguageModel>,  // 仅 finalize 能拿到 model
    pub tools: Vec<Arc<dyn Tool>>,      // 所有 Extension 收集完的最终工具列表
    pub max_iterations: u32,
}
```

### HostAPI — 运行时插件接口

`activate(api)` 接收 HostAPI 句柄，让 Extension 在 build 完成后仍能持续和 agent
交互。这是与 Pi 架构对齐的核心。

```rust
pub struct HostAPI { /* Arc<RwLock<ExtensionHost>> */ }

impl HostAPI {
    // 注册中间件（替代旧的 Extension.middleware() 方法）
    pub fn middleware(&self, mw: Arc<dyn Middleware>);

    // 订阅运行时事件
    pub fn on(&self, event_type: &'static str,
              handler: impl Fn(&ExtensionEvent) -> EventResult + Send + Sync + 'static);

    // 注册 /command 斜杠命令
    pub fn register_command(&self, name: &str, description: &str);

    // 运行时注入消息
    pub fn steer(&self, text: &str);         // 立即打断当前回合
    pub fn follow_up(&self, text: &str);     // 当前回合结束后执行

    // 终止 agent
    pub fn shutdown(&self);
}

pub enum ExtensionEvent {
    AgentStart,
    AgentEnd { error: Option<String> },
    BeforeToolCall { tool_name, tool_call_id, arguments },
    AfterToolCall { tool_name, tool_call_id, result },
    Input { text },
}

pub enum EventResult {
    Continue,               // 继续执行
    Block { reason },       // 阻止（仅 before_tool_call 生效）
    Handled,                // 已处理，短路后续 handler
}
```

### Middleware vs Event Handler

这是容易混淆的点。两者**不重复**，一个是底层机制，一个是上层 API：

| | Middleware | Event Handler |
|---|-----------|---------------|
| 定义 | `Middleware` trait impl | 通过 `api.on()` 注册的闭包 |
| 访问能力 | `&mut AgentState`，可修改 messages/response/tool_call | 只能观察 + Block/Handled |
| 注册方式 | `api.middleware(Arc<dyn Middleware>)` | `api.on(event_type, handler)` |
| 优先级 | `priority()` 排序 | 注册顺序 |
| 适用场景 | 需要修改 state 的拦截（security、compaction、loop detection） | 插件级扩展、异步通知、外部集成 |

`ExtensionBridgeMiddleware`（内置，优先级最外层）桥接二者：在每个 middleware hook
里调用 `host.emit(event)`，把 middleware 的拦截点暴露给 event handler。

### 内置 Extension 清单

**工具 Extensions**（通过 `tools()` 提供工具）：
- `CoreExtension` — read/write/edit/search/list
- `ShellExtension` — execute_shell
- `InteractionExtension` — ask_human
- `TaskExtension` — task CRUD
- `TeamExtension` — 多 agent 协调
- `PlanningExtension` — 计划 + worktree
- `UtilityExtension` — sleep/config/notebook/...
- `WebExtension` — 搜索 + 抓取 URL
- `BrowserExtension` — 7 个浏览器自动化工具

**中间件 Extensions**（通过 `activate()` 里的 `api.middleware()` 注册）：
- `LoopDetectionExtension`
- `DanglingToolCallExtension`
- `ToolTimeoutExtension`
- `CompactionExtension`
- `CheckpointExtension`
- `PlanModeExtension` — 同时在 `configure()` 里把 `PlanModeControl` 注册到 bus

**系统 Extensions**（多阶段混合）：
- `SkillsExtension` — tools() + activate()（SkillInjectionMiddleware） + configure()（扫描 skill 目录）
- `McpExtension` — configure()（异步启动 MCP server + 注入 tools）
- `HooksExtension` — activate()（HooksMiddleware）
- `SubAgentExtension` — finalize()（需要最终 tool 列表和 model）
- `EvaluationExtension` — activate()（SprintContractMiddleware，可选）

### BaseAgent 构建生命周期

```
BaseAgentBuilder
    │
    │  .workspace(path)                 ← 必需
    │  .system_prompt(&str)             ← 必需
    │  .max_iterations(n)
    │  .extension(Box::new(...))  × N   ← 所有能力都走这里
    │  .with_approval_channel()          ← HITL 权限通道
    │  .with_memory()                    ← 可选
    │
    ▼
build(model).await
    │
    │  ① 创建 Bus，注册 TokenCounter
    │  ② 遍历 extensions → 收集 tools
    │  ③ 创建 ExtensionHost + 遍历 extensions.activate(HostAPI)
    │        · 注册 middleware 到 host
    │        · 注册事件 handlers 到 host
    │        · 注册 /command
    │  ④ 构建 MiddlewareStack（security + host.take_middlewares() + bridge）
    │  ⑤ middleware_stack.configure_all(MiddlewareContext { bus, workspace })
    │  ⑥ ext.configure(ExtensionContext { bus, bus_writer, workspace, tool_names })
    │  ⑦ ext.finalize(FinalizeContext { model, tools, ... }) → 收集额外 tools
    │     （SubAgentExtension 用这个加 AgentSpawnTool）
    │  ⑧ 组装 AgentState + AgentConfig
    │  ⑨ 创建 PendingMessageQueue + CancellationToken
    │  ⑩ host.bind_agent(pending, cancel) → 激活 steer/follow_up/shutdown
    │  ⑪ 可选：创建 MemoryService
    │
    ▼
BaseAgent
    ├── state:        Arc<Mutex<AgentState { model, tools, session }>>
    ├── config:       Arc<AgentConfig { middleware_stack, bus, workspace }>
    ├── tool_registry
    ├── memory:       Option<MemoryService>
    ├── security_guard: Option<Arc<Mutex<SecurityGuard>>>
    ├── pending_messages: Arc<PendingMessageQueue>     ← steer/follow_up
    ├── current_cancel:   Arc<Mutex<CancellationToken>>  ← shutdown
    ├── bus / bus_writer                                  ← 跨层通信
    └── extension_host: Arc<RwLock<ExtensionHost>>        ← 事件分发 + 命令
```

### 使用方式

```rust
// ── CLI（生产配置）─────────────────────────────────────────
let agent = BaseAgent::builder()
    .workspace(workspace)
    .system_prompt(&prompt)
    // 工具
    .extension(Box::new(SkillsExtension::new(skill_dirs)))
    .extension(Box::new(CoreExtension))
    .extension(Box::new(ShellExtension))
    .extension(Box::new(InteractionExtension))
    .extension(Box::new(TaskExtension))
    .extension(Box::new(TeamExtension))
    .extension(Box::new(PlanningExtension))
    .extension(Box::new(UtilityExtension))
    .extension(Box::new(WebExtension))
    // 中间件
    .extension(Box::new(LoopDetectionExtension))
    .extension(Box::new(DanglingToolCallExtension))
    .extension(Box::new(ToolTimeoutExtension))
    .extension(Box::new(CompactionExtension))
    .extension(Box::new(CheckpointExtension))
    .extension(Box::new(PlanModeExtension::new()))
    // 系统
    .extension(Box::new(SubAgentExtension::new(3)))
    .extension(Box::new(McpExtension::new(mcp_paths)))
    .extension(Box::new(HooksExtension::new(hook_settings)))
    .build(model).await?;

// ── Eval / 动态配置 ────────────────────────────────────────
// Eval 也走 Extension，根据 UI 勾选动态构造
for name in selected_extensions {
    if let Some(ext) = create_extension(name, &workspace) {
        builder = builder.extension(ext);
    }
}

// ── 自定义 Extension ──────────────────────────────────────
struct MyExtension;

#[async_trait]
impl Extension for MyExtension {
    fn name(&self) -> &str { "my-ext" }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(MyTool)]
    }

    fn activate(&self, api: &HostAPI) {
        // 监听工具执行完成，注入 steering 消息
        let api_clone = /* ... */;
        api.on("after_tool_call", move |event| {
            if let ExtensionEvent::AfterToolCall { tool_name, .. } = event {
                if tool_name == "my_tool" {
                    // 异步处理...
                }
            }
            EventResult::Continue
        });

        // 注册 /my-command
        api.register_command("my-cmd", "do my thing");
    }
}
```

---

## Bus 总线架构

### 设计哲学

各层通过 Cargo 依赖图是**纵向隔离**的（上层不能 import 下层的具体类型）。
Bus 提供**横向通信**——任何层都可以注册能力、发射事件、发现服务，而不需要知道对方在哪一层。

```
     alva-app-core（创建 Bus，注册能力）
            │
            │  bus.provide::<dyn TokenCounter>(...)
            │  bus.provide::<dyn AgentLoopHook>(...)
            │  bus.provide::<ApprovalNotifier>(...)
            │
     ┌──────┴──────────────────────────────────────┐
     │              BusHandle（Clone）               │
     │                                              │
     │   Caps        EventBus       StateCell       │
     │   provide()   emit()         new(T)          │
     │   get()       subscribe()    get() / set()   │
     │   require()                  watch()         │
     └──┬──────────┬──────────────┬────────────────┘
        │          │              │
        ▼          ▼              ▼
   agent-core  agent-runtime  agent-context
   (run loop)  (middleware)   (sdk_impl)

   bus.get::<dyn AgentLoopHook>()
              bus.get::<ApprovalNotifier>()
              bus.emit(TokenBudgetExceeded{...})
                            bus.get::<dyn TokenCounter>()
                            bus.get::<dyn MemoryBackend>()
                            bus.get::<dyn Summarizer>()
```

### Bus 上注册了什么

**能力（Caps）— 在 BaseAgent::build() 中注册：**

| 能力类型 | 注册方 | 消费方 | 作用 |
|---------|--------|--------|------|
| `dyn TokenCounter` | app-core (HeuristicTokenCounter) | context, compaction middleware | 真实 token 计数 |
| `dyn AgentLoopHook` | app-core (PendingMessageQueue) | agent-core run loop | mid-turn 消息注入 |
| `ApprovalNotifier` | app-core (if approval enabled) | security middleware | 工具审批通知 UI |
| `CheckpointCallbackRef` | app-core (set_checkpoint_callback) | checkpoint middleware | 文件写入前备份 |
| `dyn MemoryBackend` | (未来) app-core | context sdk_impl | 记忆查询/存储 |
| `dyn Summarizer` | (未来) app-core | context sdk_impl | LLM 摘要生成 |

**事件（Events）— 运行时发射：**

| 事件 | 发送方 | 接收方 | 含义 |
|------|--------|--------|------|
| `TokenBudgetExceeded` | compaction middleware | UI, metrics | 上下文 token 超预算 |
| `ContextCompacted` | compaction middleware | UI, metrics | 压缩完成 |
| `MemoryExtracted` | (未来) context hooks | UI, metrics | 记忆提取完成 |

### 通信三条路

Bus 提供三种机制，适用于不同场景：

```
┌─────────────────────────────────────────────────────────────────┐
│                        三种通信方式                               │
├───────────────┬──────────────────┬──────────────────────────────┤
│  Caps         │  EventBus        │  StateCell                   │
│  能力发现      │  事件通知         │  共享状态                     │
├───────────────┼──────────────────┼──────────────────────────────┤
│  provide(Arc) │  emit(event)     │  new(initial)                │
│  get() → Arc  │  subscribe() →Rx │  get() → T / set(T)          │
│  require()    │                  │  watch() → Rx                │
├───────────────┼──────────────────┼──────────────────────────────┤
│  初始化时注册  │  运行时发射       │  运行时读写                   │
│  不变，长生命周期│  即发即忘        │  写入时自动通知               │
├───────────────┼──────────────────┼──────────────────────────────┤
│  适用：        │  适用：           │  适用：                      │
│  服务发现      │  跨层通知         │  UI 可观测状态                │
│  TokenCounter │  TokenBudget     │  当前 permission mode        │
│  MemoryBackend│  ContextCompacted│  token 使用率                 │
│  Summarizer   │  MemoryExtracted │                              │
└───────────────┴──────────────────┴──────────────────────────────┘
```

### 什么走 Bus，什么不走

| 场景 | 走 Bus | 不走 Bus |
|------|--------|---------|
| 跨 crate 服务发现 | ✅ `bus.get::<dyn Service>()` | |
| 跨 crate 事件通知 | ✅ `bus.emit(Event)` | |
| 同一 crate 内调用 | | ✅ 直接函数调用 |
| 父子 trait 接口 | | ✅ Tool → ToolExecutionContext |
| agent 之间消息传递 | | ✅ Blackboard（scope 内） |
| middleware 内部状态 | | ✅ Extensions（请求级） |
| 高频数据流（token delta） | | ✅ mpsc channel |

---

## Bus 数据流全景

### Agent 一次完整 prompt 的数据流

```
用户输入 "help me refactor"
        │
        ▼
  BaseAgent.prompt_text()
        │
        │  ┌──────────── Bus 上已注册 ────────────┐
        │  │ dyn TokenCounter (HeuristicCounter)  │
        │  │ dyn AgentLoopHook (PendingMsgQueue)  │
        │  │ ApprovalNotifier (if enabled)        │
        │  │ CheckpointCallbackRef (if set)       │
        │  └──────────────────────────────────────┘
        │
        ▼
  run_agent(state, config{bus: Some(handle)}, cancel, messages, event_tx)
        │
        ├─── Middleware: CompactionMiddleware.before_llm_call()
        │      │
        │      ├─ bus.get::<dyn TokenCounter>()  ← 真实 token 计数
        │      ├─ 超预算？→ bus.emit(TokenBudgetExceeded{...})
        │      ├─ LLM 摘要压缩
        │      └─ 压缩完成 → bus.emit(ContextCompacted{...})
        │
        ├─── Middleware: SecurityMiddleware.before_tool_call()
        │      │
        │      ├─ 需要审批？→ bus.get::<ApprovalNotifier>()
        │      └─ 发送 ApprovalRequest → UI 展示 → 用户决定
        │
        ├─── Middleware: CheckpointMiddleware.before_tool_call()
        │      │
        │      └─ bus.get::<CheckpointCallbackRef>() → 文件备份
        │
        ├─── Tool 执行
        │      │
        │      └─ tool.execute(input, ctx)
        │           │
        │           └─ ctx.bus()  ← tool 可以访问 bus 上任何能力
        │
        ├─── Steering 检查
        │      │
        │      └─ bus.get::<dyn AgentLoopHook>()?.take_steering()
        │
        └─── Follow-up 检查
               │
               └─ bus.get::<dyn AgentLoopHook>()?.take_follow_ups()
```

### Context 管理的 Bus 集成

```
  ContextHandleImpl（持有 bus: Option<BusHandle>）
        │
        ├─── inject_message()
        │      └─ bus.get::<dyn TokenCounter>()  ← 准确估算 token
        │
        ├─── query_memory() / store_memory() / delete_memory()
        │      └─ bus.get::<dyn MemoryBackend>()  ← 优先用 bus 上的
        │         └─ 降级 → self.memory（构造注入的）
        │
        └─── summarize()
               └─ bus.get::<dyn Summarizer>()  ← 优先用 bus 上的
                  └─ 降级 → self.summarize_fn（构造注入的）
                     └─ 降级 → 截断拼接
```

---

## 依赖防火墙

`scripts/ci-check-deps.sh` 自动检查边界规则：

```
Rule 0:  alva-kernel-bus 零 workspace 依赖
Rule 1:  alva-kernel-abi → alva-kernel-bus only
Rule 2:  alva-agent-context → alva-kernel-abi only
Rule 3:  alva-kernel-core → alva-kernel-abi only
Rule 4:  alva-agent-tools → alva-kernel-abi only
Rule 5:  alva-agent-security → alva-kernel-abi only
Rule 6:  alva-agent-memory → alva-kernel-abi only
Rule 7:  alva-host-native → foundation agent-* crates
Rule 8:  alva-agent-graph → alva-kernel-abi + alva-kernel-core
Rule 9:  alva-engine-runtime → alva-kernel-abi only
Rule 10: alva-engine-adapter-claude → alva-kernel-abi + engine-runtime
Rule 11: alva-engine-adapter-alva → alva-kernel-abi + engine-runtime + agent-core
Rule 12: protocol crates 不依赖 alva-app-*
Rule 13: alva-app 不直接依赖 agent-* 内部 crate（通过 facade）
```

**关键规则**：所有功能层 crate 只依赖 alva-kernel-abi。它们通过 Bus 横向通信，不需要互相依赖。

---

## 怎么用 Bus

### 场景 1：新增一个跨层能力

比如要加一个"代码索引服务"，让 tool 和 context 都能查询。

```rust
// 1. 在 alva-kernel-abi 定义 trait
pub trait CodeIndex: Send + Sync {
    fn search(&self, query: &str) -> Vec<CodeMatch>;
}

// 2. 在某个 crate（如 alva-agent-tools）实现
struct LocalCodeIndex { ... }
impl CodeIndex for LocalCodeIndex { ... }

// 3. 在 BaseAgent::build() 注册
bus_handle.provide::<dyn CodeIndex>(Arc::new(LocalCodeIndex::new(&workspace)));

// 4. 任何层按需使用（tool 里）
async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
    if let Some(bus) = ctx.bus() {
        if let Some(index) = bus.get::<dyn CodeIndex>() {
            let results = index.search(&query);
            // ...
        }
    }
}

// 5. 或者在 middleware 里
async fn before_llm_call(&self, state: &mut AgentState, messages: &mut Vec<Message>) {
    if let Some(ref bus) = self.bus {
        if let Some(index) = bus.get::<dyn CodeIndex>() {
            // 自动注入相关代码上下文
        }
    }
}
```

**改动量：1 个 trait + 1 个 impl + 1 行 provide + 按需 get。不改任何签名。**

### 场景 2：新增一个跨层事件

比如要通知"agent 开始执行工具"给 UI 和 metrics。

```rust
// 1. 在 alva-kernel-abi 定义事件
#[derive(Clone, Debug)]
pub struct ToolExecutionStarted {
    pub tool_name: String,
    pub session_id: String,
}
impl BusEvent for ToolExecutionStarted {}

// 2. 在 run.rs emit
if let Some(ref bus) = config.bus {
    bus.emit(ToolExecutionStarted {
        tool_name: tool_call.name.clone(),
        session_id: state.session.id().to_string(),
    });
}

// 3. 在 UI 层 subscribe
let mut rx = agent.bus().subscribe::<ToolExecutionStarted>();
tokio::spawn(async move {
    while let Ok(evt) = rx.recv().await {
        update_ui_tool_status(&evt.tool_name);
    }
});
```

**发送方不知道谁在听。接收方不知道谁在发。两边只共享 alva-kernel-abi 里的事件定义。**

### 场景 3：替换一个能力的实现

比如要把 HeuristicTokenCounter 换成真正的 tiktoken。

```rust
// 在 alva-provider 里实现
struct TiktokenCounter { encoding: tiktoken::Encoding }
impl TokenCounter for TiktokenCounter {
    fn count_tokens(&self, text: &str) -> usize {
        self.encoding.encode(text).len()
    }
    fn context_window(&self) -> usize { 200_000 }
}

// 在 BaseAgent::build() 里替换
// 旧：bus_handle.provide::<dyn TokenCounter>(Arc::new(HeuristicTokenCounter::new(200_000)));
// 新：
bus_handle.provide::<dyn TokenCounter>(Arc::new(TiktokenCounter::new("cl100k_base")));

// 所有消费方自动使用新实现 — context, compaction, 任何 bus.get::<dyn TokenCounter>() 的地方
```

**零改动。只换注册端，所有消费端自动生效。**

---

## alva-sandbox 沙箱层

```
┌─ alva-sandbox（core）────────────────────────────────────────────┐
│  Sandbox trait        — exec / read_file / write_file / list_dir │
│  SandboxProvider      — create / get / destroy                    │
│  SandboxAdapter       — 后端实现者接口                             │
│  SandboxCapability    — 可选能力（Stream / Sleep / Snapshot 等）   │
│  EnvPolicy            — 环境变量隔离（Inherit / Clean / Whitelist）│
└──────────────────────────────────────────────────────────────────┘
              ↑ impl SandboxAdapter
┌─ Adapter 实现 ───────────────────────────────────────────────────┐
│  alva-sandbox-local       — 本地 macOS/Linux（已实现）            │
│  alva-sandbox-docker      — Docker 容器（bollard）               │
│  alva-sandbox-e2b         — E2B 云沙箱（REST API）               │
│  alva-sandbox-v8          — V8 isolate（类 Cloudflare Workers）  │
│  alva-sandbox-cloudflare  — Cloudflare Workers                   │
│  (future)                 — iOS sandbox / WASM                   │
└──────────────────────────────────────────────────────────────────┘
```

**Sandbox 不知道谁跑在里面**——可以是我们的 Agent、Claude Code、任何 CLI 工具、Node.js 应用。

## ToolFs：连接 Agent 和 Sandbox 的桥

Agent 工具通过 `ToolFs` trait 操作文件和执行命令，不直接调用系统 API：

```
工具代码 → ToolFs trait (alva-kernel-abi 中定义)
                ↓
        ┌───────┴───────┐
    LocalToolFs      SandboxToolFs (alva-app-core 桥接)
    (tokio::fs)      (dyn Sandbox → exec/read/write)
```

- **Agent 框架不依赖 Sandbox 框架**——ToolFs 是纯抽象
- **Sandbox 框架不依赖 Agent 框架**——它只提供执行环境
- **alva-app-core 做桥接**——SandboxToolFs 实现 ToolFs，委托给 dyn Sandbox

## 引擎系统

EngineRuntime trait 统一不同的 Agent 引擎后端：

```
EngineRuntime trait（execute / cancel / respond_permission / capabilities）
        ↓ impl
┌───────┴──────────────────────┐
│  AlvaAdapter                  │  ClaudeAdapter
│  直接 Rust 调用               │  Node.js bridge + JSON-line
│  AgentEvent → RuntimeEvent    │  SDK message → RuntimeEvent
│  本地工具执行                  │  SDK 内部管理工具
│  CancellationToken 取消       │  stdin 信号取消
└───────────────────────────────┘
```

## Skill 系统

三级渐进加载，按 Agent 模板定义：

| Level | 内容 | 何时加载 | Token 开销 |
|:---:|------|---------|-----------|
| 1 | Metadata（name + description） | 始终驻留 system prompt | ~50-150 |
| 2 | Body（SKILL.md 完整内容） | 用户 prompt 触发后 | ~500-2000 |
| 3 | Resources（scripts / references） | Agent 按需调用 | 可变 |

注入策略：
- **Auto** — 只注入 metadata，Agent 用 `use_skill` 工具按需拉取
- **Explicit** — 直接注入完整 body 到 system prompt
- **Strict** — 同 Explicit + 限制只能用该 Skill 允许的工具

SkillInjectionMiddleware 可根据用户消息动态搜索并注入相关 Skill。

## 安全模型

```
┌─ 权限层 ─────────────────────────────────────────┐
│  SecurityGuard     — 工具调用前检查（allow/block）  │
│  PermissionManager — HITL 四选项审批               │
│  SensitivePathFilter — .env/证书/密钥路径过滤      │
│  AuthorizedRoots   — 允许的工作区根目录             │
│  → SecurityMiddleware 通过 Bus 获取 ApprovalNotifier│
└──────────────────────────────────────────────────┘

┌─ 沙箱层 ─────────────────────────────────────────┐
│  SandboxConfig     — macOS Seatbelt profile 生成   │
│  EnvPolicy         — 环境变量隔离策略              │
│  NetworkPolicy     — 网络访问控制                  │
│  (Docker/E2B 天然隔离)                             │
└──────────────────────────────────────────────────┘

Secrets 管理 — 独立 CLI 工具：
├── 以 npm 包 / 纯 JS / WASM 形式部署到沙箱
├── 通过网络 API 请求临时授权获取密钥
├── 每次访问有审计日志
└── 授权有 TTL，沙箱销毁后失效
```

## 参考设计

| 参考 | 我们学了什么 |
|------|------------|
| [Sandbank](https://github.com/chekusu/sandbank) | Sandbox Provider/Adapter 三层分离 + Capability 协商 |
| Claude Code | Skill 系统 + ACP 协议 + HITL 权限模型 |
| LangGraph | StateGraph + Pregel 图编排 |
| D-Bus / Android Context | Bus 跨层协调模式 — 能力注册 + 事件分发 |
