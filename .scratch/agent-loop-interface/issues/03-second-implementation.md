# 03 — Prove swappability with a second in-repo loop implementation

**What to build:** 写一个最小「第二 loop 实现」证明接口真的可替换，而非只有默认实现能跑。端到端效果：hub / 第三方 harness 能注册自己的 loop 实现，内核不因替换而行为漂移。用测试替身即可，不必是产品功能。

**Blocked by:** 02 — Route the three real callers through the loop interface

**Status:** shipped

- [x] 一个测试性质的第二 loop 实现，经 `dyn` 分发后能跑通并被断言。
- [x] 证明「接口 == 替换点」：换上第二实现不破坏编译、不破坏测试替身的既有契约。
- [x] 该实现是测试/示例性质，不作为产品功能上线。

**Landed:** `9003f7f`（`crates/alva-kernel-core/tests/agent_loop_swappable.rs`，2 tests 通过）
