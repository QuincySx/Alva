# alva-kernel-core/src
> Agent 内核源码：session-centric run loop、middleware orchestration、runtime context 与 pending message injection。

## 地位
此目录是 `alva-kernel-core` 的全部核心实现。它直接消费 `alva-kernel-abi` 提供的 model/tool/session 抽象，向上提供 `run_agent()`（及可替换的 `AgentLoop`/`DefaultAgentLoop` driver 接口）、`AgentState`、`AgentConfig`、`AgentEvent`、`MiddlewareStack` 等核心 API。

## 逻辑
1. `run/` 实现主循环（`mod.rs`）+ LLM 调用准备与执行（`llm_call.rs`）+ 可替换接口（`loop_impl.rs`）。每轮从 `session` 读取上下文，经过 middleware，调用 model，解析 `ToolUse`，执行 tool，再把结果写回 `session`；LLM session 事件同时记录版本化 cache-base/tool-schema 指纹、provider usage outcome，以及每次响应至多一条 `llm_first_output` 与一条无正文的 `llm_stream_summary`，提供 TTFT/流耗时而不按 token 同步写盘。`mod.rs` 提供 `run_agent`；`loop_impl.rs` 提供 `AgentLoop` trait 与 `DefaultAgentLoop` 默认实现（`run_agent` 是其背后驱动）。
2. `run_child.rs` 提供 child-agent 执行辅助逻辑，用于在受限上下文中运行子 agent 并回收结构化输出。
3. `state.rs` 定义 `AgentState` 与 `AgentConfig`：state 放运行态能力，config 放 middleware、system prompt、workspace、bus 等稳定配置。`AgentState.session` 只是对话记录（`ConversationLog`，R05）——查询/回滚/快照/订阅/压缩由装配层显式提供，内核入口不强制。
4. `middleware.rs` 和 `shared.rs` 组成 onion-style middleware 子系统，负责 hook 分发、优先级、错误传播与扩展存储；每层 hook 的名称/顺序/失败/inclusive duration 打包进每阶段一条 `middleware_hook_batch` canonical event，观测粒度不变，SQLite 写入量不随 middleware 数量线性放大。
5. `runtime_context.rs` 为 tool 提供统一 `ToolExecutionContext` 实现，把 cancellation、workspace、progress event、bus 能力桥接到工具层。
6. `tool_batch.rs` 根据 Tool 的 concurrency-safe、ExecutionMode 与 ResourceKey 声明生成无冲突并行 wave，并按模型顺序提交 durable intent/outcome。
7. `tool_lock.rs` 已迁移至 `alva-agent-core`（R-tickets 08）：跨 Agent 资源锁是装配层机制，内核只保留调度词汇（`Scheduling`/`ResourceKey`）与波次调度；取锁经装配层 `ToolLockMiddleware` 完成。
9. `builtins/` 放内核默认 middleware：`DanglingToolCallMiddleware`、`LoopDetectionMiddleware`、`ToolTimeoutMiddleware`。

## 约束
- 消息历史只存在于 `session`，避免 `state` 与 `session` 双写。
- `run/` 只把 `AgentMessage::Standard` 送进 LLM；`Steering` / `FollowUp` 在进入 session 前会标准化。
- runtime-specific 组装逻辑不应回流到 core；需要 bus、security、checkpoint 等能力时，通过 `AgentConfig.bus` 和 middleware 协作。
- `src/` 中的模块图必须与真实文件一致，不保留已删除的 `agent.rs` / `agent_loop.rs` / `tool_executor.rs` 等旧名词。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate 入口 | `lib.rs` | 声明子模块并 re-export public API |
| 主运行循环 | `run/mod.rs` | `run_agent()`，session-centric 双层循环、cache-base 审计与 tool execution 主链路 |
| LLM 调用 | `run/llm_call.rs` | `bake_tool_schemas`、`ActualLlmCall`（schema 预烘焙 + 真实 LLM 调用的 LlmCallFn 适配器 + 紧凑首输出/流摘要） |
| 可替换接口 | `run/loop_impl.rs` | `AgentLoop` trait、`DefaultAgentLoop` 默认实现 |
| Child Agent | `run_child.rs` | 子 agent 执行辅助与输出收敛 |
| 状态与配置 | `state.rs` | `AgentState`、`AgentConfig` |
| 事件定义 | `event.rs` | `AgentEvent` 线格式、bounded/unbounded `AgentEventSender`、小型传输错误与默认生产容量 |
| Middleware 系统 | `middleware.rs` | `Middleware`、`MiddlewareStack`、`LlmCallFn`、`ToolCallFn` 与 packed durable hook-phase envelope |
| 共享基础 | `shared.rs` | `Extensions`、`MiddlewareError`、`MiddlewarePriority` |
| Tool Context | `runtime_context.rs` | `RuntimeExecutionContext` |
| Tool 批调度 | `tool_batch.rs` | 无冲突并行 wave、模型顺序 durable commit、取消/失败结果补齐 |
| 会话契约实现 | `agent_session.rs` 已迁出 | InMemory / Listenable 内存会话后端现居 `alva-agent-core/src/agent_session.rs`（R-ticket 09） |
| 上下文执行 | `context_apply.rs` 已迁出 | 注入/压缩执行逻辑现居 `alva-agent-context/src/context_apply.rs`（R-ticket 10）；内核经 `ContextApply` 执行器 trait 间接使用 |
| Builtin Middleware | `builtins/` | loop detection、dangling tool call、tool timeout |
