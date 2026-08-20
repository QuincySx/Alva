# 01 — Fix periodic session flush undercounting on long runs

**What to build:** `crates/alva-kernel-core/tests/integration.rs:1890`（`session_is_flushed_periodically_during_a_long_run`）断言 25 轮迭代应该在第 10、20 轮各触发一次周期性 flush（加上 run 结束时那一次，总共 ≥2 次可观测的周期性 flush），但实测只观察到 1 次——等同于只有 agent-end 那次 flush 发生了，周期性 flush 完全没触发。

这是一个跟 ADR-0007 / `AgentLoop` 改动**完全无关**的既有 bug：已经在 `a89b4f6`（ADR-0007 工作开始前的最后一个 commit）单独开的 worktree 里复现了一模一样的失败（`got 1`），说明周期性 flush 机制本身有问题，不是某次重构引入的回归，也不是这条测试自己的问题。

**Blocked by:** None — can start immediately, independent of everything in `.scratch/agent-loop-interface/`.

**Status:** done/shipped — 经 `19aa7d0 fix(kernel-core): flush-test session missed append_records, aborting long run` 修复并验证。

**根因（与初步推测不同）：** 周期性 flush 机制本身没有坏——`run/mod.rs` 的 inner loop 里 `FLUSH_EVERY_N_ITERATIONS=10` 的迭代取模触发逻辑正确。真正的病根是**测试夹具 `FlushCountingSession` 漏了 `append_records` override**。`AgentSession::append_records` 默认实现刻意返回 `Err("atomic typed record append is not implemented by this backend")`，而 `tool_batch::commit_tool_intent` 每轮依赖它。于是第一次 tool 执行就失败、run 在第 1 轮就 abort，根本跑不到第 10/20 轮的迭代边界 flush——误判为"周期 flush 没触发"。

- [x] 定位 kernel-core 里"周期性 flush"的触发逻辑（在 `run/mod.rs` inner loop 迭代边界按 `total_iterations.is_multiple_of(FLUSH_EVERY_N_ITERATIONS=10)` 或超 `FLUSH_INTERVAL=30s` 触发；通过探针确认 iter=1 后就 abort，未到周期 flush 边界）。
- [x] 判断根因：不是触发条件写错，而是测试夹具 `FlushCountingSession` 缺 `append_records`，导致 `execute_batch` 在工具执行时返回 Err、run_loop 第 1 轮就提前 return（探针 `tool_calls=1` 后无 `committed=` 输出，证明 `execute_batch().await?` abort）。
- [x] 修掉根因：给 `FlushCountingSession` 补 `append_records` override delegate 到 inner `InMemoryAgentSession`（其实现完整）。修复后长跑到 25 轮，周期 flush 在第 10/20 轮正常触发，测试转绿；完整 `cargo test -p alva-kernel-core` 全绿（132 unit + 20 integration，0 failed）。
- [x] 单独提交 commit（`19aa7d0`），未与其它工作混在一起。
