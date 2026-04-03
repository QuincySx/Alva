# alva-agent-graph/src
> 图编排源码：StateGraph builder、Pregel executor、checkpoint/retry/compaction helper 与 context transform。

## 地位
此目录是 `alva-agent-graph` 的全部实现。它提供可编译的状态图、可执行的 Pregel-style graph runtime，以及若干与 graph orchestration 相关的辅助类型。

## 逻辑
1. `graph.rs` 定义 `StateGraph`、`START` / `END`、`NodeResult`、`SendTo`，负责图结构构建与 compile-time 校验。
2. `pregel.rs` 定义 `CompiledGraph`、`GraphEvent`、`InvokeConfig`，负责 superstep 执行、dynamic routing、checkpoint 与 event emission。
3. `channel.rs` 定义 `LastValue` / `BinaryOperatorAggregate` / `EphemeralValue` 三类状态聚合语义。
4. `checkpoint.rs` 和 `retry.rs` 提供 checkpoint saver、retry config 等编排辅助能力。
5. `compaction.rs` 提供消息压缩相关 utility：token estimation、是否需要压缩、简单 compact helper。
6. `context_transform.rs` 提供 `ContextTransform` 与 `TransformPipeline`，用于图执行前后的上下文改写。
7. `session.rs` 提供 `AgentSession` 高层包装器，用于把 compiled graph 与 retry/compaction/checkpoint 配置组合在一起。

## 约束
- graph 执行主入口是 `CompiledGraph::invoke()` / `invoke_with_config()`。
- `session.rs` 当前只持有 orchestration config，不描述为独立 executor。
- `compaction.rs` 当前暴露的是 helper function；如果未来真正接入 graph session 执行链路，需要同步更新本页说明。
- 模块清单必须与真实文件一致，不再引用不存在的 `sub_agent.rs`。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 模块入口 | `lib.rs` | 声明子模块并 re-export public API |
| Graph Builder | `graph.rs` | `StateGraph`、`START`、`END`、`NodeResult`、`SendTo` |
| Pregel Executor | `pregel.rs` | `CompiledGraph`、`GraphEvent`、`InvokeConfig` |
| State Channel | `channel.rs` | channel 聚合语义类型 |
| Checkpoint | `checkpoint.rs` | `CheckpointSaver`、`InMemoryCheckpointSaver` |
| Retry | `retry.rs` | `RetryConfig` |
| Compaction Utility | `compaction.rs` | `CompactionConfig`、`estimate_tokens()`、`should_compact()`、`compact_messages()` |
| Context Transform | `context_transform.rs` | `ContextTransform`、`TransformPipeline` |
| Session Wrapper | `session.rs` | `AgentSession` 配置包装器 |
