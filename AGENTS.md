# Alva

> Rust 实现的分层架构 AI Agent 框架。
> `alva-kernel-bus`（L0 leaf）→ `alva-kernel-abi`（L1 纯契约）→ `alva-kernel-core`（L2 kernel 循环）→ 5 个 L3 功能 Box：`tools` / `security` / `context` / `memory` / `graph`→ `alva-host-native` / `alva-host-wasm`（L4 装配，每目标一个）→ `alva-engine-*`（L4.5 引擎桥）→ `alva-app-*`（L5 应用）

> **⚠ 本项目采用分形文档协议，必须严格遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md) 中定义的三层文档规范。**

---

## 业务域清单

> **记忆锚点 — 看前缀就知道角色：**
> - **`alva-kernel-*` / 底座**：kernel 本体 + 契约 + bus，不装不行
> - **`alva-agent-*` / Box**：插在 kernel 扩展点上的功能包，可装可卸
> - **`alva-host-*` / 装配**：某个目标平台的 batteries-included 成品
> - **`alva-engine-*` / 引擎桥**：把不同 agent 引擎统一到 `EngineRuntime` trait 后面
> - **`alva-app-*` / 应用**：最终用户能跑起来的产品
> - **`alva-protocol-*` / 协议**：Skill / MCP / ACP 等对外协议
>
> **5 个 L3 Box 对应 5 个动词**：想让 agent **干活** → `tools`；**守规矩** → `security`；**记得住** → `memory`；**控制 prompt + 多 agent 协作** → `context`；**多步骤工作流编排** → `graph`。

### Kernel 底座（L0–L2）

| crate | 层级 | 职责 |
|------|------|------|
| `alva-kernel-bus` | L0 leaf | Bus / Caps（typed 能力注册）/ EventBus（typed pub/sub）/ StateCell（可观察状态）/ BusPlugin（两阶段：`register` + `start`）/ `BusWriter` vs `BusHandle` 编译期读写分离。无任何 agent/runtime 依赖 |
| `alva-kernel-abi` | L1 纯 trait 契约层 | 全部核心 trait 与值类型：`Tool` / `ToolRegistry` / `ToolExecutionContext` / `ToolFs` / `LanguageModel` / `Provider` / `ProviderRegistry` / `AgentSession` trait + `InMemorySession` / `scope::context::{ContextHooks, ContextHandle, ContextSystem, SessionAccess}` + 8 钩子生命周期 / `Message` / `AgentMessage` / `ContentBlock` / `ModelConfig` / `TokenCounter` + `HeuristicTokenCounter` / `CancellationToken` / `AgentError` |
| `alva-kernel-core` | L2 kernel 循环 | `run_agent` 双层循环（inner = LLM stream + tool 执行，outer = follow-up continuation via `PendingMessageQueue`）/ `AgentState`（可变：model / tools / session / extensions）与 `AgentConfig`（不可变：middleware / system_prompt / max_iterations / model_config / context_window / workspace / bus）读写分离 / 洋葱式 `MiddlewareStack`（before/after/wrap_llm_call、before/after/wrap_tool_call）/ `AgentLoopHook` + `PendingMessageQueue`（steering / follow-up 注入点）/ `RuntimeExecutionContext`（桥接 tool progress → `AgentEvent`）/ kernel 卫生 builtins：`DanglingToolCallMiddleware` + `LoopDetectionMiddleware` + `ToolTimeoutMiddleware` |

### 功能 Box（L3 — 每个都是一个可装可卸的插件包）

| crate | 动词 / 一句话 | 关键内容 |
|------|------|------|
| `alva-agent-tools` | **干活** | 40+ 个 `Tool` 实现，按 `tool_presets::*` 分组：`file_io`（read / create / file_edit / list / find / grep / view_image）、`shell`（execute_shell）、`interaction`（ask_human）、`task_management`（create / update / get / list / output / stop）、`team`（create / delete / send_message）、`planning`（enter/exit_plan_mode / todo_write）、`worktree`（enter / exit）、`utility`（sleep / config / notebook_edit / skill / tool_search / schedule_cron / remote_trigger）、`web`（internet_search / read_url，`feature = "native"`）、`browser_tools`（7 个 CDP 操作，`feature = "browser"`）。`LocalToolFs` 实现 `ToolFs` trait（`#[cfg(not(target_family = "wasm"))]`），`MockToolFs` 测试替身 |
| `alva-agent-security` | **守规矩** | `SecurityGuard`（路径过滤 + 授权根 + HITL 权限统一闸门）/ `PermissionManager`（session 级审批 + always-allow/deny 缓存 + async 审批流）/ `SensitivePathFilter`（密钥 / 证书 / 私有配置，dirs + 扩展名 + 文件名 + 正则）/ `AuthorizedRoots` / `SandboxConfig`（macOS Seatbelt profile）/ `PermissionRules` + `PermissionCache` + `PermissionMode` / `BashClassifier`（命令分类）/ `middleware/`：`SecurityMiddleware` + `PlanModeMiddleware` |
| `alva-agent-memory` | **记得住** | `MemoryBackend` trait + `MemorySqlite` 默认 impl（FTS5 全文 + 暴力向量检索 + embedding 缓存 + 文件/块 CRUD）/ `EmbeddingProvider` trait + `NoopEmbeddingProvider` / `MemoryService`（FTS + 向量混合检索，加权分数融合）/ workspace 扫描：`sync.rs` 扫 `MEMORY.md` / 切块 / 算 embedding / 索引 / `extract.rs` 提取 `ExtractedMemory` |
| `alva-agent-context` | **控制 prompt + 多 agent 协作** | **契约在 `alva-kernel-abi::scope::context`，实现在这里**。**核心 context 插件系统**：`ContextHooks` 8 钩子生命周期（bootstrap / on_message / on_budget_exceeded / assemble / ingest / after_turn / dispose）/ `ContextStore` **四层**容器（AlwaysPresent / OnDemand / RuntimeInject / Memory）/ `ContextHandle` SDK / `ContextHooksChain` / `RulesContextHooks` / `DefaultContextHooks`（LLM 回调 + deterministic fallback）/ `apply.rs` / `compact.rs` + `auto_compact.rs` / `default_context_system()`。**`middleware.rs`**：`CompactionMiddleware`（LLM-summarization 过渡版，与 ContextHooks 路径并存——用户启用 `with_context_token_budget` 即可切到 hook-driven 路径）。**`scope/` 子模块**（Phase 3 从原 `alva-agent-scope` 合入）：`Blackboard`（共享消息板 / @mention 风格协作）+ `BoardMessage` / `MessageKind` / `TaskPhase` + `AgentProfile` + `BlackboardPlugin`（实现 `ContextHooks`：bootstrap 注册 profile、assemble 注入最近 board 消息、after_turn 同步输出）+ `BoardRegistry`（按 `SpawnScope` ID 隔离多个 Blackboard）+ `SessionTracker`（spawn 树父子关系）+ `SpawnScopeImpl`。**Phase 4 已接入 `alva-kernel-core::run.rs`**：6 个钩子（bootstrap / on_message / assemble / on_budget_exceeded / after_turn / dispose）在 run loop 对应生命周期点被触发；on_message 返回的 Injection 由 pending_injections 缓冲区在下一轮 LLM 调用前经 apply_injections 落到 prompt；on_budget_exceeded 在 token 超限时由 apply_compressions 把 CompressAction 落到工作消息列表（kernel 通过 bus TokenCounter 估算 token） |
| `alva-agent-graph` | **多步骤工作流编排** | LangGraph 风格：`StateGraph<S>` builder（nodes / edges / conditional router / dynamic `SendTo` fan-out / merge fn）+ `CompiledGraph::invoke_with_config` Pregel BSP superstep executor（支持 checkpoint / retry / event streaming）+ `NodeResult::Update / Sends` + Channel 类型（`LastValue` / `EphemeralValue` / `BinaryOperatorAggregate`）+ `CheckpointSaver` + `InMemoryCheckpointSaver` + `RetryConfig` + 独立 `CompactionConfig` + `compact_messages` / `estimate_tokens`（按 AGENTS.md 明确声明是 standalone utility）+ `ContextTransform` / `TransformPipeline` + `GraphRun` wrapper struct（与 L1 的 `AgentSession` trait 区分） |

### 平台装配（L4）

| crate | 职责 |
|------|------|
| `alva-host-native` | Batteries-included 装配层（native 目标）：`AgentRuntimeBuilder` + `with_standard_agent_stack(SandboxMode)` 一键装配：`HeuristicTokenCounter` 注入 bus / `PendingMessageQueue` 作为 `AgentLoopHook` / 遍历 `BusPlugin::register` + `start` / 顺序装载 7 个 middleware / 注册 tool registry / 构造 `AgentState` + `AgentConfig` + `AgentRuntime`。`init::model("provider/id")` 一句话初始化 `LanguageModel`。`TokioSleeper` 作为 `Sleeper` 的 native impl，注入 `ToolTimeoutMiddleware`。middleware/：`CheckpointMiddleware`（host 持久化）+ 从 box 转发的 `SecurityMiddleware` / `PlanModeMiddleware`（来自 `alva-agent-security`）/ `CompactionMiddleware`（来自 `alva-agent-context`） |
| `alva-host-wasm` | 装配层（wasm32 目标）：`WasmAgent` facade（`new` / `run` / `run_simple` / `state` / `config_mut`）+ `WasmSleeper`（`spawn_local + oneshot` 桥接 non-Send `gloo_timers` future）+ `smoke::_wasm_smoke_probe` 编译期探针，强制 `cargo check --target wasm32` 穿透整条 kernel API 表面。**共享同一份 `alva-kernel-core`——kernel 一行不用动**。Phase 5 交付物，证明 "kernel 平台无关" 在实践上成立 |

### 引擎桥接层（L4.5 — 给 app 层用的统一引擎接口）

| crate | 职责 |
|------|------|
| `alva-engine-runtime` | `EngineRuntime` trait：`execute` / `cancel` / `respond_permission` / `capabilities`。让 app 层用同一套接口驱动多种 agent 后端 |
| `alva-engine-adapter-alva` | 本地 Alva agent 适配器：`AgentEvent → RuntimeEvent` 映射，直接 Rust 调用 `alva-host-native` |
| `alva-engine-adapter-claude` | Claude Code SDK 适配器：Node.js bridge + JSON-line 协议 |

### 应用层（L5）

| crate | 职责 |
|------|------|
| `alva-app` | GPUI 桌面 GUI：Sidebar / Chat / Markdown 渲染 / 主窗体 |
| `alva-app-core` | 薄 Facade：re-export 下层 crate + 保留 skills / mcp / environment / persistence / domain / evaluation |
| `alva-app-cli` | CLI 入口 |
| `alva-app-debug` | AI 调试系统：HTTP API / 日志捕获 / 视图树检查 / `traced!` 宏 |
| `alva-app-devtools-mcp` | MCP 服务器：包装 `alva-app-debug` HTTP API，供外部 IDE 调试 |
| `alva-app-eval` | Axum 本地 HTTP 服务 + 内嵌 SPA（rust-embed）+ SSE 流式 `AgentEvent`：本地 eval playground |

### 协议与基础设施

| crate | 职责 |
|------|------|
| `alva-protocol-skill` | Skill 系统：加载 / 注入 / 存储 / 渐进式三级加载 |
| `alva-protocol-mcp` | Model Context Protocol 客户端 + `McpToolAdapter`（把远端 MCP 工具桥接成 `Tool` trait） |
| `alva-protocol-acp` | Agent Client Protocol：消息类型 / 会话 / 连接 / 进程管理 |
| `alva-llm-provider` | LLM provider 实现：`AnthropicProvider` / `OpenAIChatProvider` / `OpenAIResponsesProvider` |
| `alva-environment` | 运行环境检测与抽象 |
| `alva-macros` | 过程宏：`#[derive(Tool)]` 等 |
| `alva-test` | 测试辅助：`MockLanguageModel` 等 |

### 工具与约束

| 项 | 职责 |
|------|------|
| `scripts/ci-check-deps.sh` | CI 强制 crate 边界规则，防止分层被破坏 |
| `docs/BUS-RULES.md` | bus 防退化规则（防止退化为 God Object） |
| `docs/ARCHITECTURE.md` | 三仓库架构设计：alva-sandbox + alva-agent + alva-app |
| `Cargo.toml` | Rust workspace，管理 26 个 crate |

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
