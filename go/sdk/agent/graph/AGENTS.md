# L3 · sdk/agent/graph
> 与标准 AgentLoop 平行的 typed 状态图能力库，不是 Plugin，也不依赖 harness、platform 或用户产品。

## 地位
SDK 能力；标准库加 context/wire，必须 wasm-clean。

## 逻辑
StateGraph 编译验证节点与边；每个 superstep 从 detached state 执行有界并行节点，再按确定顺序 merge。
静态并行 routing 看 merge 后状态；动态 Send routing 看各自输出。动态嵌套 fan-out 真实递归执行，受深度/任务/并发预算约束，
不沿袭旧实现丢弃嵌套 Send 的缺陷。Checkpoint 保存真实 state 与下一 frontier，Resume 是显式执行动作；保存失败不伪称成功。
Retry 必须显式提供 Retryable，取消不重放；所有已开始 node 等实际退出。调用方提供 clone，声明 state 的所有权复制语义。Checkpoint clone在状态锁外执行，回调后重查取消，取消不得提交新checkpoint。Channel clone/equal/reduce也在状态锁外执行，独立writer锁串行化更新，允许回调观测Version等只读元数据；equal/reduce还可调用Get，clone不得递归触发自身。不支持同channel递归更新，回调不得修改输入。TransformPipeline拒绝nil能力、回调后检查取消并复制输出，外部保留slice不能改后续结果。

## 约束
Checkpoint 默认进程内存，有界且不静默逐出；不自动持久化、不授权工具、不自动重新执行副作用。
节点本身须遵循 context。合并函数和 clone 须可安全并发，Channel 未提供 clone 时只适用于不可变值。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| Graph builder | `graph.go` | typed node/send/router、严格 compile |
| BSP runtime | `engine.go` | 并行 superstep、merge、routing、显式 resume |
| Checkpoint | `checkpoint.go` | typed clone 边界与有界 memory saver |
| Channels | `channel.go` | LastValue、EphemeralValue、reducer aggregate |
| Helpers | `helpers.go` | 显式 retry、context transform、compaction、GraphRun |
| Callback boundaries | `boundary_test.go` | 取消不提交/不成功、clone与channel回调可观测metadata、pipeline输出所有权 |
| Regressions | `graph_test.go` | routing、嵌套 sends、checkpoint、取消与 channel |
