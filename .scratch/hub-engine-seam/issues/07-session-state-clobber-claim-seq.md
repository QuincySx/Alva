# 07 旧轮的回滚快照覆写新轮状态（引入 claim_seq 守卫）

- 级别：P1
- 影响：同一个会话可能同时跑两条引擎流互相踩状态；引擎致命失败的一轮会被显示成"已完成"，随后 CLI 报 `completed session has no native engine session id` 并拒绝写入会话日志（下次 `--resume` 找不到这个会话）。
- 状态：TODO

## 前提：今天 blast radius 有限，路线 A 落地当天变线上并发 bug

三处缺陷都要求「上一轮的收尾代码」与「下一轮的认领代码」在同一个 `HubRuntime` 实例上交错。今天不容易触发，因为：

- `crates/alva-hub-desktop/src/run.rs:526` 与 `crates/alva-hub-gpui/src/run.rs:508` **每轮**都 `HubRuntime::new()` 重建，session map 不跨轮存活；
- 全仓 `HubRuntime::new()` 只有 5 个调用点（`hub-desktop/src/run.rs:137,526`、`hub-cli/src/main.rs:118`、`hub-gpui/src/run.rs:126,508`），其中唯一长生命周期实例是 hub-cli 的 `:118`，它被 REPL 循环 `crates/alva-hub-cli/src/main.rs:426-583` 复用（`:572` 每轮把同一个 `hub` 传进 `run_turn`），而 REPL 是串行单消费者。

`docs/specs/hub-headless-service.md:143` 的**路线 A**（复用 `alva-app-gateway` 加 hub 路由组，handler 调 `alva-hub-desktop::lib` 纯函数）一旦落地，hub 服务是常驻进程、多前端并发打同一个 `HubRuntime`，上面三条全部变成日常可触发的线上竞态。本工单要在路线 A 之前做完。

## 证据

### (a) `restore_failed_handoff` 整体覆写 session

- `crates/alva-hub-core/src/runtime.rs:752-768` —— `restore_failed_handoff` 只从当前 session 抄走三个诊断字段（`:762-764`），然后 `sessions.insert(product_session_id.clone(), previous)`（`:767`）**整体替换**整个 `HubSession`，包括 `status` 和 `engine_session_id`。
- `crates/alva-hub-core/src/runtime.rs:621-624` —— 调用点在 `tokio::spawn`（`:505`）出来的 task **尾部**，位于 stream 循环（`:516-557`）与 `contract.finish()`（`:558-588`）之后。
- `crates/alva-hub-core/src/runtime.rs:542-556` —— 窗口来源：`update_session` 先落 `Completed`，紧接着 `event_tx.send(...)` 把 `Completed` 推给消费者。消费者拿到 `Completed` 的那一刻，spawned task 还有 `:558-624` 一整段没跑完。
- `crates/alva-hub-core/src/runtime.rs:429-434,470` —— 下一轮 `execute` 的闸门是「读到 `Starting|Running` 就 `SessionBusy`」，随后在同一个写锁里把 `status` 置为 `Starting`。这个 `Starting` 正是 (a) 会被旧快照抹掉的那个值。
- 结论：`Starting` 被抹回旧快照的 `Completed` 后，第三个 `execute` 读到非 `Starting/Running`，闸门放行，**同一个 `ProductSessionId` 同时跑两条 engine 流**。

### (b) rollback 快照在 `status = Starting` 之前取，且 switch 后首轮的快照里 status 是 `Completed`

- `crates/alva-hub-core/src/runtime.rs:450` —— `let rollback = existing.clone();`
- `crates/alva-hub-core/src/runtime.rs:470` —— `existing.status = HubSessionStatus::Starting;` 在 `:450` **之后**，所以 `rollback.status` 是本轮之前的值。
- `crates/alva-hub-core/src/runtime.rs:385` —— `switch_engine` 收尾写 `session.status = HubSessionStatus::Completed;`（注释 "Ready for a new native session on the target engine."）。于是 switch 后首轮的 `rollback.status == Completed`。
- `crates/alva-hub-core/src/runtime.rs:381` —— 同一段还把 `session.engine_session_id = None`，所以 rollback 快照是 `status=Completed` + `engine_session_id=None` 这个自相矛盾的组合。
- `crates/alva-hub-core/src/runtime.rs:499,621-624` —— `failed_handoff_rollback = previous_session.zip(handoff_context_used)`，只在「有 rollback 且本轮消费了 handoff context」时非空，即恰好是 switch 后首轮；`saw_fatal_error` 时把这份快照整体写回。
- 下游可见：`crates/alva-hub-cli/src/main.rs:887` 用 `snapshot.status == HubSessionStatus::Completed` 决定是否落盘日志，`:888` 取 `engine_session_id` 落空后走 `:917-918` 打印 `alva-hub: completed session has no native engine session id` 并 `failed = true`。引擎致命失败被误报成"完成但缺 id"。
- `crates/alva-hub-core/src/runtime.rs:1420-1471` —— 现有测试 `fatal_first_handoff_stream_preserves_context_for_retry` 断言了 `pending_handoff`（`:1455`）、`engine_id`（`:1456`）、`engine_session_id`（`:1457`），**唯独没有断言 `status`**，所以这条路径今天是无人看守的。

### (c) `update_session` 对 fatal Error 无终态守卫（同源第三条）

- `crates/alva-hub-core/src/runtime.rs:701-706` —— 两个分支不对称：

  ```rust
  RuntimeEvent::Error {
      recoverable: false, ..
  } => session.status = HubSessionStatus::Failed,
  RuntimeEvent::Completed { .. } if session.status != HubSessionStatus::Failed => {
      session.status = HubSessionStatus::Completed;
  }
  ```

  `Completed` 有 `if status != Failed` 守卫，`Error` 一侧**无条件**写 `Failed`。
- `crates/alva-hub-core/src/runtime.rs:516-542` —— 契约检查发现违规后只是计数 + 日志（`:517-533`），**没有 `continue`**，`update_session(...)`（`:542`）照样对该事件执行。
- `crates/alva-engine-runtime/src/stream_contract.rs:235-240` —— `StreamContract::observe` 对 `Completed` 之后的任何事件返回 `StreamContractViolation::EventAfterCompleted`。也就是说：Hub **能识别**这是违规事件，识别完仍然拿它改状态。
- `crates/alva-hub-desktop/src/run.rs:922-945` —— 产品层自己的投影循环两个方向都有守卫（`Error` 分支 `:926` 判 `terminal_error.is_none()`，`Completed` 分支 `:945` 判 `if terminal_error.is_none()`）。产品层比 hub-core 更严谨，说明这个不对称是 hub-core 的缺陷而非有意策略。
- `crates/alva-hub-core/src/AGENTS.md:24` —— crate 自述约束："stream diagnostics 是可观测性信号，**不得**单独把成功 turn 标成 Failed。" 迟到的 fatal 把已 `Completed` 的 turn 翻成 `Failed`，与这条约束同精神相悖。

> **对上游描述的一处更正**：迟到 fatal 触发「ADR-0002 付费 fallback」的说法**不成立**。`crates/alva-hub-desktop/src/run.rs` 的 fallback 链由 `terminal_error`/`failure_class` 驱动（`:397`、`:640-670`、`:922-939`），这两个变量来自 `RuntimeEvent` 流而**不是** `HubSessionStatus`，`crates/alva-hub-desktop/src/run.rs:526` 每轮新建 runtime 也切断了这条路。已验证的实际下游只有 hub-cli 日志：`crates/alva-hub-cli/src/main.rs:887` 见到 `Failed` 就整段跳过 upsert，本轮的 `engine_session_id` 永久丢失，下次 `--resume` 无从恢复。

## 复现

### (b)(c) 静态可证 + 可直接落测（确定性，无并发）

推导链（全部行号见上）：

1. `switch_engine` 把 `status` 置 `Completed`、`engine_session_id` 置 `None`（`:381,385`）。
2. 下一次 `execute` 在 `:450` 取 `rollback = existing.clone()`（此刻 `status == Completed`），在 `:470` 才把实时状态改成 `Starting`。
3. 引擎流发 `Error{recoverable:false}` + `Completed`：`update_session` 先写 `Failed`（`:701-703`），`Completed` 被守卫挡住（`:704`），实时状态停在 `Failed`。
4. `saw_fatal_error == true` 且 `failed_handoff_rollback` 非空 → `:623` 把步骤 2 的快照整体写回 → 最终 `status == Completed`、`engine_session_id == None`。

可执行复现（现有 fixture 已够用，`FatalStreamOnceEngine` 见 `crates/alva-hub-core/src/runtime.rs:1424-1428` 的用法）：

1. 照 `fatal_first_handoff_stream_preserves_context_for_retry`（`:1420-1471`）走到 `let after_failure = hub.session(&product_id).unwrap();`（`:1454`）。
2. 在 `:1457` 后面加一行 `assert_eq!(after_failure.status, HubSessionStatus::Failed);`。
3. 观察到：断言失败，实际是 `HubSessionStatus::Completed`。
4. 期望：致命失败的一轮，`status` 必须是 `Failed`。

(c) 的独立复现（不涉及 handoff）：

1. 注册一个引擎，流依次发 `SessionStarted{session_id:"e1"}`、`Completed{session_id:"e1"}`、`Error{recoverable:false}`（`Completed` 在前，`Error` 在后）。
2. `execute` 后 drain 整条流，读 `hub.session(&id).unwrap().status`。
3. 观察到：`Failed`。契约在 `stream_contract.rs:235-240` 已把第三个事件判为 `EventAfterCompleted` 违规，`runtime.rs:542` 仍拿它改状态。
4. 期望：`Completed`（违规事件计入 `stream_hard_violations` 即可，不得改写终态）。

### (a) 并发复现（`multi_thread`，需要抢跑窗口）

`crates/alva-hub-core/Cargo.toml` 的 dev-dependencies 已含 `tokio` 的 `rt-multi-thread` + `macros`，可直接写：

1. `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`；注册一个 switch 后首轮必然 fatal 的引擎（`FatalStreamOnceEngine` 语义），`switch_engine` 后发起 execute A。
2. 消费者一收到 A 的 `Completed` 事件（`runtime.rs:544` 发出）就**立刻**发起 execute B（resume 同一个 `ProductSessionId`）。
3. B 返回 `Ok` 后不 drain，紧接着发起第三个 execute C。
4. 观察到的状态序列（上游实测）：
   - A 的 `Completed` 送达后：`status == Failed`
   - B 认领后：`status == Starting`
   - A 的尾部 `restore_failed_handoff` 落地后：`status == Completed`、`pending_handoff == true`、`engine_session_id == None`
   - C 返回 `Ok(_)` 而不是 `Err(HubError::SessionBusy(_))`
5. 期望：C 必须 `Err(HubError::SessionBusy(_))`；A 的尾部不得改动 B 已经写下的 `status` / `engine_session_id`。
6. 若单次跑不出交错，把步骤 1-4 包在 `for _ in 0..200` 里；窗口来源是 `runtime.rs:544`（送 `Completed`）到 `runtime.rs:623`（写回）之间的 `:558-620` 整段。

## 工作

全部改动在 `crates/alva-hub-core/src/runtime.rs`：

- `HubSession`（`:119-134`）新增 `claim_seq: u64` 字段，`HubSession::new`（`:136-150`）初始化为 `0`。该字段**不**进 `HubSessionSnapshot`（`:96-117`），产品层不需要看见。
- `execute` 认领处（`:470` 附近，写 `status = Starting` 的同一个写锁临界区内）`existing.claim_seq += 1`，把新值 copy 出来交给 spawned task；新建分支（`:475-478`）同样从新建的 session 取 `claim_seq`。
- spawned task（`:505-626`）持有该 `claim_seq`，并透传给 `update_session` / `record_hard_violation` / `record_soft_warnings` / `mark_session_failed` / `restore_failed_handoff`（`:683-768`）。
- 上述五个自由函数各加一道守卫：取到 session 后先比对 `session.claim_seq == expected_seq`，不等直接 `return`（旧轮的收尾一律不许写新轮的状态）。
- `restore_failed_handoff`（`:752-768`）改为**字段级**写入，不再 `sessions.insert` 整体覆盖：只写 `pending_handoff`、`pending_handoff_context`、`engine_id`、`previous_engine_id`、`previous_engine_session_id`；**绝不碰** `status` 与 `engine_session_id`。诊断字段的抄写（`:762-764`）随之删除——原地改写本来就不需要抄。
- `update_session`（`:701-703`）给 fatal `Error` 分支补上与 `Completed` 对称的终态守卫：`RuntimeEvent::Error { recoverable: false, .. } if session.status != HubSessionStatus::Completed`。
- 更新 `crates/alva-hub-core/src/AGENTS.md`：在「逻辑」第 6 条补充 rollback 只恢复 handoff 意图字段，在「约束」补一条 claim_seq 守卫（旧轮收尾不得写新轮状态）。

## 验收

- [ ] `cargo test -p alva-hub-core` 全绿。
- [ ] 新增测试 `fatal_handoff_turn_ends_failed_not_completed`：走 (b) 的复现路径，断言 `hub.session(&id).unwrap().status == HubSessionStatus::Failed`。
- [ ] 现有测试 `fatal_first_handoff_stream_preserves_context_for_retry`（`runtime.rs:1420`）补上 `status` 断言后仍通过，且 `:1468-1470` 三条原有断言（两次重试 prompt 都含 `SAFE_HANDOFF_BLOCK`、最终 `pending_handoff == false`）逐字不变。
- [ ] 新增测试 `fatal_error_after_completed_keeps_completed`：`Completed` 之后到达的 `Error{recoverable:false}` 不改写 `status`，同时 `stream_hard_violations >= 1`（违规仍被记录）。
- [ ] 新增测试 `stale_turn_tail_does_not_release_next_turn_claim`（`#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`）：按 (a) 步骤 1-4 循环 200 次，全程第三个 `execute` 恒为 `Err(HubError::SessionBusy(_))`。
- [ ] 变异验证：临时把 `restore_failed_handoff` 的 seq 守卫去掉，上一条测试必须失败；恢复后通过。
- [ ] `cargo clippy -p alva-hub-core --all-targets -- -D warnings` 无新增告警。
- [ ] `crates/alva-hub-core/src/AGENTS.md` 已更新，且 `HubSessionSnapshot` 的公开字段集合未变（`git diff` 中 `pub struct HubSessionSnapshot` 块零改动）。
