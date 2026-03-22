# agent-core
> Agent 执行引擎，实现双层循环（inner loop + outer loop）驱动 LLM 与工具交互

## 地位
agent 体系的运行时引擎。接收 agent-base 定义的抽象类型，通过 AgentConfig 的 6 个 hook 函数实现可定制的 agent 执行流程。被 agent-graph 编排层调用，也可独立使用。

## 逻辑
```
Agent::run(user_message)
  └─→ AgentLoop (outer loop)
        ├─→ inner loop: LLM.generate() → 解析 tool_use → ToolExecutor 执行
        │     ├─→ before_tool_call hook → 决策 Allow/Block
        │     ├─→ Tool::execute() (parallel 或 sequential)
        │     ├─→ after_tool_call hook → 后处理 ToolResult
        │     └─→ get_steering_messages hook → 注入引导消息
        └─→ get_follow_up_messages hook → 注入后续消息 → 决定是否继续 outer loop
```
- `AgentConfig` 定义 6 个 hook：convert_to_llm（必选）、transform_context、before/after_tool_call、get_steering/follow_up_messages
- `ToolExecutionMode` 支持 Parallel 和 Sequential 两种工具执行模式
- `AgentEvent` 提供事件流供外部观察执行过程
- `max_iterations` 防护无限循环

## 约束
- 不直接依赖具体 LLM 或 Tool 实现，仅依赖 agent-base trait
- `convert_to_llm` 是唯一必选 hook，其余均可选
- 所有 hook 函数签名为 `Arc<dyn Fn(...) + Send + Sync>`

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| Agent 入口 | `agent.rs` | Agent 结构体，对外暴露 run/run_with_cancel 方法 |
| 执行循环 | `agent_loop.rs` | AgentLoop 双层循环实现 |
| 工具执行器 | `tool_executor.rs` | 并行/串行执行 ToolCall，调用 before/after hook |
| 类型定义 | `types.rs` | AgentConfig、AgentState、AgentContext、AgentMessage、ToolCallDecision |
| 事件 | `event.rs` | AgentEvent 枚举（流式输出、工具调用、完成等事件） |
