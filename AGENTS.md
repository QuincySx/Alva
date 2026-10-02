# Alva

> Rust 实现的分层 AI Agent 框架。架构核心是**稳定的 SDK 内核 + 一切通过插件扩展**——
> kernel 和 agent-core 不接受功能扩展，所有可选行为都是 `Tool` / `Plugin` / `Middleware`。

> **⚠ 本项目采用分形文档协议，必须严格遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md) 中定义的三层文档规范。**

---

## 扩展哲学（核心架构决策）

**Kernel + agent-core 是稳定 SDK，不接受功能扩展。** 任何"额外能力"都通过下面三种方式注入：

1. **Tool**（`alva-kernel-abi::Tool`）—— 让 LLM 能调用的"动词"。
2. **Plugin**（`alva-agent-core::extension::Plugin`）—— 一组功能的封装，可注册 tool、
   middleware、bus 能力、system prompt 片段与命令元数据。**首选扩展方式**。
3. **Middleware**（`alva-kernel-core::Middleware`）—— 直接挂到 agent loop 的洋葱
   中间件。一般通过 Plugin 间接注册，单点中间件也可以直接 `.middleware()`。

### 默认替换契约

`BaseAgent` 自带几个默认 Plugin（`MemoryPlugin::default()`、
`SecurityPlugin::for_workspace()`、`SystemContextPlugin::new()`）。要替换其中任何一个，**注册一个 `name()` 相同的
Plugin**——`BaseAgentBuilder::build()` 检测到同名时跳过默认。

```rust
// 替换默认 memory 后端
struct PostgresMemoryPlugin { /* ... */ }
impl Plugin for PostgresMemoryPlugin {
    fn name(&self) -> &str { "memory" }   // ← 同名，默认被跳过
    async fn register(&self, r: &Registrar) {
        r.provide(Arc::new(self.service.clone()));
    }
}

BaseAgent::builder()
    .workspace("/path")
    .plugin(Box::new(PostgresMemoryPlugin::new(...)))
    .build(model).await?;
```

**没有 `.with_memory()` / `.memory_service()` / `.security_middleware()` 这种 setter**。
那种 ad-hoc 开关的设计已经被全部砍掉。能用插件就用插件。

### `agent-graph` 是例外

`alva-agent-graph` 不是扩展系统的一部分，是一个 **LangGraph 风格的状态机库**——给
"多步骤工作流"（generator-evaluator / planner-executor / reflection 循环）这种比标准
agent loop 更复杂的场景用。它跟 `run_agent` 是平行的两套 runtime，互不依赖。
当前由 `alva-app-core::EvaluationPlugin` 在内部使用。

### CI 强制边界（Rule 17 / Rule 18）

`scripts/ci-check-deps.sh` 强制：

- **Rule 17**：所有 SDK crate 不得（传递）依赖任何 `alva-app-*` 或 `alva-host-*`
  crate。SDK 必须能被第三方拿去搭自己的 harness，不能拖一坨我们自己的 harness 决策。
- **Rule 19**：内核（`alva-kernel-abi` / `alva-kernel-core`）不得（传递）依赖
  `alva-agent-abi`——"极其通用的内核"由 CI 持续验证（ADR-0008）。
- **Rule 18**：原生产品入口 `alva-app-cli` / `alva-app-tauri` 不得（传递）依赖任何
  `alva-engine-*`。`EngineRuntime` 只服务 Hub 等多引擎产品；CLI/Tauri 继续直连
  `BaseAgent`，避免 multi-engine 合约变更同时打穿三端。
- **Rule 19**：内核（`alva-kernel-abi` / `alva-kernel-core`）不得（传递）依赖
  `alva-agent-abi`。任务、作用域、派生、阶段、埋点、provider 注册表是
  "我们这一类 agent 产品"的概念，不是"循环怎么转"的机制——这条规则让
  "极其通用的内核"成为 CI 可验证的事实（内核边界重构 ticket 16）。

---

## 业务域清单（按层）

> **看前缀就知道角色：**
> - **`alva-kernel-*`** —— 稳定内核。零扩展点之外的功能。
> - **`alva-agent-core`** —— 扩展系统的 trait + 装配 API。SDK 顶端。
> - **`alva-agent-{context,memory,security,graph}`** —— 能力库，被 plugin 消费。
> - **`alva-agent-extension-builtin`** —— 内置 tool 实现 + 默认 plugin 包装。
> - **`alva-app-extension-*`** —— 重依赖外挂（浏览器 / SQLite memory）。
> - **`alva-app-*`** —— Harness + 应用层（我们自己的"deer-flow"）。
> - **`alva-host-*`** —— 平台装配（native / wasm）。
> - **`alva-sandbox-*`** —— Native-only 沙箱宿主后端，按 job 显式授予能力。
> - **`alva-protocol-*`** —— Skill / MCP / ACP 等对外协议。
> - **`alva-engine-*`** —— 引擎桥接层。

### L0–L2.5：稳定 SDK 内核

| crate | 层 | 职责 |
|------|------|------|
| `alva-llm-wire` | L0 | LLM 线格式类型：`Message` / `ContentBlock` / `UsageMetadata` / `AgentMessage` 等纯 serde 值类型；同时提供 kernel/guest 共用的 `StreamMessageAccumulator` 与版本化、限额化 WASIp1 LLM proxy DTO。零 workspace 依赖，wasm-clean，被 kernel-abi re-export。 |
| `alva-sandbox-abi` | L0 | Sandbox guest/host 共享的纯 serde 能力 ABI；当前定义版本化、限额化 host→guest environment context、fetch 与 host escalation 请求/响应/错误信封及单向 audit event。零 workspace 依赖、wasm-clean，不拥有网络、授权、执行或日志持久化策略。 |
| `alva-kernel-bus` | L0 | `Bus` / `Caps`（typed 能力注册）/ `EventBus`（typed pub/sub）/ `StateCell` / `BusPlugin`（两阶段：register + start）/ `BusWriter` vs `BusHandle` 编译期读写分离。零依赖。 |
| `alva-kernel-abi` | L1 纯契约 | 内核循环词汇：`Tool`（7 方法：name/description/parameters_schema/execute/scheduling/describe_effect/is_read_only）+ `Scheduling` 调度描述符 / 确定性顺序且可检测同名替换的 `ToolRegistry` / `ToolFs` / `LanguageModel` / **`ConversationLog`（内核对历史的全部要求，读写可失败）** / **`SessionStore`**（harness/UI 超集需求）/ `AgentSession`（二者组合标记）/ **`PersistentSessions`**（内核循环与工具路径唯一认识的会话能力，R24/R27 起 tool path 只见 `Arc<dyn PersistentSessions>`）/ `SessionRecord` typed 记录 / `scope::context::{ContextHooks, ContextHandle, ContextSystem}` 生命周期 + `ContextApply` 执行器 / `Message` / `AgentMessage` / `ContentBlock` / `ModelConfig` / `TokenCounter` / `CancellationToken` / `AgentError`（含 `Session` 变体）。任务/作用域/派生/阶段/埋点/provider 注册表在 `alva-agent-abi`（ticket 16）。 |
| `alva-kernel-core` | L2 agent loop | `run_agent` 双层循环（inner = LLM stream + tool batch 执行；outer = 透传外壳，follow-up/steering 已迁至 `alva-app-core::PendingPlugin`）＋可替换的 `AgentLoop` trait / `DefaultAgentLoop` 默认实现（ADR-0007，loop 可替换）/ `AgentState`（mut: model / tools / session / extensions）+ `AgentConfig`（immut: middleware / system_prompt / max_iter / model_config / context_window / workspace / bus）读写分离 / `ContextRuntime`（pending injection / assemble / budget compression 的 prepare-LLM 阶段协调器）/ `ToolBatchCoordinator`（显式 concurrency-safe 且资源无冲突的调用并行执行，intent/outcome 仍按模型顺序提交，中途取消补齐 cancelled result）/ `AgentEventSender`（兼容 unbounded SDK，原生产品 bounded lossless 背压；同步 tool progress 满载时允许丢弃）/ LLM session 审计（版本化 cache-base/tool-schema 指纹 + provider cache outcome + 每响应至多两条无正文首输出/流摘要）/ 洋葱式 `MiddlewareStack`（所有 lifecycle/before/after/wrap hook 的 identity/order/inclusive duration/结果按阶段打包为一条 `middleware_hook_batch`，避免逐层 SQLite 写放大）/ `RuntimeExecutionContext`（tool progress → `AgentEvent`）/ kernel 卫生 builtins：`DanglingToolCallMiddleware` / `LoopDetectionMiddleware` / `ToolTimeoutMiddleware`。 |
| `alva-agent-abi` | L2.3 Agent 契约 | **Agent 产品概念契约**（ticket 12 新建，16 收口）：Task（任务）/ Scope + Spawn（作用域与派生通信）/ Phase（阶段时间线）/ Analytics（埋点）/ Provider 注册表 / TokenEstimator。这些类型描述"我们这一类 agent 产品长什么样"，不是"循环怎么转"——第三方拿内核搭不同形态的 agent 时可整层不用。依赖内核契约层，被 SDK 装配层消费；内核契约层不得依赖本层（CI Rule 19，ticket 16 焊死）。`scope::context` 与 `session_record` 是内核循环/对话记录词汇，留在 `alva-kernel-abi`。 |
| `alva-agent-core` | L2.5 SDK 装配 | **Plugin 系统的合约 + 装配入口**。`Plugin` trait（兼容 `register` / `finalize`，生产实现可用可失败的 `try_register` / `try_finalize`）+ `Registrar`（单一跨层装配句柄：`tool` / `tools` / `phase`（metadata-only）/ `phase_handler`（executable）/ `middleware` / `provide` / `system_prompt` / `command` + `bus` / `bus_writer` / `workspace` 只读访问）+ `PhaseContribution` / `PhaseHandler`（目标 timeline 来自 `alva_kernel_abi::{Phase, PhaseEffect}`，agent-core 只拥有 `PhaseOrder` 与装配/编译逻辑）+ `LateContext`（`finalize` 晚期接线上下文）+ `PluginHost`（运行期 middleware/command/phase contribution 容器）+ `AgentAssemblySnapshot` / `PluginAssemblySnapshot`（装配可观测性）。`AgentBuilder` 对 plugin/tool 重名和不支持 atomic records 的 tool session fail-fast；`Agent::run` 保留 executor-neutral unbounded 兼容流，原生 `Agent::run_bounded` 提供真实生产背压。事件层已删除（plugin 只产出 phase/middleware/tool/bus 能力，不再有 `on` / `ExtensionEvent` / `ExtensionBridgeMiddleware`）。`Agent` + `AgentBuilder`：纯 SDK 级 agent 装配（无任何 harness 决策），第三方搭自己 harness 就从这里入。`MockToolFs` 测试替身。 |

### L3：能力库（被 plugin 消费的具体实现）

| crate | 一句话 | 关键内容 |
|------|------|------|
| `alva-agent-context` | 控制 prompt + 多 agent 协作 | **核心 context 插件系统**：`ContextHooks` 7 钩子生命周期 / `ContextStore` 四层容器（AlwaysPresent / OnDemand / RuntimeInject / Memory）/ `ContextHandle` SDK / `RulesContextHooks` / `DefaultContextHooks`（LLM 回调 + 确定性 fallback）/ `compact.rs` + `auto_compact.rs` / `default_context_system()`。**`middleware.rs`** `CompactionMiddleware`（hook-driven 路径并存）。**`scope/` 子模块**：`Blackboard`（共享消息板 / @mention 协作）+ `BoardMessage` / `MessageKind` / `TaskPhase` + `AgentProfile` + `BlackboardPlugin` + `BoardRegistry`（按 `SpawnScope` 隔离）+ `SessionTracker`（spawn 树父子关系）+ `SpawnScopeImpl`。运行期由 `kernel-core::ContextRuntime` 在 prepare-LLM/on_message/after_turn/dispose 阶段协调。 |
| `alva-agent-memory` | 记得住 | `MemoryBackend` trait + **`InMemoryBackend` 默认实现**（HashMap + Mutex，零依赖，纯 Rust，进程 lifetime）+ `MemoryService`（FTS + 向量混合检索，加权分数融合）+ `EmbeddingProvider` trait + `NoopEmbeddingProvider` + 数据类型 `MemoryFile` / `MemoryChunk` / `MemoryEntry` / `SyncReport`。**重的持久化实现（SQLite + 工作区扫描 + 提取）住在 `alva-app-extension-memory`**。 |
| `alva-agent-security` | 守规矩 | `SecurityGuard`（路径过滤 + 授权根 + HITL 权限统一闸门）/ `PermissionManager`（session 级审批 + always-allow/deny 缓存 + async 审批流）/ `SensitivePathFilter`（密钥 / 证书 / 私有配置）/ `AuthorizedRoots` / `SandboxConfig`（macOS Seatbelt profile）/ `PermissionRules` + `PermissionCache` + `PermissionMode`（Ask / AcceptEdits / Plan）/ `BashClassifier` / `ApprovalNotifier` + `ApprovalRequest`。`middleware/`：`SecurityMiddleware`（OnceLock 拿 bus）+ `PlanModeMiddleware`。 |
| `alva-agent-graph` | **【例外】** 多步骤工作流编排 | LangGraph 风格状态机库——**不是 extension**。`StateGraph<S>` builder（nodes / edges / conditional router / dynamic `SendTo` fan-out / merge fn）+ `CompiledGraph::invoke_with_config` Pregel BSP superstep executor + `NodeResult::Update / Sends` + Channel 类型（`LastValue` / `EphemeralValue` / `BinaryOperatorAggregate`）+ `CheckpointSaver` + `InMemoryCheckpointSaver` + `RetryConfig` + `CompactionConfig` + `ContextTransform` / `TransformPipeline`。当前消费者：`alva-app-core::EvaluationExtension`、`alva-host-native::graph` re-export。 |

### L4：扩展实现（"插件"——真正可装可卸的功能）

| crate | 职责 |
|------|------|
| `alva-agent-extension-builtin` | **内置 tool + 默认 Plugin wrapper**。`tools/` 目录 40+ 个 `Tool` 实现，按 feature 分组（`core` = file_io + shell + interaction + plan primitives；`utility` = config / tool_search / sleep；统一 `skill` tool 由 `SkillsPlugin` 注册并通过 bus registry 接线；`web` = internet_search + read_url；`notebook` / `worktree` / `team` / `task` / `schedule` / `browser`）。`tool_presets::*` 函数把工具按组打包。`wrappers/` 目录的 `Plugin` 包装：`CorePlugin` / `ShellPlugin` / `InteractionPlugin` / `TaskPlugin` / `TeamPlugin` / `PlanningPlugin` / `UtilityPlugin` / `WebPlugin` / `BrowserPlugin` / `SystemContextPlugin` —— 这些是 tool 组的 `Plugin` 形式。**`MemoryPlugin::default()`**（默认用 `InMemoryBackend`，可被同名 plugin 替换）。**`SecurityPlugin::for_workspace()`**（默认 sandbox 中间件）。纯中间件壳（旧 `LoopDetectionExtension` / `DanglingToolCallExtension` / `ToolTimeoutExtension` / `CompactionExtension` / `CheckpointExtension`）已删除——这些单点 middleware 现在直接 `.middleware()` 注册，不再包 Plugin 壳。`LocalToolFs`：本地 OS `ToolFs` 适配器，native-only；前台命令有界捕获并在取消/超时/退出时清理进程组，显式 background 路径断开 stdio 后独立存活并异步回收；`WasiFs`：WASI 同步文件系统适配器（无子进程）。`register_builtin_tools(registry)` legacy shim。 |
| `alva-app-extension-browser` | **重依赖外挂**：基于 `chromiumoxide` 的浏览器自动化（CDP），独立 crate 隔离 mio 依赖，wasm 不可用。 |
| `alva-app-extension-memory` | **重依赖外挂**：基于 `rusqlite` + 捆绑 SQLite 的持久化 memory backend (`MemorySqlite`) + 工作区扫描 + LLM 提取。**不是默认 backend**——想要持久化的用户显式注册一个同名 `Plugin` 来替换 `MemoryPlugin::default()`。 |
| `alva-app-extension-loader` | **第三方子进程插件加载器**：实现 AEP（Alva Extension Protocol，JSON-RPC 2.0 over stdio），把 Python / JS / 其它语言写的插件作为子进程加载成标准 `Plugin`。`SubprocessLoaderPlugin` 扫描 `~/.config/alva/extensions/` + `<workspace>/.alva/extensions/`；manifest/entry 严格校验，bundle 有目录项/文件数/字节/深度上限并拒绝 symlink，启动前复制为 owner-only snapshot，可选 `content_sha256` fail-closed pin，子进程清空宿主环境后只注入安全 PATH/locale 和显式 override；AEP handshake identity/capability/tool/subscription、frame/queue/callback 都有 fail-closed 校验或硬上限，callback task 由 dispatcher 管理并回收。Native-only（wasm 下编译为空 lib）。插件作者不用写 Rust——配套 SDK 在 `sdk/python`（`alva_sdk`）/ `sdk/js`（规划中）。 |

### L5：Harness 装配（把上面的拼起来变成可跑的 agent）

| crate | 职责 |
|------|------|
| `alva-app-core` | **预设 harness 层（"我们的 deer-flow"）**。核心：`BaseAgent`（`inner: Arc<Agent>` / `current_cancel` / `bus_writer`）+ `BaseAgentBuilder`（薄薄一层包装，`build()` 内部委托给 `alva_agent_core::AgentBuilder`，自动塞入默认 `MemoryPlugin` + `SecurityPlugin` 除非用户已注册同名 plugin）；兼容 `prompt` 保留 unbounded 流，原生产品走 `prompt_bounded`。`extension/` 子目录持有协议 + middleware plugins：`SkillsPlugin` / `McpPlugin` / `HooksPlugin` / `EvaluationPlugin` / `SubAgentPlugin`（folder-based）+ `PermissionPlugin`（注册 `PlanModeMiddleware` + 提供 `PlanModeControl` / `PermissionModeService`）/ `AnalyticsPlugin` / `LspPlugin` / `ApprovalPlugin` / `PendingPlugin` / `BlackboardCommPlugin` / `ProviderRegistryPlugin` / `SpawnCommRegistryPlugin` / `ToolLockRegistryPlugin`（flat）。纯中间件壳（旧 `LoopDetection` / `DanglingToolCall` / `ToolTimeout` / `Compaction` / `Checkpoint` 包装）已删，单点 middleware 直接 `.middleware()` 注册。`PermissionModeService` + `PermissionMode` 现位于 bus 上，由 `PermissionPlugin::register()` 注册。`base_agent/` 模块。 |
| `alva-host-native` | Native 平台能力层。主职责是 `init::model("provider/id")` 初始化 `LanguageModel`、`TokioSleeper`、native-only middleware（`CheckpointMiddleware` 以及从 box 转发的 `SecurityMiddleware` / `PlanModeMiddleware` / `CompactionMiddleware`）和 graph re-export。`AgentRuntimeBuilder` / `with_standard_agent_stack(SandboxMode)` 仍保留但已 deprecated，只作为 legacy/parallel 路径维护；新 app 入口走 `BaseAgentBuilder`，SDK 入口走 `alva-agent-core::AgentBuilder`。 |
| `alva-host-wasm` | wasm32 装配。`WasmAgent` facade + `WasmSleeper`（spawn_local + oneshot 桥接 non-Send `gloo_timers` future）+ `smoke::_wasm_smoke_probe` 编译期探针，强制 `cargo check --target wasm32-unknown-unknown` 穿透整条 kernel API 表面。**共享同一份 `alva-kernel-core`，kernel 一行不用动**。 |
| `alva-sandbox-wasm` | Native-only WASIp1 模块宿主 runner。每次调用新建 wasmtime `Engine` / `Store` / `WasiCtx`，把 job 授权目录作为 preopen 映射进 guest，并捕获退出码与 stdout/stderr；提供 L0 DTO 驱动的 ptr/len LLM / host escalation import 桥、guest cwd→`RunRequest.grants` host path 安全翻译、宿主强制域名白名单/逐跳重定向校验的 fetch import，以及不持有落盘策略的有界 audit event import。 |
| `alva-worker-wasm` | WASIp1 agent worker：用 SDK `AgentBuilder` + WASI 文件工具在 guest 内运行真 agent loop，经版本化、限额化 ptr/len import 接收宿主解析的 `wasm-env`、代理宿主 LLM stream、同步 fetch、`request_escalation` 与单向 audit event；workspace、任务、授权 guest path 与结果通道全部由 WASI args 注入，environment 正文不进 argv，审批模式/宿主路径/job 日志路径不进入 guest。 |

### L4.5：引擎契约与 SDK bridge

> 引擎 contract 是轻量 SDK；若存在 SDK adapter，只能保持 SDK-clean。原生 `alva-app-cli` / `alva-app-tauri` 不接 `EngineRuntime`，继续是直接消费 `alva-app-core` 的 Alva 产品入口。

| crate | 职责 |
|------|------|
| `alva-engine-runtime` | `EngineRuntime` trait：`execute` / `cancel` / `respond_permission` / `capabilities`。让 app 层用同一套接口驱动多种 agent 后端。 |
| `alva-engine-adapter-claude` | Claude Code 双模式适配器：Full = Node.js Agent SDK bridge + JSON-line；Light = `claude -p --output-format stream-json`。Light 默认 `--safe-mode` + fail-closed `dontAsk`，bypass 仅显式 opt-in。 |
| `alva-engine-adapter-codex` | Codex CLI 适配器：`codex exec --json` 子进程 + JSONL 事件映射。 |
| `alva-engine-adapter-pi` | Pi CLI 适配器：JSON print mode + owner-only 临时 provider 配置；第三方凭证只经子进程环境传递。 |

### L5：协议与基础设施

| crate | 职责 |
|------|------|
| `alva-protocol-skill` | Skill 系统：加载 / 注入 / 存储 / 渐进式三级加载。 |
| `alva-protocol-mcp` | Model Context Protocol 客户端 + `McpToolAdapter`（把远端 MCP 工具桥接成 `Tool` trait）。 |
| `alva-protocol-acp` | Agent Client Protocol：消息类型 / 会话 / 连接 / 进程管理。 |
| `alva-llm-provider` | LLM provider 实现：`AnthropicProvider` / `OpenAIChatProvider` / `OpenAIResponsesProvider` / `GeminiProvider`；**canonical 工厂 `build_language_model(kind, config)` 与默认端点表 `default_base_url(kind)`（PR-10 之后调用方一律走这两个入口，禁止再复制 match）**。 |
| `alva-environment` | 运行环境检测与抽象。 |
| `alva-macros` | 过程宏：`#[derive(Tool)]` 等。 |
| `alva-test` | 测试辅助：`MockLanguageModel` 等。 |

### L6：应用层（最终用户能跑起来的产品）

| crate | 职责 |
|------|------|
| `alva-app-cli` | CLI 入口（`pi` 风格的交互式终端 + REPL）；`-p --sandbox wasm --grant <dir>` 生产路径在宿主保留 provider/key/PermissionMode，解析 bundled `wasm-env` 后经有界 context ABI 下发，用 sidecar worker 跑 WASIp1 agent loop，并在 host escalation 时审批、翻译 cwd、执行；jobs 由宿主以 native middleware / wasm audit import 及 `escalation_request`/`escalation_result` 实时追加统一 `tools.jsonl`。 |
| `alva-app-debug` | AI 调试系统：HTTP API / 日志捕获 / 视图树检查 / `traced!` 宏。 |
| `alva-app-devtools-mcp` | MCP 服务器：包装 `alva-app-debug` HTTP API，供外部 IDE 调试。 |
| `alva-app-tauri` | Tauri 2 桌面壳 + React/Vite 前端：新一代 GUI，内置 Inspector、持久化 session（SQLite）、多窗口。 |
| `alva-app-gateway` | 内嵌 HTTP gateway（OpenAI 兼容对外面）。**被 `alva-app-tauri::start_gateway` 承重**——曾被误判为死代码，勿删。 |

### L7：Hub 产品聚合层（位于原生 app 之上）

> Alva Hub 不改写原生 CLI/Tauri 的入口。它把多 engine session、会话路由和跨 engine 编排提到产品层；Hub 内的 Alva adapter 依赖 `alva-app-core`，因此必须位于 SDK 与原生 app 之上。

```text
current: alva-hub-cli + alva-hub-desktop (ticket-03 durable attempts + crash recovery)
target:  complete Desktop V1 (fallback + branches + Orca collaboration)
                  │
             alva-hub-core
                  │
                 alva-engine-runtime
       ┌──────────┬──────────┬──────────┐
       │          │          │          │
   adapter-alva  adapter-claude  adapter-codex  adapter-pi
       │          │          │          │
  alva-app-core  Claude CLI  codex exec  pi JSON mode
```

| crate | 职责 |
|------|------|
| `alva-hub-core` | Hub 产品会话监管器：engine registry、product session ↔ engine session 路由、resume/cancel/permission dispatch；仅依赖 engine contract。 |
| `alva-engine-adapter-alva` | Hub 的 Alva adapter：独立完整 `BaseAgent`；共享 `SessionRepository` records/Inspector，opt-in `ContextRetrievalPlugin` 在压缩前投影大结果/媒体引用并提供自有 native 历史检索；app-core SpawnContextPolicy 将其重绑到递归子会话，首个 assembly hook + middleware fail-closed，不授予父历史或 root host 工具。受管 MCP 单 gateway，duration 保留；不控制外部 engine，不冒充跨 engine 历史。 |
| `alva-hub-cli` | Hub 命令行验证、诊断与自动化入口：支持 one-shot / REPL、私有 session journal、显式 Alva / Claude / Codex 选择、resume、手动切换与 handoff；与原生 `alva-app-cli` 保持独立。 |
| `alva-hub-desktop` | **正在实现的 V1 主产品**：独立 Tauri 2 + React/Vite/TypeScript L7 入口；新会话可选 owner-only 专属目录的“独立会话”或用户选择 Workspace 的“项目”；支持 ALVA/Claude Code/Codex/Pi、统一第三方 Provider route、CLI 私有订阅、可逆多 EngineBranch、完整 HandoffPackage、自动 fallback 与 Cindy 风格 Lead/Worker，并用私有 SQLite 持久化产品状态；稳定 TimelineEvent 使用产品 shared clock，UI 提供 Input/Agent/Tools 三泳道概览；Workbench Inspector 经稳定 IPC 消费 Alva typed records，支持 LLM/Tool/Middleware 与递归子 Agent 钻取、逐页回退；不得改写 `alva-app-tauri`。 |
| `alva-hub-server` | Hub Desktop 同源的本地 headless HTTP/SSE 入口；复用 HubStore、engine assembly 与 Inspector DTO，让浏览器版得到与 Tauri 一致的 session、timeline 和 durable inspection surface。 |

---

## 两种入口

```rust
// 入口 A：SDK 级（用户搭自己的 harness）
use alva_agent_core::{Agent, AgentBuilder};
use alva_agent_extension_builtin::wrappers::{CorePlugin, ShellPlugin};

let agent = Agent::builder()
    .model(my_model)
    .plugin(Box::new(CorePlugin))
    .plugin(Box::new(MyCustomPlugin))
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
    // 默认自带 MemoryPlugin (InMemoryBackend) + SecurityPlugin + SystemContextPlugin
    // 想换 memory？.plugin(Box::new(MyMemoryPlugin))，name() 撞车自动跳过默认
    .build(my_model)
    .await?;
```

两条路最终都跑在 `alva-kernel-core::run_agent` 上。

---

## 工具与约束

| 项 | 职责 |
|------|------|
| `scripts/ci-check-deps.sh` | CI 强制 crate 边界规则。Rule 17 = SDK 不得依赖 app/host。wasm-clean crate 集合必须通过 `wasm32-unknown-unknown`；native host 由 `alva-host-native` 承担，不要求 wasm-clean。 |
| `scripts/public-surface.sh` | **公开符号快照闸**：`alva-kernel-abi`、`alva-agent-abi` 与 `alva-agent-core` 的公开符号快照存于 `scripts/public-surface/`，CI 比对；对这两层公开表面的任何增删改都必须在同一个 commit 里显式更新快照。同时执行 `docs/REGISTRAR-RULES.md` 的 Registrar 准入规则（只增贡献方法，不增读取方法）。扫描器自身的回归 fixtures 在 `scripts/public-surface-regressions.py`（R70：属性项、变体 payload、关联类型 bounds、`->` 分割、堆叠属性均须可检测）。 |
| `docs/BUS-RULES.md` | bus 防退化规则（防止退化为 God Object）。 |
| `docs/ARCHITECTURE.md` | 三仓库架构设计：alva-sandbox + alva-agent + alva-app。 |
| `docs/ALVA-HUB-V1.md` | 已接受、正在增量实现的 Hub Desktop V1 产品、fallback、Orca 协作与验收规范。 |
| `docs/ALVA-HUB-GLOSSARY.md` | HubSession / EngineBranch / Turn / Attempt / TeamRun / Worker 等领域术语规范。 |
| `docs/ALVA-HUB-UI-GLOSSARY.md` | Hub Tauri UI 的 Token / Control / Region / Surface / Domain / Page 与验收词汇。 |
| `docs/ALVA-HUB-UI-WIREFRAMES.md` | Cindy 参考图转七张统一总层架的区域、滚动、停靠与 Overlay 规范。 |
| `docs/ALVA-HUB-UI-MOTION.md` | Hub 组件动效频率、目的、时长、曲线、实现与减弱动效规范。 |
| `docs/ALVA-HUB-MESSAGE-MODULES.md` | 可信 Hub 消息 renderer 注册、版本化纯数据模块与原文 fallback。 |
| `docs/ALVA-HUB-CONTEXT-RETRIEVAL.md` | ADR-0011 引用优先：所有工具/文件/MCP/历史按需检索与媒体处理来源说明；实现状态分开记录。 |
| `docs/adr/` | Hub Desktop-first、可逆多引擎 fallback、Cindy Orca 风格协作的架构决策记录。 |
| `Cargo.toml` | Rust workspace，管理 44 个 crate。 |
| `alva-bus-lint` | CI lint binary：扫 `#[bus_cap]` trait 的跨 crate 类型表面，配合 `docs/BUS-RULES.md` 防止 bus 退化为 God Object。 |
| `sdk/python/` | Python 插件 SDK（`alva_sdk` 包）。供第三方用 Python 写 `alva-app-extension-loader` 的子进程插件，不在 Cargo workspace 里。 |

---

# 项目架构

> 详细架构设计见 [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)

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

## Hub 产品 UI（alva-hub-desktop）

Hub 上下文贯彻引用优先（ADR-0011）：文件/媒体、所有大工具结果、MCP 目录和历史应先持久化，模型只拿小索引/引用，再按任务有界检索。具体当前实现/未完成项见 `docs/ALVA-HUB-CONTEXT-RETRIEVAL.md`；不得把 UI 分页当作模型端控制。上传当前保存原字节、20 MiB/文件和 40 MiB/8 文件每轮，明确原始哈希/处理/投递范围；无压缩/抽帧就明确注明未做，不冒充媒体理解。政策在产品/Plugin/Middleware 实施，不向 kernel/agent-core 增加功能。

外观偏好与工作区 Skills 文件管理属于 Hub 产品层：主题/密度/减弱动效保存在 WebView，
技能目录绑定已保存 session cwd，复用协议 parser，仅受管项目文件可写与可恢复归档，
不扫描全局来源、不下载或执行脚本，也不修改 kernel/agent-core 或运行中的 Agent。
本地自动化也由 Hub 产品层持久化和调度：默认暂停，显式确认账户/计费后按间隔恢复
普通会话，固定路由且禁用自动 fallback；不安装 OS daemon，不补跑漏过的任务。

ALVA 能力/MCP 配置同样归 Hub 私有产品层：真实 catalog 开关不包括安全 substrate，
仅下一轮 ALVA 消费，不影响其他 engine、不加载外部现有 MCP/AEP。受管 MCP 当前仅
原生 stdio，不安装程序或继承凭据；新增/编辑禁用，启用/测试需确认宿主访问，测试
绑定已存 session cwd 并结束回收；MCP 工具含 meta-tool 进入既有 HITL。

Hub 产品 UI 以 `crates/alva-hub-desktop` 的 Tauri + React 实现为准，旧 GPUI 产品 UI
迁移方向已退役。正式架构决策见 `docs/adr/0006-hub-tauri-ui-system.md`：Cindy 负责结构与
交互参考，ALVA 负责视觉语言；组件严格按 Tokens → Controls → Regions/Surfaces → Domain
Components → Pages 五层依赖。动任何 Hub UI 代码前先读
`crates/alva-hub-desktop/web/src/AGENTS.md`、`docs/ALVA-HUB-UI-GLOSSARY.md`、
`docs/ALVA-HUB-UI-WIREFRAMES.md` 与 `docs/ALVA-HUB-UI-MOTION.md`。

`docs/GPUI-MIGRATION.md` 和 `crates/alva-hub-gpui/AGENTS.md` 只记录旧实验路线，不再是 Hub
产品 UI 的实现权威。
