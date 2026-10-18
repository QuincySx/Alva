# R13: 清理本轮残留的旧模块与接线文档

**Priority:** P3
**Axis:** Standards / R06 未完整覆盖
**Origin:** 2026-09-10 review，`6e5bf33...134b8b3`
**Blocked by:** R09–R12
**Status:** archived-reviewed

> 已于2026-09-11复审；未完成行为见[当前第三轮清单](../../kernel-boundary-review-round3/TASKS.md)。不重新按本卡开工；归档不表示全部验收通过。

## 问题与证据

FRACTAL-DOCS.md 第三层要求接口/职责变化级联更新目录文档。
1. `crates/alva-app-core/src/AGENTS.md:62` 仍写 AnalyticsPlugin 通过 phase_handler 注册两个 phase，但 R02 已改为 raw AnalyticsMiddleware。
2. `crates/alva-kernel-core/src/AGENTS.md:37–38` 仍列已迁走的 tool_lock.rs、pending_queue.rs，与同文件 :14 冲突。
这是两项明确规范违例，合为一张文档修复卡；不是重新执行旧 R06 全部工作。

## 实现范围

- 更新上述目录职责与业务域表，并核对 R09–R12 修改影响的文件头、目录及上级说明。
- 清单中的文件必须真实存在，所有权必须与代码一致；避免用修复卡编号代替长期架构说明。

## 验收

- [ ] Analytics 描述与真实 register 路径一致。
- [ ] kernel-core 模块表不再列迁走模块，新所有者说明正确。
- [ ] 人工核对各受影响目录文件表与真实代码；记录检查范围。
- [ ] 不把纯内部变动扩展为无关文档重写。

## 执行记录

遵守根及目录 AGENTS.md、FRACTAL-DOCS.md；只修本卡范围。先复现再修复，每项逻辑改动独立提交。公开表面变化同步快照。完成时填写 commit、实际验证命令及结果；不得将静态推导写为测试已复现，或将定向测试写为全量通过。

- Commit：待填写
- 验证命令及结果：待填写
