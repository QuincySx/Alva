# 第二轮 Review 修复清单（归档）

> **ARCHIVED：R09–R13 已实施并于2026-09-11复审，不再作为当前待办。**
> 当前唯一执行入口：[R14–R23 + UI01–UI10](../kernel-boundary-review-round3/TASKS.md)。
> 归档不表示全部验收通过；遗留/回归转入新卡。下方顺序和交接指令仅是历史记录。
> 复审基线 `134b8b3`；审查 diff `6e5bf33...134b8b3`。新 HEAD 开工前先核对问题仍存在。

| 卡 | 优先级 | 标题 | Blocked by | 状态 |
|---|---|---|---|---|
| [R09](./issues/R09-restore-persistent-child-session-wiring.md) | P1 | 恢复生产工具上下文的持久化子会话能力 | 无 | archived-reviewed |
| [R10](./issues/R10-keep-session-capabilities-current-on-swap.md) | P1 | 切换会话后压缩必须写入当前会话 | 无 | archived-reviewed |
| [R11](./issues/R11-configure-child-lock-workspace.md) | P1 | 子 Agent 资源锁继承工作区解析 | 无 | archived-reviewed |
| [R12](./issues/R12-complete-session-broadcast-handover.md) | P2 | 补齐广播历史交接与慢监听者隔离 | 无 | archived-reviewed |
| [R13](./issues/R13-finish-fractal-module-map-sync.md) | P3 | 清理本轮残留的旧模块与接线文档 | R09–R12 | archived-reviewed |

R09/R10 涉及同一会话能力装配，建议同一执行者依次完成，保持独立提交；随后 R11、R12，最后 R13。
`archived-reviewed` 不豁免 Blocked by。每卡完成后更新状态、commit、真实验证结果；最终执行相应测试、公开符号与依赖/wasm 闸门，再 review。

## 交接指令

> 只执行 `.scratch/kernel-boundary-review-round2/TASKS.md` 的 R09–R13。先读 README 与 evidence，核对当前 HEAD；按卡复现、修复、验证、独立提交并记录。旧两轮卡仅作来源，不重新开工。本任务不包含推送、部署。
