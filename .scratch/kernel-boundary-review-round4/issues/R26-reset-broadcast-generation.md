# R26: 隔离 clear 与在途广播的序号代际

**Priority:** P2
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R17
**Blocked by:** R25
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

crates/alva-agent-core/src/agent_session.rs:884 clear 只清共享队列并重置水位和 cursor；正在 drain 的本地 batch 不受影响。阻塞旧 seq1 → clear → 追加新 seq1 → 放行旧回调，旧 advance_to 会把新游标推到2，新事件被跳过。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

为 clear/restore/rollback/compact 定义统一事件与订阅契约，采用单调序号或显式代际/关闭协议。避免只锁队列或重置数字；注意 callback 重入时锁次序。

## 验收

- [x] 用 gate 确定性覆盖在途旧批次与 clear 交错：新事件不漏，旧事件不污染新代际。
- [x] 既有和新订阅都满足明确契约，无死锁；restore/rollback 历史到实时边界有测试。
- [x] 保持简单 clear 场景和顺序/去重原回归通过。

## 执行记录（执行者填写）

- Commit：`77db3d8`（与 R25 同 commit）。
- 专项复现/回归命令、版本、结果（rustc 1.98.1，HEAD `77db3d8`）：
  - `cargo test --offline -p alva-agent-core --test review_broadcast_regression clear_during_delivery_does_not_skip_new_generation` — 通过：gate 确定性构造"阻塞旧事件投递 → clear → 追加新 seq1 → 放行"，新事件不漏、旧 `advance_after` 因代际不匹配不污染新游标。
  - 同文件 `restore_then_append_reaches_new_subscriber`、`rollback_then_append_still_reaches_subscriber` — 通过（restore/rollback 的历史→实时边界；rollback 不重置序号）。
  - 同文件 `subscription_after_clear`（简单 clear 场景，原探针）与顺序/去重回归均通过。
  - 契约：`BroadcastEntry{generation, event}` + `ListenerCursor{generation, next_seq}`；clear 在队列锁内 fetch_add 代际并重置全部游标；clear 不取 delivery_lock（回调内重入 clear 不死锁）。
- 未完成项：无。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
