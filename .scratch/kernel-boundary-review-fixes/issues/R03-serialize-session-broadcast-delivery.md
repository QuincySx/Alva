# R03: 会话事件广播保证真实投递顺序

**Priority:** P2
**Axis:** Spec / 原修复不完整
**Origin:** 原卡 06；2026-09-09 review
**Blocked by:** 无
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题与证据

原卡 06 要求「并发追加的广播顺序与序号顺序一致；慢监听者不再阻塞追加与订阅」。
`crates/alva-agent-core/src/agent_session.rs` 的 drain_broadcasts 在取出一批事件后释放队列锁，
每个追加者都可进入自己的 drain。先前批次的异步回调暂停时，后续批次可先完成投递；
FIFO 入队不等于 FIFO 投递。

已复现：第一个监听者处理 seq=1 时等待 semaphore，同时 append seq=2，
连 `subscribe_events` 的 channel 订阅者也收到 `[2, 1]`。
测试：`concurrent_broadcast_preserves_seq_with_yielding_listener`，见 [复现材料](../evidence/README.md)。

## 实现范围

- 为实际投递建立单一顺序，隔离慢监听者对写入与订阅的影响；选择符合 executor-neutral/wasm 约束的方案。
- 不以在持久化锁或监听者列表锁内等待任意异步回调的方式修复。
- 保留历史回放到实时订阅的无遗漏、无重复契约，处理失效监听者回收。

## 验收

- [ ] semaphore 确定性乱序测试转绿，不依赖随机调度或 sleep 碰运气。
- [ ] 并发 append / append_message / append_records 的每个订阅者按 seq 递增接收。
- [ ] 有真正会 await 的慢监听者时，追加与注册/取消订阅仍可推进。
- [ ] 历史→实时交接无遗漏无重复，监听者回调读取/转发会话不死锁。
- [ ] 现有 listenable session 测试及所属 crate 的 wasm 检查通过。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
