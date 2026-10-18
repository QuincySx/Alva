# 第一轮 Review 修复清单（归档）

> **ARCHIVED：R01–R08 已有实现并于 2026-09-10 复审，不再作为待办。**
> 当前唯一执行入口：[第三轮 R14–R23 + UI01–UI10](../kernel-boundary-review-round3/TASKS.md)。
> 实现范围 `6e5bf33...134b8b3`；归档不代表全部验收通过，遗留/回归已建新卡。
> 下方顺序与执行要求仅为历史记录。

## 本轮任务

`archived-reviewed` 表示内容已可执行；仍须满足 Blocked by，不表示可越过依赖。

| 卡 | 优先级 | 标题 | Blocked by | 状态 |
|---|---|---|---|---|
| [R01](./issues/R01-restore-child-shared-tool-locks.md) | P1 | 子 agent 恢复共享资源锁约束 | — | archived-reviewed |
| [R02](./issues/R02-wire-production-llm-analytics.md) | P1 | 把 LLM 埋点接回生产 AnalyticsPlugin | — | archived-reviewed |
| [R03](./issues/R03-serialize-session-broadcast-delivery.md) | P2 | 会话事件广播保证真实投递顺序 | — | archived-reviewed |
| [R04](./issues/R04-finalize-on-run-start-write-error.md) | P2 | run_start 写入失败也必须发送错误终态并收尾 | — | archived-reviewed |
| [R05](./issues/R05-narrow-the-actual-kernel-session-input.md) | P1 | 内核入口真正只要求 ConversationLog | R01–R04 | archived-reviewed |
| [R06](./issues/R06-sync-fractal-module-documentation.md) | P2 | 补齐重构后的分形文档与模块所有权地图 | R01–R05、R07、R08 | archived-reviewed |
| [R07](./issues/R07-move-engine-test-builder-out-of-public-api.md) | P3 | 测试构造便利函数移出生产公开 API | — | archived-reviewed |
| [R08](./issues/R08-rename-shared-task-claim-data.md) | P3 | 资源持有者与等待者使用中性的共享数据名 | — | archived-reviewed |

## 建议执行顺序

**R01 → R02 → R03 → R04 → R05 → R07 → R08 → R06**。

1. 先修四个已复现的行为问题，分别迁入回归测试，先红后绿。
2. 再做 R05 的真正契约收窄，确保前四张的行为仍成立。
3. R07/R08 是低优先级的 API/命名整理，各自单独提交。
4. R06 最后核对整轮文档一致性；每张前置卡修改时仍须即时更新自己的分形文档。

## 执行记录要求

完成每张卡时更新本表与卡内状态，填写 commit、实际验证命令和结果；遇到失败保留证据。
本轮最终运行完整测试套件、公开符号快照及项目要求的依赖/wasm 检查，再执行代码审查。
不能把已有定向测试通过写成全量通过。这里不包含推送、部署或发布任务。
