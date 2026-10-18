# SDK extensibility probes
> 独立消费者的可执行扩展性实验，不是产品能力或新 runtime 公共接口。

## 地位
architecture 下的测试包；仅通过公开 SDK/原生存储接口构建小型实现，不被生产源码依赖。

## 逻辑
四种不同执行需求分别验证接入位置、既有语义复用范围和仍需新契约的限制。普通上下文与多 Agent 实验使用真实默认循环；双向实验明确替换 loop；持久实验通过独立测试子进程重新打开专属0700临时 SQLite目录。持久文件排除js/wasip1，其余三类可独立进行WASM测试编译。

## 约束
无联网、真实模型或音视频设备，所有数据只在 t.TempDir。双向帧只是确定性 transport fixture，不宣称真实音频或继承默认循环的工具/审计保障；Open前拒绝现有Middleware、工具、context和Run消息/prompt，单次请求模型接口仍是Builder的必需项。持久实验只恢复外围业务阶段，不恢复 Go 调用栈，不自动重放未知副作用，也不声称 exactly-once；动作串行、批准绑定、完成以实际成功outcome为据，模型文本不能修改账本。不得为让实验通过增加 SDK hook、改公开快照或放宽生产权限。

## 业务域清单
| 文件 | 职责 |
|---|---|
| `support_test.go` | 最小模型、工具、Plugin 与真实 Run 的私有测试装配；读工具显式可并发，写工具SerialGlobal |
| `context_test.go` | 无四层 Store/压缩动作的检索工作视图，完整工具协议与原始事实保留；Source失败零次全读，不声称成功路径有界恢复 |
| `multiagent_test.go` | 外围 BSP 调度独立 Agent，真实并行工具、会话隔离；持有取消后的真实工作验证先join再返回 |
| `duplex_test.go` | Plugin 注入双向传输、替换 loop，流中输入、背压和取消所有权 |
| `durable_test.go` | native-only SQLite审批阶段、跨进程恢复/真实effect后退出、重复恢复/版本拒绝；不合作模型的未批准/重复effect/状态重置/虚假完成反例 |
