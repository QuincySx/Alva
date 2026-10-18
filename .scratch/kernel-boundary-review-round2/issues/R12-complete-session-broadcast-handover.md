# R12: 补齐广播历史交接与慢监听者隔离

**Priority:** P2
**Axis:** Spec / R03 未完整覆盖
**Origin:** 2026-09-10 review，`6e5bf33...134b8b3`
**Blocked by:** 无
**Status:** archived-reviewed

> 已于2026-09-11复审；未完成行为见[当前第三轮清单](../../kernel-boundary-review-round3/TASKS.md)。不重新按本卡开工；归档不表示全部验收通过。

## 问题与证据

`agent_session.rs:512–529` 用 delivery_lock 单一 drainer 修复乱序，但投递时才读取 listeners 快照。
确定性场景：阻塞 seq=1 的异步监听回调；append seq=2 入队返回；此时 subscribe_events(0) 回放历史 [1,2] 并注册新 listener；释放 seq=1，下一批 seq=2 再投给新 listener，得到 [1,2,2]。
此外赢得 delivery_lock 的 append 仍直接 await 任意监听者；不能因为第二个 append 可返回就声称满足“慢监听者不阻塞追加”。取消正在 drain 的追加还可能丢掉已从 queue 取出的未投递事件，需一起验证。
R03 明确要求历史→实时无遗漏无重复，以及真正异步慢监听者下追加仍推进。复现测试种子位于 evidence，实际运行状态见证据说明。

## 实现范围

- 建立明确的历史/实时序号边界，保障并发追加与订阅恰好一次交接。
- 将任意慢回调与写入完成隔离，明确投递任务/队列的生命周期、取消与错误处理。
- 同时审查 drainer 退出与新入队交接，防止 stranded queue；保留 executor-neutral/wasm。
- 不以禁用异步监听者、缩减现有契约或把回调放入存储锁修复。

## 验收

- [ ] 所附重复投递测试转绿，历史 [1,2] 后不再收到重复 seq=2。
- [ ] 原乱序回归继续通过；append/append_message/append_records 按 seq 投递。
- [ ] 阻塞第一个监听回调时，第一个追加本身与后续追加、订阅均可完成。
- [ ] 取消追加者不丢失已提交事件；并发订阅/取消、回调转发不死锁。
- [ ] 所属 crate session 测试与 wasm 检查通过。

## 执行记录

遵守根及目录 AGENTS.md、FRACTAL-DOCS.md；只修本卡范围。先复现再修复，每项逻辑改动独立提交。公开表面变化同步快照。完成时填写 commit、实际验证命令及结果；不得将静态推导写为测试已复现，或将定向测试写为全量通过。

- Commit：待填写
- 验证命令及结果：待填写
