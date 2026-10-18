# Round 6 修复卡

基线 `5290e033585bd44c6c019c2a47661043e05009b7`。15 张卡，全部 open。生产代码未由 reviewer 修改。

| 卡 | 优先级 | 轴 | 任务 |
|---|---|---|---|
| [R62](issues/R62.md) | P1 | Spec | 会话广播重入必须限定到正确的会话和异步执行上下文 |
| [R63](issues/R63.md) | P1 | Spec | 旧取消结果不得中止新一轮 |
| [R64](issues/R64.md) | P1 | Spec | 迟到的启动登记不得覆盖当前运行句柄 |
| [R65](issues/R65.md) | P1 | Spec | HTTP Stop 以真实流结束结算并隔离取消代际 |
| [R66](issues/R66.md) | P2 | Spec | 同步启动失败也必须回收已发布的 ActiveHubs |
| [R67](issues/R67.md) | P2 | Spec | Inspector 定位展开完整祖先路径并恢复执行概览 |
| [R68](issues/R68.md) | P2 | Spec | 工具执行状态不能继续写成文件变更 |
| [R69](issues/R69.md) | P2 | Spec | 费用完整度必须计入整个 usage 缺失的 attempt |
| [R70](issues/R70.md) | P2 | Standards | 公开符号扫描保留属性后的项目及完整签名 |
| [R71](issues/R71.md) | P2 | Spec | swap 与运行中 child 验收加入真实等待和下一轮 |
| [R72](issues/R72.md) | P2 | Spec | 工具锁矩阵必须制造两个真实 child 的执行重叠 |
| [R73](issues/R73.md) | P2 | Spec | 压缩换会话测试检查真正运行过的 A 会话 |
| [R74](issues/R74.md) | P2 | Spec | 完成引擎合规矩阵并如实保留未实现项 |
| [R75](issues/R75.md) | P3 | Standards | 补齐新增测试导航与最后一处聊天间距 token |
| [R76](issues/R76.md) | P2 | Spec | 中文审批保留未知工具的实际效果说明 |

执行顺序：先 R76、R62、R63→R64、R65→R66；UI 的 R67→R68→R69 同文件串行。R70 与 R71/R72/R73 可分责任文件并行。R74 独立推进；R75 最后同步。所有人共享仓库，不得回退他人修改。

旧 [R45](../kernel-boundary-review-round5/issues/R45.md) 继续 open，不另复制卡：真实权限事件、浏览器滚动/长代码、双密度、Tab/焦点/overlay/handoff 验收必须完成。R52 的剩余工作由 R74 追踪，其余原卡按 REVIEW 映射验收。
