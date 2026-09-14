# alva-kernel-core
> Session-centric Agent engine crate: run loop, middleware, context runtime, tool batch coordination, and runtime execution context.

## 地位
`alva-kernel-core` 是 agent 体系的内核执行层。它不关心具体 provider、tool 实现或 UI，只负责驱动单 agent 的运行循环，并通过 middleware、bus hook 与上层运行时协作。

## 逻辑
- `run/` 模块提供 `run_agent()`（`mod.rs`）与可替换的 `AgentLoop` trait（`loop_impl.rs`，+ `DefaultAgentLoop` 默认实现），以 session 为单一消息真相源执行双层循环：inner loop 负责 LLM 调用与 tool batch execution，outer loop 负责 run continuation；`llm_call.rs` 收拢 schema 预烘焙、真实 LLM 调用的 `LlmCallFn` 适配器，以及不含正文的首输出/流摘要观测（每次响应至多两条 durable event，不按 token 写盘）。
- `state.rs` 将可变运行态 `AgentState` 与不可变配置 `AgentConfig` 分离，降低借用冲突并把消息历史外置到 `session`。
- `middleware.rs` 和 `shared.rs` 提供 async middleware stack、priority、error 和 type-safe `Extensions`；每层 hook 的 identity/order/inclusive duration/结果按 stack phase 打包为一条 `middleware_hook_batch` 进入 canonical session，避免逐 middleware SQLite 写放大；`input_committed` 在输入消息写入 session 后触发，是 `Phase::InputCommitted` 的真实执行点。
- `context_runtime.rs` 提供 `ContextRuntime`，集中处理 ContextHooks 的 pending injection、assemble、budget compression、after_turn 与 dispose。
- `event.rs` 提供 `AgentEventSender`：保留 unbounded SDK 兼容路径，bounded 路径让 lifecycle/message/tool terminal 事件对生产消费者施加真实背压；同步 progress 更新满载时 best-effort 丢弃。
- `run/mod.rs` 的 `llm_call_start` 记录版本化 cache-base/tool-schema 指纹，`llm_call_end` 把 provider usage 归类为 hit/write/miss/unreported；指纹只做通用审计，provider 专有 cache-control 仍归 provider adapter。
- `tool_batch.rs` 提供 `ToolBatchCoordinator`，集中处理一批模型声明的 tool call：显式 concurrency-safe 且资源无冲突的调用组成并行 wave；未知、冲突、全局串行和 coordinator 调用形成顺序屏障；intent/outcome 始终按模型顺序提交。
- （R46 更正：`context_apply` 与 `tool_lock` 已不在本 crate——`tool_lock` 在 `alva-agent-core`，context hook 返回值落位由 `context_runtime.rs` 协调。）
- `runtime_context.rs` 为 tool 执行构造 `RuntimeExecutionContext`，把 progress、workspace、bus 等能力桥接给 tool。
- `builtins/` 放 kernel 级默认 middleware，如 loop detection、dangling tool call、tool timeout。

## 约束
- 仅依赖 `alva-kernel-abi` trait 与基础类型，不直接绑定具体 engine/provider。
- 消息历史不存放在 `AgentState`，统一存放在 `session`。
- middleware 为 async；tool batch 只并行执行工具显式声明安全且资源无冲突的调用，所有未声明安全的工具保持顺序执行。
- 生产 harness 应使用 bounded 事件路径；unbounded 仅用于兼容旧 SDK、executor-neutral wasm 与受控内部消费。
- crate 暴露的是执行内核，不负责 batteries-included 组装；标准 stack 由 `alva-host-native` 完成。
- 缓存审计只记录逻辑前缀与 provider 已报告 usage，不在内核猜测或配置 Anthropic/OpenAI/Gemini 专有缓存策略。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 源码实现 | `src/` | 内核执行循环、状态、middleware、context runtime/apply、tool batch、runtime context |
| Builtins 示例 | `examples/` | 演示内核层能力与基础集成方式 |
| 集成测试 | `tests/` | 覆盖 run loop、context runtime、tool batch、tool execution 等行为 |
| Crate 配置 | `Cargo.toml` | 依赖声明：alva-kernel-abi、tokio、async-trait、serde、tracing 等 |
