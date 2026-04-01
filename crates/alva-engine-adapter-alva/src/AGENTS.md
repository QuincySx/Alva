# alva-engine-adapter-alva
> 本地 Agent 引擎的 EngineRuntime 适配器，将 alva-agent-core 的 Agent 包装为统一运行时接口。

## 地位
作为 alva-engine-runtime 的具体实现之一，本模块将内置的 alva-agent-core Agent 引擎适配为统一的 `EngineRuntime` trait。上层消费者仅依赖 `EngineRuntime`，无需感知底层引擎细节。属于"多引擎架构"中的本地引擎适配层。

## 逻辑
1. `AlvaAdapter::new()` 接收 `AlvaAdapterConfig`（含 LLM model、工具集、执行模式等），构建适配器实例并初始化空的 session map。
2. `execute()` 每次调用生成唯一 session_id，读取 `RuntimeRequest` 中的 `system_prompt` / `working_directory` / `max_turns`，创建 `Agent` 实例并启动 agent loop，通过 `mpsc` channel 将 `AgentEvent` 流转发到输出端。
3. `EventMapper` 是有状态映射器，维护 session_id、turn_count、tool_names（tool_use_id -> name 查找表），将 `AgentEvent` 逐个转换为 `Vec<RuntimeEvent>`。
4. `cancel()` 通过 session map 查找并终止对应 Agent；`respond_permission()` 当前未实现（本地引擎不涉及外部权限回调）；`resume_session` 明确返回 `RuntimeError::Unsupported`。
5. 事件流终止语义：`Completed` 为唯一终端事件，错误时先发 `Error { recoverable: false }` 再发 `Completed { result: None }`。

## 约束
- `AlvaAdapterConfig` 中 `model` 和 `tools` 为必填项；`max_iterations` 是默认值，可被 `RuntimeRequest.options.max_turns` 覆盖。
- `EventMapper` 在 `MessageEnd` 时拆分 ToolUse/ToolResult 为独立的 `ToolStart`/`ToolEnd` 事件，`Message.content` 中不含工具块。
- 返回的 Stream 为 `'static`，不借用 `&self`。
- `working_directory` 会写入 `AgentConfig.workspace` 并透传给 tool context。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| AlvaAdapter | adapter.rs | EngineRuntime trait 实现，管理 session 生命周期、启动 agent loop 并输出事件流 |
| AlvaAdapterConfig | config.rs | 适配器配置：LLM model、工具集、工具上下文、执行模式、最大迭代数、流式开关 |
| EventMapper | mapping.rs | 有状态事件映射器，将 AgentEvent 转换为 RuntimeEvent，维护 turn 计数和工具名查找表 |
| lib.rs | lib.rs | 模块入口，重导出 AlvaAdapter 和 AlvaAdapterConfig |
