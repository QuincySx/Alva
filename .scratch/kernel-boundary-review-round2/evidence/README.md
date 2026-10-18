# 2026-09-10 Review 证据

审查 HEAD：`134b8b3`；diff 基线：`6e5bf33`。本轮只 review/建卡，临时测试已移回本目录，生产源码及既有测试未修改。

## 已实际运行

```sh
cargo test -p alva-app-core --test review_wiring_regressions -p alva-agent-core --test review_broadcast_regression -p alva-kernel-core --test minimal_log_consumer --offline --locked
```

**PASS：5 tests**：原广播顺序 1、真实装配回归 3（子全局锁/LLM analytics/run_start 失败终态）、最小 ConversationLog 消费者 1。完整输出见 [existing-targeted-tests.txt](./existing-targeted-tests.txt)。

```sh
cargo test -p alva-agent-core --test review_subscription_probe --offline --locked
```

**FAIL：1 test**，`subscribe_during_delivery_does_not_duplicate_history` 断言 `queued event was delivered again after history replay` 失败。输出见 [subscription-probe.txt](./subscription-probe.txt)，源码见 [review_subscription_probe.rs](./review_subscription_probe.rs)。
复现时将源码复制到 `crates/alva-agent-core/tests/review_subscription_probe.rs` 后执行上述命令；修复者可将其作为正式回归测试并同步分形文档。
测试用 semaphore 固定 seq1 回调暂停→seq2 入队→新订阅回放→恢复投递的顺序；30ms timeout 仅用于检查不应出现的第三条事件，当前第三条立即出现。

## 静态结论与验证边界

- R09：RuntimeExecutionContext 默认 None / with_store_session 无生产调用 / agent_spawn 读取它，静态接线确认；未另跑持久化仓库端到端测试。
- R10：configure 的 OnceLock 与 swap_session 更新集合不一致，静态确认；未另跑切换后实际压缩测试。
- R11：child 仅 with_bus，workspace 未 configure，锁键按 workspace 解析，静态确认；现有 PASS 测试只覆盖 SerialGlobal。
- R12：重复投递已实际复现；首个追加 await 慢回调为静态确认；取消与退出交接列为修复验收，未声称已复现。
- R13：目录文档与实际文件/注册方式对照确认。
- 未运行全量 workspace 测试、完整依赖/wasm/public-surface 闸门，不宣称全量通过。编译有 warning，原样保留于日志。
