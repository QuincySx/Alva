# Alva

> Rust 实现的分层 AI Agent 框架。架构核心是**稳定的 SDK 内核 + 一切通过插件扩展**——
> kernel 和 agent-core 不接受功能扩展，所有可选行为都是 `Extension` / `Tool` / `Middleware`。

> **⚠ 本项目采用分形文档协议，必须严格遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md) 中定义的三层文档规范。**

---

## 扩展哲学（核心架构决策）

**Kernel + agent-core 是稳定 SDK，不接受功能扩展。** 任何"额外能力"都通过下面三种方式注入：

1. **Tool**（`alva-kernel-abi::Tool`）—— 让 LLM 能调用的"动词"。
2. **Extension**（`alva-agent-core::Extension`）—— 一组功能的封装，可注册 tool、
   middleware、bus 能力、事件 handler 与命令。**首选扩展方式**。
3. **Middleware**（`alva-kernel-core::Middleware`）—— 直接挂到 agent loop 的洋葱
   中间件。一般通过 Extension 间接注册。

### 默认替换契约

`BaseAgent` 自带几个默认 Extension（`MemoryExtension::default()`、
`SecurityExtension::for_workspace()`）。要替换其中任何一个，**注册一个 `name()` 相同的
Extension**——`BaseAgentBuilder::build()` 检测到同名时跳过默认。

```rust
// 替换默认 memory 后端
struct PostgresMemoryExt { /* ... */ }
impl Extension for PostgresMemoryExt {
    fn name(&self) -> &str { "memory" }   // ← 同名，默认被跳过
    async fn configure(&self, ctx: &ExtensionContext) {
        ctx.bus_writer.provide(Arc::new(self.service.clone()));
    }
}

BaseAgent::builder()
    .workspace("/path")
    .extension(Box::new(PostgresMemoryExt::new(...)))
    .build(model).await?;
```

**没有 `.with_memory()` / `.memory_service()` / `.security_middleware()` 这种 setter**。
那种 ad-hoc 开关的设计已经被全部砍掉。能用插件就用插件。

### `agent-graph` 是例外

`alva-agent-graph` 不是扩展系统的一部分，是一个 **LangGraph 风格的状态机库**——给
"多步骤工作流"（generator-evaluator / planner-executor / reflection 循环）这种比标准
agent loop 更复杂的场景用。它跟 `run_agent` 是平行的两套 runtime，互不依赖。
当前由 `alva-app-core::EvaluationExtension` 在内部使用。

### CI 强制边界（Rule 17）

`scripts/ci-check-deps.sh` 强制：所有 SDK crate 不得（传递）依赖任何 `alva-app-*` 或
`alva-host-*` crate。这是项目最重要的分层约束——SDK 必须能被第三方拿去搭自己的 harness，
不能拖一坨我们自己的 harness 决策。

---

## 业务域清单（按层）

> **看前缀就知道角色：**
> - **`alva-kernel-*`** —— 稳定内核。零扩展点之外的功能。
> - **`alva-agent-core`** —— 扩展系统的 trait + 装配 API。SDK 顶端。
> - **`alva-agent-{context,memory,security,graph}`** —— 能力库，被 extension 消费。
> - **`alva-agent-extension-builtin`** —— 内置 tool 实现 + 默认 extension 包装。
> - **`alva-app-extension-*`** —— 重依赖外挂（浏览器 / SQLite memory）。
> - **`alva-app-*`** —— Harness + 应用层（我们自己的"deer-flow"）。
> - **`alva-host-*`** —— 平台装配（native / wasm）。
> - **`alva-protocol-*`** —— Skill / MCP / ACP 等对外协议。
> - **`alva-engine-*`** —— 引擎桥接层。

### L0–L2.5：稳定 SDK 内核

| crate | 层 | 职责 |
|------|------|------|
| `alva-kernel-bus` | L0 | `Bus` / `Caps`（typed 能力注册）/ `EventBus`（typed pub/sub）/ `StateCell` / `BusPlugin`（两阶段：register + start）/ `BusWriter` vs `BusHandle` 编译期读写分离。零依赖。 |
| `alva-kernel-abi` | L1 纯契约 | 全部核心 trait + 值类型：`Tool` / `ToolRegistry` / `ToolFs` / `LanguageModel` / `Provider` / `AgentSession` + `InMemorySession` / `scope::context::{ContextHooks, ContextHandle, ContextSystem}` 8 钩子生命周期 / `Message` / `AgentMessage` / `ContentBlock` / `ModelConfig` / `TokenCounter` + `HeuristicTokenCounter` / `CancellationToken` / `AgentError`。 |
| `alva-kernel-core` | L2 agent loop | `run_agent` 双层循环（inner = LLM stream + tool 执行；outer = follow-up via `PendingMessageQueue`）/ `AgentState`（mut: model / tools / session / extensions）+ `AgentConfig`（immut: middleware / system_prompt / max_iter / model_config / context_window / workspace / bus）读写分离 / 洋葱式 `MiddlewareStack`（before/after/wrap_llm_call、before/after/wrap_tool_call）/ `AgentLoopHook` + `PendingMessageQueue` / `RuntimeExecutionContext`（tool progress → `AgentEvent`）/ kernel 卫生 builtins：`DanglingToolCallMiddleware` / `LoopDetectionMiddleware` / `ToolTimeoutMiddleware`。 |
| `alva-agent-core` | L2.5 SDK 装配 | **扩展系统的合约 + 装配入口**。`Extension` trait（`name` / `tools` / `activate` / `configure` / `finalize`）+ `HostAPI`（`middleware` / `on` / `register_command` / `steer` / `follow_up` / `shutdown`）+ `ExtensionHost` / `ExtensionContext` / `FinalizeContext` / `ExtensionEvent` + `ExtensionBridgeMiddleware`。`Agent` + `AgentBuilder`：纯 SDK 级 agent 装配（无任何 harness 决策），第三方搭自己 harness 就从这里入。`MockToolFs` 测试替身。 |

### L3：能力库（被 extension 消费的具体实现）

| crate | 一句话 | 关键内容 |
|------|------|------|
| `alva-agent-context` | 控制 prompt + 多 agent 协作 | **核心 context 插件系统**：`ContextHooks` 8 钩子生命周期 / `ContextStore` 四层容器（AlwaysPresent / OnDemand / RuntimeInject / Memory）/ `ContextHandle` SDK / `RulesContextHooks` / `DefaultContextHooks`（LLM 回调 + 确定性 fallback）/ `compact.rs` + `auto_compact.rs` / `default_context_system()`。**`middleware.rs`** `CompactionMiddleware`（hook-driven 路径并存）。**`scope/` 子模块**：`Blackboard`（共享消息板 / @mention 协作）+ `BoardMessage` / `MessageKind` / `TaskPhase` + `AgentProfile` + `BlackboardPlugin` + `BoardRegistry`（按 `SpawnScope` 隔离）+ `SessionTracker`（spawn 树父子关系）+ `SpawnScopeImpl`。已接入 `kernel-core::run.rs` 6 个钩子点。 |
| `alva-agent-memory` | 记得住 | `MemoryBackend` trait + **`InMemoryBackend` 默认实现**（HashMap + Mutex，零依赖，纯 Rust，进程 lifetime）+ `MemoryService`（FTS + 向量混合检索，加权分数融合）+ `EmbeddingProvider` trait + `NoopEmbeddingProvider` + 数据类型 `MemoryFile` / `MemoryChunk` / `MemoryEntry` / `SyncReport`。**重的持久化实现（SQLite + 工作区扫描 + 提取）住在 `alva-app-extension-memory`**。 |
| `alva-agent-security` | 守规矩 | `SecurityGuard`（路径过滤 + 授权根 + HITL 权限统一闸门）/ `PermissionManager`（session 级审批 + always-allow/deny 缓存 + async 审批流）/ `SensitivePathFilter`（密钥 / 证书 / 私有配置）/ `AuthorizedRoots` / `SandboxConfig`（macOS Seatbelt profile）/ `PermissionRules` + `PermissionCache` + `PermissionMode`（Ask / AcceptEdits / Plan）/ `BashClassifier` / `ApprovalNotifier` + `ApprovalRequest`。`middleware/`：`SecurityMiddleware`（OnceLock 拿 bus）+ `PlanModeMiddleware`。 |
| `alva-agent-graph` | **【例外】** 多步骤工作流编排 | LangGraph 风格状态机库——**不是 extension**。`StateGraph<S>` builder（nodes / edges / conditional router / dynamic `SendTo` fan-out / merge fn）+ `CompiledGraph::invoke_with_config` Pregel BSP superstep executor + `NodeResult::Update / Sends` + Channel 类型（`LastValue` / `EphemeralValue` / `BinaryOperatorAggregate`）+ `CheckpointSaver` + `InMemoryCheckpointSaver` + `RetryConfig` + `CompactionConfig` + `ContextTransform` / `TransformPipeline`。当前消费者：`alva-app-core::EvaluationExtension`、`alva-host-native::graph` re-export。 |

### L4：扩展实现（"插件"——真正可装可卸的功能）

| crate | 职责 |
|------|------|
| `alva-agent-extension-builtin` | **内置 tool + 默认 extension wrapper**。`tools/` 目录 40+ 个 `Tool` 实现，按 feature 分组（`core` = file_io + shell + interaction + plan primitives；`utility` = config / skill / tool_search / sleep；`web` = internet_search + read_url；`notebook` / `worktree` / `team` / `task` / `schedule` / `browser`）。`tool_presets::*` 函数把工具按组打包。`wrappers/` 目录的 11 个 `Extension` 包装：`CoreExtension` / `ShellExtension` / `InteractionExtension` / `TaskExtension` / `TeamExtension` / `PlanningExtension` / `UtilityExtension` / `WebExtension` / `BrowserExtension` —— 这些是 tool 组的 Extension 形式。**`MemoryExtension::default()`**（默认用 `InMemoryBackend`，可被同名 extension 替换）。**`SecurityExtension::for_workspace()`**（默认 sandbox 中间件）。`LocalToolFs`：本地 OS `ToolFs` 适配器，native-only。`register_builtin_tools(registry)` legacy shim。 |
| `alva-app-extension-browser` | **重依赖外挂**：基于 `chromiumoxide` 的浏览器自动化（CDP），独立 crate 隔离 mio 依赖，wasm 不可用。 |
| `alva-app-extension-memory` | **重依赖外挂**：基于 `rusqlite` + 捆绑 SQLite 的持久化 memory backend (`MemorySqlite`) + 工作区扫描 + LLM 提取。**不是默认 backend**——想要持久化的用户显式注册一个同名 `Extension` 来替换 `MemoryExtension::default()`。 |

### L5：Harness 装配（把上面的拼起来变成可跑的 agent）

| crate | 职责 |
|------|------|
| `alva-app-core` | **预设 harness 层（"我们的 deer-flow"）**。核心：`BaseAgent`（**4 字段**：`inner: Arc<Agent>` / `current_cancel` / `pending_messages` / `bus_writer`）+ `BaseAgentBuilder`（薄薄一层包装，`build()` 内部委托给 `alva_agent_core::AgentBuilder`，自动塞入默认 `MemoryExtension` + `SecurityExtension` 除非用户已注册同名 extension）。`extension/` 子目录持有协议 + middleware extensions：`SkillsExtension` / `McpExtension` / `HooksExtension` / `EvaluationExtension` / `SubAgentExtension`（folder-based）+ `LoopDetectionExtension` / `DanglingToolCallExtension` / `ToolTimeoutExtension` / `CompactionExtension` / `CheckpointExtension` / `PlanModeExtension` / `AnalyticsExtension` / `AuthExtension` / `LspExtension` / `ApprovalExtension`（flat）。`PermissionModeService` + `PermissionMode` 现位于 bus 上，由 `PlanModeExtension::configure()` 注册。`base_agent/` 模块。 |
| `alva-host-native` | Native 平台装配。`AgentRuntimeBuilder` + `with_standard_agent_stack(SandboxMode)` 一键装配（legacy/parallel 路径，跟 BaseAgentBuilder 平行存在）。`init::model("provider/id")` 一句话初始化 `LanguageModel`。`TokioSleeper` 作为 native `Sleeper`。`middleware/`：`CheckpointMiddleware`（host 持久化）+ 从 box 转发的 `SecurityMiddleware` / `PlanModeMiddleware` / `CompactionMiddleware`。 |
| `alva-host-wasm` | wasm32 装配。`WasmAgent` facade + `WasmSleeper`（spawn_local + oneshot 桥接 non-Send `gloo_timers` future）+ `smoke::_wasm_smoke_probe` 编译期探针，强制 `cargo check --target wasm32-unknown-unknown` 穿透整条 kernel API 表面。**共享同一份 `alva-kernel-core`，kernel 一行不用动**。 |

### L4.5：引擎桥接（给 app 层用的统一引擎接口）

| crate | 职责 |
|------|------|
| `alva-engine-runtime` | `EngineRuntime` trait：`execute` / `cancel` / `respond_permission` / `capabilities`。让 app 层用同一套接口驱动多种 agent 后端。 |
| `alva-engine-adapter-alva` | 本地 Alva agent 适配器：`AgentEvent → RuntimeEvent` 映射，直接 Rust 调用 `alva-host-native`。 |
| `alva-engine-adapter-claude` | Claude Code SDK 适配器：Node.js bridge + JSON-line 协议。 |

### L5：协议与基础设施

| crate | 职责 |
|------|------|
| `alva-protocol-skill` | Skill 系统：加载 / 注入 / 存储 / 渐进式三级加载。 |
| `alva-protocol-mcp` | Model Context Protocol 客户端 + `McpToolAdapter`（把远端 MCP 工具桥接成 `Tool` trait）。 |
| `alva-protocol-acp` | Agent Client Protocol：消息类型 / 会话 / 连接 / 进程管理。 |
| `alva-llm-provider` | LLM provider 实现：`AnthropicProvider` / `OpenAIChatProvider` / `OpenAIResponsesProvider`。 |
| `alva-environment` | 运行环境检测与抽象。 |
| `alva-macros` | 过程宏：`#[derive(Tool)]` 等。 |
| `alva-test` | 测试辅助：`MockLanguageModel` 等。 |

### L6：应用层（最终用户能跑起来的产品）

| crate | 职责 |
|------|------|
| `alva-app` | GPUI 桌面 GUI：Sidebar / Chat / Markdown 渲染 / 主窗体。 |
| `alva-app-cli` | CLI 入口（`pi` 风格的交互式终端 + REPL）。 |
| `alva-app-debug` | AI 调试系统：HTTP API / 日志捕获 / 视图树检查 / `traced!` 宏。 |
| `alva-app-devtools-mcp` | MCP 服务器：包装 `alva-app-debug` HTTP API，供外部 IDE 调试。 |
| `alva-app-eval` | Axum 本地 HTTP 服务 + 内嵌 SPA（rust-embed）+ SSE 流式 `AgentEvent`：本地 eval playground。 |

---

## 两种入口

```rust
// 入口 A：SDK 级（用户搭自己的 harness）
use alva_agent_core::{Agent, AgentBuilder};
use alva_agent_extension_builtin::wrappers::{CoreExtension, ShellExtension};

let agent = Agent::builder()
    .model(my_model)
    .extension(Box::new(CoreExtension))
    .extension(Box::new(MyCustomExtension))
    .system_prompt("...")
    .build()
    .await?;

let events = agent.run(input, cancel).await?;
```

```rust
// 入口 B：Harness 级（用我们的预设）
use alva_app_core::BaseAgent;

let agent = BaseAgent::builder()
    .workspace("/path")
    .sandbox_mode(SandboxMode::RestrictiveOpen)
    // 默认自带 MemoryExtension (InMemoryBackend) + SecurityExtension
    // 想换 memory？.extension(Box::new(MyMemoryExt))，name() 撞车自动跳过默认
    .build(my_model)
    .await?;
```

两条路最终都跑在 `alva-kernel-core::run_agent` 上。

---

## 工具与约束

| 项 | 职责 |
|------|------|
| `scripts/ci-check-deps.sh` | CI 强制 crate 边界规则。Rule 17 = SDK 不得依赖 app/host。19 个 crate 必须 wasm32-clean。 |
| `docs/BUS-RULES.md` | bus 防退化规则（防止退化为 God Object）。 |
| `docs/ARCHITECTURE.md` | 三仓库架构设计：alva-sandbox + alva-agent + alva-app。 |
| `Cargo.toml` | Rust workspace，管理 29 个 crate。 |

---

# 项目架构

> 详细架构设计见 [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)

## GPUI
Use when building GPUI components, custom elements, managing state/entities, working with contexts, handling events/subscriptions, async tasks, global state, actions/keybindings, focus management, layout/styling, code style conventions, or writing GPUI tests.
`docs/gpui/index.md`

## Git Commit 规范

1. **小步提交**：每个逻辑改动单独一个 commit，不要攒多个改动一起提交。方便后续 bisect、revert 和 review。
2. **说清楚改了什么**：commit message 第一行写改动内容，用 `feat:` / `fix:` / `refactor:` / `chore:` 前缀区分类型。
3. **写清楚为什么改**：如果改动原因不是显而易见的，在 commit message body 里补充原因。格式：第一行摘要，空一行，然后写原因。

示例：

```
refactor: rename MessageInjector → PendingMessageQueue

"Injector" 是依赖注入框架术语，在 agent 消息队列场景下不直观。
PendingMessageQueue（待处理消息队列）一读就懂。
```

```
fix: remove dead Steering branch from LLM filter

Steering 消息在注入 session 前已转成 Standard，
session 里不会出现 Steering 变体，这个 match 分支永远不会命中。
```

# alva-kernel-bus 防破坏规则
> Bus 是跨层协调总线，不是万能通道。本文档定义它的边界，防止退化为 God Object。
./docs/BUS-RULES.md

## Compact Instructions 如何保留关键信息
保留优先级：
1. 架构决策，不得摘要
2. 已修改文件和关键变更
3. 验证状态，pass/fail
4. 未解决的 TODO 和回滚笔记
5. 工具输出，可删，只保留 pass/fail 结论
