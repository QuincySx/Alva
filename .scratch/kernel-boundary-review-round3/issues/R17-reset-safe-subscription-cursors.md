# R17: 清空会话后实时订阅不得跳过新事件

**Priority:** P2
**Axis:** Spec / R12 新回归
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** R16
**Status:** ready-for-agent

## 问题与证据

ListenableInMemorySession::clear（agent_session.rs:769）只委托 inner.clear；后者将 seq_counter 重置为 1，但 last_committed 和 RegisteredListener.cursor 保留旧值。
复现：append seq1 → clear → subscribe_events(0) → append 新 seq1。新订阅游标从旧 high-water+1 开始，漏掉这条刚提交的事件；已有订阅者也有同类问题。
这是游标去重引入的新回归。确定性复现种子 subscription_after_clear_receives_new_live_event 见 evidence/review_round3_probes.rs。

## 实现范围

定义 clear/restore/rollback/compact 与事件序号、订阅游标的契约；选择单调序号或明确重置/结束旧订阅，但不能在同一存活流里静默跳过新事件。不要只重置一个数字而漏掉现有订阅和排队事件。

## 验收

- [ ] clear 后立即建立的新订阅收到下一条真实追加；已有订阅按明确契约继续或显式结束。
- [ ] clear 与队列中尚未投递事件并发时无旧事件污染、无死锁。
- [ ] restore/rollback 后的历史→实时游标边界正确；追加序号契约明确并有测试。
- [ ] 原顺序与去重回归通过。

## 执行记录

- Commit：4d12490
- 内容：clear() 重置广播游标（queue/last_committed/listener cursor），
清空会话后新实时订阅从新事件开始，不跳发不重放。
- 验证（2026-09-12 复跑）：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` 32 个测试目标全部 ok，655 passed / 0 failed；`scripts/ci-check-deps.sh` 通过（Rule 17/18/19 边界完好）。
- 实际验证命令及结果：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` → 32 个测试目标 ok，655 passed / 0 failed（2026-09-12 复跑）；`scripts/ci-check-deps.sh` exit 0。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
