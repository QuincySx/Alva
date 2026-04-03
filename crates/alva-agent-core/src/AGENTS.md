# alva-agent-core/src
> Agent 内核源码：session-centric run loop、middleware orchestration、runtime context 与 pending message injection。

## 地位
此目录是 `alva-agent-core` 的全部核心实现。它直接消费 `alva-types` 提供的 model/tool/session 抽象，向上提供 `run_agent()`、`AgentState`、`AgentConfig`、`AgentEvent`、`MiddlewareStack` 等核心 API。

## 逻辑
1. `run.rs` 实现主循环。每轮从 `session` 读取上下文，经过 middleware，调用 model，解析 `ToolUse`，执行 tool，再把结果写回 `session`。
2. `run_child.rs` 提供 child-agent 执行辅助逻辑，用于在受限上下文中运行子 agent 并回收结构化输出。
3. `state.rs` 定义 `AgentState` 与 `AgentConfig`：state 放运行态能力，config 放 middleware、system prompt、workspace、bus 等稳定配置。
4. `middleware.rs` 和 `shared.rs` 组成 onion-style middleware 子系统，负责 hook 分发、优先级、错误传播与扩展存储。
5. `runtime_context.rs` 为 tool 提供统一 `ToolExecutionContext` 实现，把 cancellation、workspace、progress event、bus 能力桥接到工具层。
6. `pending_queue.rs` 定义 `AgentLoopHook` 与 `PendingMessageQueue`，供外部在 tool 执行后插入 steering/follow-up。
7. `builtins/` 放内核默认 middleware：`DanglingToolCallMiddleware`、`LoopDetectionMiddleware`、`ToolTimeoutMiddleware`。

## 约束
- 消息历史只存在于 `session`，避免 `state` 与 `session` 双写。
- `run.rs` 只把 `AgentMessage::Standard` 送进 LLM；`Steering` / `FollowUp` 在进入 session 前会标准化。
- runtime-specific 组装逻辑不应回流到 core；需要 bus、security、checkpoint 等能力时，通过 `AgentConfig.bus` 和 middleware 协作。
- `src/` 中的模块图必须与真实文件一致，不保留已删除的 `agent.rs` / `agent_loop.rs` / `tool_executor.rs` 等旧名词。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate 入口 | `lib.rs` | 声明子模块并 re-export public API |
| 主运行循环 | `run.rs` | `run_agent()`，session-centric 双层循环与 tool execution 主链路 |
| Child Agent | `run_child.rs` | 子 agent 执行辅助与输出收敛 |
| 状态与配置 | `state.rs` | `AgentState`、`AgentConfig` |
| 事件定义 | `event.rs` | `AgentEvent` 可观测事件流 |
| Middleware 系统 | `middleware.rs` | `Middleware`、`MiddlewareStack`、`LlmCallFn`、`ToolCallFn` |
| 共享基础 | `shared.rs` | `Extensions`、`MiddlewareError`、`MiddlewarePriority` |
| Tool Context | `runtime_context.rs` | `RuntimeExecutionContext` |
| 注入队列 | `pending_queue.rs` | `AgentLoopHook`、`PendingMessageQueue` |
| Builtin Middleware | `builtins/` | loop detection、dangling tool call、tool timeout |
