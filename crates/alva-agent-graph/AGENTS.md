# alva-agent-graph
> Graph orchestration crate: StateGraph builder, Pregel executor, checkpoint/retry helpers, and standalone graph utilities.

## 地位
`alva-agent-graph` 位于单 agent 内核之上，提供多步骤状态图编排能力。它本身不依赖 `alva-agent-core` 的 run loop，而是通过独立的 graph builder 与 Pregel-style executor 驱动工作流。

## 逻辑
- `graph.rs` 负责图定义：节点、边、条件路由、dynamic `SendTo` fan-out、merge function。
- `pregel.rs` 负责执行：`CompiledGraph::invoke_with_config()` 支持 superstep 执行、checkpoint 和 event streaming。
- `checkpoint.rs` / `retry.rs` / `compaction.rs` 提供编排相关辅助类型与工具函数。
- `session.rs` 把 compiled graph 与 retry/compaction/checkpoint 配置打包成高层 `AgentSession` 包装器，但执行入口仍然是 `CompiledGraph`。
- `context_transform.rs` 提供上下文变换 pipeline，用于在 graph 边界上改写输入输出状态。

## 约束
- 编排入口是 `StateGraph::compile()` 产出的 `CompiledGraph`；`session.rs` 当前主要承担配置聚合，不替代 executor。
- `compaction.rs` 当前是 standalone utility，不应在文档中描述为已自动接入图执行主链路。
- 当前 crate 没有 `sub_agent.rs`，目录清单不得继续引用该文件。
- `START` / `END` 是保留节点名。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 源码实现 | `src/` | graph builder、Pregel executor、orchestration helper，详见 `src/AGENTS.md` |
| Crate 配置 | `Cargo.toml` | 依赖声明：alva-types、tokio、serde 等 |
