# 夜间执行入口

工作区：`/Volumes/DevVault/Dev/Projects/QuincyWork/alva-agent`

阅读 [TASKS.md](TASKS.md) 后，按依赖完成队列里的修复与验收。用户已要求继续修，不必逐卡重复确认。此文件是交接材料，不代表已有终端收到任务。

## 先做

1. 读根 AGENTS/FRACTAL-DOCS 和将修改目录的 AGENTS；Hub UI 必须先读其指定四份规范。
2. 查看最新 HEAD、dirty work 与卡的执行记录。复审基线 c8aca9c8；若已有后续实现，复验并记录，不重复覆盖。
3. 当前 agent_spawn.rs、r11_tool_lock_matrix.rs、tool_batch.rs 已有未提交 R72 工作。你并非独占代码库：保留他人改动，不能 reset/checkout 覆盖。先排除 R72 的 test 构建 E0382，再做相关验收。

## 每卡完成定义

先让能覆盖该问题的行为反例失败，再修复并通过。静态卡必须补成实际回归，不得只改卡状态。

- 记录具体触发交错、测试命令、真实 pass/fail、实现 commit。
- 正常成功既要有 Completed，也不能有 fatal，并要保留期望正文；这是 R81–R83 已暴露的验收缺口。
- 取消类用 gate 固定 await/终止/下一轮交错；检查真实引擎调用、持久化事实、UI 事件和控制句柄，不只检查内部布尔量。
- UI 用受控 Promise、真实导航和目标内容断言；浏览器焦点/滚动验收不能只依赖 jsdom。不要仅刷新截图基线。
- 每个逻辑修复独立提交，遵守仓库 git 规范和适用技能。不要推送或发布，除非既有会话明确授权。
- 验证适量：改动稳定后跑相关包/模块；不需要每改一行重跑全仓。失败和未执行必须保留为 open，继续独立可做的卡。

## 本轮关键证据入口

- [成功误判](evidence/adapters/RESULTS.md)：两个真实 adapter execute 都失败，但 Normal harness 接受。
- [UI 异步问题](UI.md)：两个真实 App 受控 Promise 反例。
- [控制面 witness](evidence/server-control-probe.mjs)：分类器真实执行，其他为静态顺序检查，需你补集成回归。

实现完队列后，汇总每张卡的 commit/验收；还没做的列明原因。不要只报告“全绿”或“全部修完”，要让下一轮复审能逐卡核对。
