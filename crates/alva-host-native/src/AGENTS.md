# alva-host-native/src
> Native 平台能力层：model init / sleeper / native middleware / graph re-export。`AgentRuntimeBuilder` 是 deprecated legacy runtime path。

## 地位
`alva-host-native` crate 的全部源码。当前定位是 native 平台能力层，而不是推荐的 agent harness 入口。新 app 入口走 `alva-app-core::BaseAgentBuilder`，SDK 入口走 `alva-agent-core::AgentBuilder`。本 crate 的 `AgentRuntimeBuilder` 保留为 deprecated legacy/parallel runtime path，主要用于低层 native runtime 实验和兼容测试。

## 逻辑
1. `builder.rs` 提供 deprecated `AgentRuntimeBuilder`，通过 Builder 模式配置 system prompt、tools、middleware、bus 与运行时选项，并在 `build(model)` 时接收 `LanguageModel`，返回 `Result<AgentRuntime, AgentError>`（包含 AgentState / AgentConfig / ToolRegistry / bus 相关 handles）；未注入外部 bus 时会自动创建一个默认 bus 并写入 `AgentConfig.bus`。
2. `AgentRuntimeBuilder::with_standard_agent_stack()` 是 legacy 一键装配路径：heuristic token counter、Security / LoopDetection / ToolTimeout / Compaction / PlanMode / Checkpoint middleware，以及可选 approval notifier 和 bus plugins。新 harness 不应继续扩展这条路径。
3. `init.rs` 提供 `model()` 函数，解析 `"provider/model_id"` 格式字符串并通过 `ProviderRegistry` 解析为 `LanguageModel`。
4. `graph.rs` re-export `alva-agent-graph` 的图编排类型（StateGraph / CompiledGraph / GraphRun / CheckpointSaver 等），使运行时用户无需直接依赖 graph crate。
5. `middleware/` 子目录提供领域特定的中间件实现（如 SecurityMiddleware），这些中间件依赖领域 crate 所以不放在 core 中。

## 约束
- `AgentRuntimeBuilder` 已 deprecated；只维护兼容和低层 native runtime 测试，不作为新功能入口。
- `AgentRuntimeBuilder` 不持有 model；调用方需在 `build(model)` 时传入已解析好的 `LanguageModel`。
- `AgentRuntimeBuilder::with_bus()` 可复用外部 bus；未显式设置时，builder 仍保证运行时拥有可用的 `BusHandle`。
- `AgentRuntimeBuilder::with_standard_agent_stack()` 要求已设置 workspace，并且需要可写 bus（默认内部 bus 或 `with_bus_writer()`）；配置错误通过 `AgentError::ConfigError` 返回，不得 panic。
- `graph.rs` 是纯 re-export 模块，不包含自有逻辑，graph 行为由 `alva-agent-graph` crate 决定。
- `model()` 函数要求 spec 格式为 `"provider/model_id"`，不符合格式将返回 `ProviderError`。
- native feature 下会额外注册 internet_search / read_url 工具和 MemoryService。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | lib.rs | 组合所有 Agent 子系统并 re-export 电池齐全的公共 API |
| AgentRuntimeBuilder | builder.rs | Deprecated legacy builder：配置 model、tools、middleware、hooks、bus 与标准 agent stack；新代码不要从这里装配 agent |
| Graph Re-exports | graph.rs | Re-export alva-agent-graph 的图编排类型供运行时用户使用 |
| Model Init | init.rs | 解析 "provider/model_id" spec 字符串为 LanguageModel |
| Middleware | middleware/ | 领域特定中间件实现（依赖领域 crate，不适合放在 core） |
