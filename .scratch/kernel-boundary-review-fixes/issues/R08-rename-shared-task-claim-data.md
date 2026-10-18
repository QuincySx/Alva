# R08: 资源持有者与等待者使用中性的共享数据名

**Priority:** P3
**Axis:** Standards / 已接受的命名建议（非行为故障）
**Origin:** 原卡 18；2026-09-09 review
**Blocked by:** 无
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题

`crates/alva-hub-desktop/src/store/orchestration.rs` 的
`older_waiters: Vec<RunningHolder>` 把尚未获得资源的等待者命名为运行中持有者。
同一结构实际表达 task_id、worker_id、claims；集合本身才区分当前持有与公平性预留。

## 实现范围

- 采用中性名称（例如 TaskClaims），同步 `store/team.rs` 的导入、构造与相关注释。
- 保留 holders / older_waiters 的明确角色，不改变 SQL、FIFO、公平性预留、生命周期判断或状态写入。
- 这是机械重命名，不抽 trait、不新增策略层；保持原卡 18 的行为零变化约束。

## 验收

- [ ] 同时代表持有者与等待者的结构名称不再暗示 running 状态。
- [ ] 调度条件、查询排序、状态转换、序列化和存储结构均不变。
- [ ] 团队编排既有测试断言不改且通过；无需新增仅验证名称的测试。
- [ ] 对应注释和目录文档名称同步。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
