# R25: 完成广播 driver 的唤醒、取消隔离和回收

**Priority:** P1
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R16
**Blocked by:** 无
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

crates/alva-agent-core/src/agent_session.rs:719、765 仍在 ensure_driver 后 await drain_broadcasts；:828 的 append_records 提前返回，但 delivery_notify 从无 notify 调用，:634 等待后无法再醒。:643 注释声称交付后移除，:649 实际 mem::take。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

统一投递路径，明确定义 native/executor-neutral/wasm 的推进与关闭；驱动任务不能由追加者取消，空闲后可被唤醒，会话弃置能退出。不要用多套 drain 或额外无界任务掩盖问题。

## 验收

- [x] 阻塞 listener 时首个 append、append_message、append_records 都按契约返回；外层调用者真正 abort 后，已有实时订阅仍收到已提交事件。
- [x] 先消费首批，driver 进入空闲，再只追加 typed records，已有订阅及时收到；不能用新建订阅的历史回放证明。
- [x] 顺序、去重、listener 回收、关闭、非 Tokio/wasm 适用路径有测试；修正既有名为 cancelled 但无 abort 的测试。

## 执行记录（执行者填写）

- Commit：`77db3d8`（与 R26 同 commit——代际隔离是同一次投递路径重写的两半）。
- 专项复现/回归命令、版本、结果（rustc 1.98.1，HEAD `77db3d8`）：
  - `cargo test --offline -p alva-agent-core --test review_broadcast_regression` — 12/12 通过，覆盖：阻塞监听者下 append/append_message/append_records 均按契约返回、空闲 driver 被 typed append 唤醒（已有订阅流证明，非历史回放）、真实 abort（JoinError::is_cancelled）后已提交事件仍送达已有实时订阅、顺序（seq 单调）、监听者回收、无 Tokio runtime 内联路径、带队列事件弃置会话的优雅退出。
  - `cargo test --offline -p alva-agent-core --test review_subscription_probe` — 通过（历史/实时不重不漏保持）。
  - `cargo test --offline -p alva-agent-core` — 全部通过（含 5 个改为确定性等待 driver 的单测）。
  - 假 cancelled 测试已修正为真实 abort 并断言 `is_cancelled`。
  - 统一投递路径：`BroadcastCore::deliver_pending` 是唯一实现，原 `drain_broadcasts`/`drain_core` 两套已删除；追加者 commit→notify 段无 await 点，取消不可能留下"已入队未唤醒"事件。
- 未完成项：无（wasm 路径靠 `#[cfg]` 内联兜底 + 非 Tokio 测试覆盖；真实 wasm 运行时验证未做，属既有边界而非本卡新增缺口）。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
