# alva-agent-core
> Session-centric Agent engine crate: run loop, middleware, runtime context, and pending message injection.

## 地位
`alva-agent-core` 是 agent 体系的内核执行层。它不关心具体 provider、tool 实现或 UI，只负责驱动单 agent 的运行循环，并通过 middleware、bus hook 与上层运行时协作。

## 逻辑
- `run.rs` 提供 `run_agent()`，以 session 为单一消息真相源执行双层循环：inner loop 负责 LLM 调用与 tool execution，outer loop 负责 follow-up continuation。
- `state.rs` 将可变运行态 `AgentState` 与不可变配置 `AgentConfig` 分离，降低借用冲突并把消息历史外置到 `session`。
- `middleware.rs` 和 `shared.rs` 提供 async middleware stack、priority、error 和 type-safe `Extensions`。
- `runtime_context.rs` 为 tool 执行构造 `RuntimeExecutionContext`，把 progress、workspace、bus 等能力桥接给 tool。
- `pending_queue.rs` 通过 `AgentLoopHook` / `PendingMessageQueue` 支持 steering 与 follow-up 注入。
- `builtins/` 放 kernel 级默认 middleware，如 loop detection、dangling tool call、tool timeout。

## 约束
- 仅依赖 `alva-types` trait 与基础类型，不直接绑定具体 engine/provider。
- 消息历史不存放在 `AgentState`，统一存放在 `session`。
- middleware 为 async；loop hook 为同步接口，供 runtime/UI 在循环检查点注入消息。
- crate 暴露的是执行内核，不负责 batteries-included 组装；标准 stack 由 `alva-agent-runtime` 完成。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 源码实现 | `src/` | 内核执行循环、状态、middleware、runtime context、pending queue，详见 `src/AGENTS.md` |
| Builtins 示例 | `examples/` | 演示内核层能力与基础集成方式 |
| 集成测试 | `tests/` | 覆盖 V2 run loop、tool execution、消息注入等行为 |
| Crate 配置 | `Cargo.toml` | 依赖声明：alva-types、tokio、async-trait、serde、tracing 等 |
