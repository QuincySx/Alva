# 02 — Route the three real callers through the loop interface

**What to build:** 把 SDK `Agent`、harness `BaseAgent`、WASM facade 三处「直接调 `run_agent`」改为通过 ticket 01 的 loop 接口调用。端到端效果：三个产品入口走可替换的 loop 入口，行为与迁移前逐位一致（同一事件流、同一取消/审批语义）。

**Blocked by:** 01 — Make `run_agent` callable through an object-safe loop interface (expand)

**Status:** shipped

- [x] 三个调用方从 `run_agent(...)` 改为经 loop 接口调用。
- [x] 每个调用方产出的 `AgentEvent` 序列与迁移前逐位一致，用「接口跑一遍 == 直接 run_agent 跑一遍」的等价断言验收。
- [x] 三个调用方各自的既有测试通过，无行为回退。
- [x] `cargo build` + 全量测试 + `ci-check-deps.sh` 全绿。

**Landed:** `7fd8ae0`
