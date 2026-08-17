# 01 — Make `run_agent` callable through an object-safe loop interface (expand)

**What to build:** 让「跑一次 agent 循环」能通过一个 object-safe 的接口表达，同时保留现有 `run_agent` 作为默认实现。端到端效果：外部 harness 能拿到一个可 `dyn` 分发的 loop 驱动对象，调用它等同于直接调 `run_agent`，产生的事件序列零差异。这一步只做「expand」——新接口与旧函数并立，不改任何现有调用方。

**Blocked by:** None — can start immediately.

**Status:** shipped

- [x] 在 `alva-kernel-core` 引入一个 object-safe 的 loop trait，方法入参对齐 `run_agent`（state、config、cancel、input、event channel）。
- [x] 默认实现转发到现有 `run_agent`，签名与行为不动。
- [x] 解决借用约束：实际方案是 `Extensions` interior mutability（`bebc7d6`）+ `&mut AgentState → &AgentState` 全量替换（`0f44bb8`），使接口自然 object-safe。折衷与回写见 ADR-0007「Implementation Note」。
- [x] 现有 `alva-kernel-core/tests/` 里直接 `run_agent(` 的测试继续通过。
- [x] `ci-check-deps.sh` 全绿，wasm-clean 集合的 `cargo check --target wasm32-unknown-unknown` 不回归。

**Landed:** `7fd8ae0`（接口本身）+ `bebc7d6`/`0f44bb8`（前置 borrow 约束解决）。
