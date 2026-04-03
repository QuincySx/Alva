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
│  │  alva-app-core     — 应用编排层（Agent 生命周期 + Skills + 持久化）│    │
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
┌─ alva-agent-bus ─────────────────────────────────────────────────┐
│  Bus, BusHandle, Caps, EventBus, BusEvent, StateCell             │
│  （跨层协调总线，零 workspace 依赖）                                │
└──────────────────────────────────────────────────────────────────┘
              ↑ 唯一依赖
┌─ alva-types ─────────────────────────────────────────────────────┐
│  Message, ContentBlock, Tool, LanguageModel, ToolFs              │
│  AgentMessage, StreamEvent, ToolCall, ToolOutput                 │
│  TokenCounter, ToolExecutionContext（含 bus() 方法）              │
│  TokenBudgetExceeded, ContextCompacted, MemoryExtracted          │
│  Bus, BusHandle, BusEvent, StateCell（re-export）                │
│  （共享词汇表 — 所有 crate 通过依赖 alva-types 自动获得 bus）       │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 功能层（并行，互不依赖）─────────────────────────────────────────┐
│  alva-agent-context  — 上下文管理 Hooks + ContextStore + 四层模型 │
│  alva-agent-core     — Agent 循环引擎 + Middleware 洋葱模型       │
│  alva-agent-tools    — 16 内置工具（通过 ToolFs 抽象）            │
│  alva-agent-security — SecurityGuard + PermissionManager          │
│  alva-agent-memory   — FTS + 向量搜索 + MemoryBackend trait       │
│  alva-agent-graph    — StateGraph + Pregel + Channel + SubAgent   │
│  alva-agent-scope    — SpawnScope + Blackboard                    │
└──────────────────────────────────────────────────────────────────┘
              ↑ 依赖
┌─ 组装层 ─────────────────────────────────────────────────────────┐
│  alva-agent-runtime  — AgentRuntimeBuilder（组合所有功能层 + 默认装配 Bus + 标准 Agent Stack）│
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
Rule 0:  alva-agent-bus 零 workspace 依赖
Rule 1:  alva-types → alva-agent-bus only
Rule 2:  alva-agent-context → alva-types only
Rule 3:  alva-agent-core → alva-types only
Rule 4:  alva-agent-tools → alva-types only
Rule 5:  alva-agent-security → alva-types only
Rule 6:  alva-agent-memory → alva-types only
Rule 7:  alva-agent-runtime → foundation agent-* crates
Rule 8:  alva-agent-graph → alva-types + alva-agent-core
Rule 9:  alva-engine-runtime → alva-types only
Rule 10: alva-engine-adapter-claude → alva-types + engine-runtime
Rule 11: alva-engine-adapter-alva → alva-types + engine-runtime + agent-core
Rule 12: protocol crates 不依赖 alva-app-*
Rule 13: alva-app 不直接依赖 agent-* 内部 crate（通过 facade）
```

**关键规则**：所有功能层 crate 只依赖 alva-types。它们通过 Bus 横向通信，不需要互相依赖。

---

## 怎么用 Bus

### 场景 1：新增一个跨层能力

比如要加一个"代码索引服务"，让 tool 和 context 都能查询。

```rust
// 1. 在 alva-types 定义 trait
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
// 1. 在 alva-types 定义事件
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

**发送方不知道谁在听。接收方不知道谁在发。两边只共享 alva-types 里的事件定义。**

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
工具代码 → ToolFs trait (alva-types 中定义)
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
