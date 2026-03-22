# agent-graph
> 图编排引擎，基于 StateGraph + Pregel BSP 模型实现多 Agent 工作流

## 地位
agent 体系的编排层。在 agent-core 单 agent 循环之上，提供有向图定义、状态通道、检查点持久化、上下文压缩、子 agent 配置等能力，支撑多步骤/多 agent 工作流。

## 逻辑
```
StateGraph (builder)
  ├─→ add_node(name, handler)
  ├─→ add_edge(from, to) / add_conditional_edges
  ├─→ set_channel(key, Channel)
  └─→ compile() → CompiledGraph (Pregel engine)
        └─→ Pregel BSP loop:
              ├─→ 按拓扑序执行节点
              ├─→ 通过 Channel 聚合状态 (LastValue / BinaryOp / Ephemeral)
              ├─→ CheckpointSaver 存取快照
              ├─→ RetryConfig 控制重试策略
              └─→ CompactionConfig 压缩历史消息

AgentSession 管理运行时会话状态
TransformPipeline 串联多个 ContextTransform 处理上下文
SubAgentConfig 配置子 agent 的模型和工具集
```

## 约束
- StateGraph 编译后不可变，运行时状态通过 Channel 传递
- Channel 三种类型各有语义：LastValue（覆写）、BinaryOp（归约）、Ephemeral（每轮清空）
- CheckpointSaver 是 trait，默认提供 InMemoryCheckpointSaver
- START / END 是保留节点名，不可被用户定义的节点覆盖

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| 图构建器 | `graph.rs` | StateGraph builder，定义节点、边、条件边，编译为 CompiledGraph |
| BSP 引擎 | `pregel.rs` | CompiledGraph / Pregel 执行器，按超步执行图节点 |
| 状态通道 | `channel.rs` | Channel 枚举（LastValue / BinaryOp / Ephemeral） |
| 检查点 | `checkpoint.rs` | CheckpointSaver trait + InMemoryCheckpointSaver |
| 重试策略 | `retry.rs` | RetryConfig（最大重试次数、退避策略） |
| 上下文压缩 | `compaction.rs` | CompactionConfig、compact_messages、estimate_tokens、should_compact |
| 上下文变换 | `context_transform.rs` | ContextTransform trait + TransformPipeline |
| 会话管理 | `session.rs` | AgentSession 运行时状态 |
| 子 Agent | `sub_agent.rs` | SubAgentConfig、SubAgentModel、SubAgentTools |
