# R16: 隔离慢监听者并保证追加取消后的实时投递

**Priority:** P2
**Axis:** Spec / R12；Standards 测试覆盖缺口
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** 无
**Status:** ready-for-agent

## 问题与证据

agent_session.rs:561 将排队事件 mem::take 到追加任务本地，:576 仍直接 await 任意监听回调。赢得 delivery_lock 的第一个 append 仍被慢监听者阻塞；abort 它会丢弃未投递的本地批次，已有实时订阅者永久漏事件。
新增 cancelled_append_task_does_not_lose_committed_events（review_broadcast_regression.rs:66–94）没有注册阻塞监听者、没有等待进入回调、没有 abort，只正常 task.await；它不能证明取消安全。
R12 要求第一个追加也不阻塞、取消追加者不丢失已提交事件。真实 gate/abort 复现种子见 evidence/review_round3_probes.rs；测试日志见证据说明。

## 实现范围

将交付生命周期与单个追加者解耦，明确定义 executor-neutral/wasm 下谁推进投递、关闭与回收，避免只让第二个追加者返回。修正名为“取消”的测试，保留已通过的历史去重与序号顺序验收。

## 验收

- [ ] 阻塞监听回调时，第一个 append 自身已返回；后续追加/订阅可继续。
- [ ] 将首个 append 完成验收与调用者取消验收分开。取消用例在 append 返回后增加调用者存活 gate，确定性等待监听回调进入，再 abort 外层调用者并断言 JoinError.is_cancelled；释放回调后，已存在的实时订阅者仍收到已提交事件。不得要求已正常返回的 append 任务还能被取消。
- [ ] 不仅检查新订阅历史回放；回调读取/转发、监听者回收和队列退出交接不丢不重不死锁。
- [ ] 原 seq 顺序、历史→实时去重以及所属 wasm 检查通过。

## 执行记录

- Commit：4d12490
- 内容：BroadcastCore 抽取 + Weak 引用的原生投递 driver：追加路径只入队并通知，
慢监听者不再阻塞 appender；wasm 保持内联 drain。
- 验证（2026-09-12 复跑）：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` 32 个测试目标全部 ok，655 passed / 0 failed；`scripts/ci-check-deps.sh` 通过（Rule 17/18/19 边界完好）。
- 实际验证命令及结果：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` → 32 个测试目标 ok，655 passed / 0 failed（2026-09-12 复跑）；`scripts/ci-check-deps.sh` exit 0。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
