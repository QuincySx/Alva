# alva-agent-runtime/src
> 电池齐全的 Agent 运行时源码实现层：组合 core + tools + security + memory 并提供 Builder API

## 地位
`alva-agent-runtime` crate 的全部源码。是面向使用者的最高层抽象，组合所有 Agent 子系统（alva-agent-core / alva-agent-tools / alva-agent-security / alva-agent-memory）为一个开箱即用的运行时。被 `alva-app` 或 CLI 入口直接使用。

## 逻辑
1. `builder.rs` 提供 `AgentRuntimeBuilder`，通过 Builder 模式配置 model、tools、middleware、hooks，最终构建 `AgentRuntime`（包含 Agent + ToolRegistry）。
2. `init.rs` 提供 `model()` 函数，解析 `"provider/model_id"` 格式字符串并通过 `ProviderRegistry` 解析为 `LanguageModel`。
3. `graph.rs` re-export `alva-agent-graph` 的图编排类型（StateGraph / CompiledGraph / AgentSession / CheckpointSaver 等），使运行时用户无需直接依赖 graph crate。
4. `middleware/` 子目录提供领域特定的中间件实现（如 SecurityMiddleware），这些中间件依赖领域 crate 所以不放在 core 中。

## 约束
- `AgentRuntimeBuilder` 必须在 `build()` 前设置 model，否则 panic。
- `graph.rs` 是纯 re-export 模块，不包含自有逻辑，graph 行为由 `alva-agent-graph` crate 决定。
- `model()` 函数要求 spec 格式为 `"provider/model_id"`，不符合格式将返回 `ProviderError`。
- native feature 下会额外注册 internet_search / read_url 工具和 MemoryService。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | lib.rs | 组合所有 Agent 子系统并 re-export 电池齐全的公共 API |
| AgentRuntimeBuilder | builder.rs | Builder 模式构建 AgentRuntime：配置 model、tools、middleware、hooks |
| Graph Re-exports | graph.rs | Re-export alva-agent-graph 的图编排类型供运行时用户使用 |
| Model Init | init.rs | 解析 "provider/model_id" spec 字符串为 LanguageModel |
| Middleware | middleware/ | 领域特定中间件实现（依赖领域 crate，不适合放在 core） |
