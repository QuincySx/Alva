# alva-kernel-core
> Session-centric Agent engine crate: run loop, middleware, context runtime, tool batch coordination, and runtime execution context.

## 地位
`alva-kernel-core` 是 agent 体系的内核执行层。它不关心具体 provider、tool 实现或 UI，只负责驱动单 agent 的运行循环，并通过 middleware、bus hook 与上层运行时协作。

## 逻辑
- `run.rs` 提供 `run_agent()`，以 session 为单一消息真相源执行双层循环：inner loop 负责 LLM 调用与 tool batch execution，outer loop 负责 run continuation。
- `state.rs` 将可变运行态 `AgentState` 与不可变配置 `AgentConfig` 分离，降低借用冲突并把消息历史外置到 `session`。
- `middleware.rs` 和 `shared.rs` 提供 async middleware stack、priority、error 和 type-safe `Extensions`；`input_committed` 在输入消息写入 session 后触发，是 `Phase::InputCommitted` 的真实执行点。
- `context_runtime.rs` 提供 `ContextRuntime`，集中处理 ContextHooks 的 pending injection、assemble、budget compression、after_turn 与 dispose。
- `tool_batch.rs` 提供 `ToolBatchCoordinator`，集中处理一批模型声明的 tool call：发 `tool_use` skeleton、执行 before/wrap/after middleware、写 `tool_result`，并保证 session commit 顺序稳定。
- `runtime_context.rs` 为 tool 执行构造 `RuntimeExecutionContext`，把 progress、workspace、bus 等能力桥接给 tool。
- `builtins/` 放 kernel 级默认 middleware，如 loop detection、dangling tool call、tool timeout。

## 约束
- 仅依赖 `alva-kernel-abi` trait 与基础类型，不直接绑定具体 engine/provider。
- 消息历史不存放在 `AgentState`，统一存放在 `session`。
- middleware 为 async；tool batch 当前保持顺序执行，但 coordinator 是后续并发调度的唯一入口。
- crate 暴露的是执行内核，不负责 batteries-included 组装；标准 stack 由 `alva-host-native` 完成。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 源码实现 | `src/` | 内核执行循环、状态、middleware、context runtime、tool batch、runtime context |
| Builtins 示例 | `examples/` | 演示内核层能力与基础集成方式 |
| 集成测试 | `tests/` | 覆盖 run loop、context runtime、tool batch、tool execution 等行为 |
| Crate 配置 | `Cargo.toml` | 依赖声明：alva-kernel-abi、tokio、async-trait、serde、tracing 等 |
