# Round 8 review
> 夜间修复队列与扩展复审证据。

## 地位
本地 scratch 任务交接材料，不是产品实现。
## 逻辑
README 说明结论；TASKS 合并旧卡与新增卡；HANDOFF 是给实现 agent 的入口。
## 约束
新旧卡不重复计数，静态证据与实际运行严格分开；不得修改他人 dirty work。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 夜间入口 | README.md / TASKS.md / HANDOFF.md | 队列、依赖与执行约束 |
| 专项 | UI.md | 异步导航/刷新反例 |
| 任务 | issues/ | 新增有限修复卡 |
| 证据 | evidence/ | 独立 fixture、源码 witness 和真实结果 |
