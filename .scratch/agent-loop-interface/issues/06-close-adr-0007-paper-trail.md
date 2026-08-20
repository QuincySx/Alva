# 06 — Close the ADR-0007 paper trail

**What to build:** Ticket 01 预先约定了一个兜底：「若无法既 object-safe 又不改行为，PR 需记录折衷并回写 ADR-0007」。实际落地的 `&mut AgentState → &AgentState` 改动覆盖了整个 `Middleware`/`PhaseHandler` trait 及其全部实现（27 个文件），明显比 ticket 01 设想的"只有 `state.extensions.insert(cancel)` 这一处需要处理"要宽——这个兜底该触发但没触发。同时 ticket 01/02/03 本身的 checkbox 和 Status 也从没随实现更新过。本票据把 ADR 与三张 ticket 的记录，同步成跟实际交付一致的状态。

**Blocked by:** None — can run independently of / in parallel with 04 and 05.

**Status:** done/shipped — 经 `3c21c1d docs(adr): close ADR-0007 paper trail` + 本次追加的 `MiddlewareStack` wrap-chain Clone caveat 落地。

- [x] `.scratch/agent-loop-interface/issues/01-loop-interface.md`：勾选所有 checkbox，`Status: ready-for-agent` 改成 done/shipped，标注落地 commit（`bebc7d6` 解决 borrow 约束的前置步骤，`7fd8ae0` 交付接口本身）。
- [x] `.scratch/agent-loop-interface/issues/02-route-callers.md`：同上，标注 `7fd8ae0`（三个调用方改走接口）。
- [x] `.scratch/agent-loop-interface/issues/03-second-implementation.md`：同上，标注 `9003f7f`。
- [x] `docs/adr/0007-agent-hub-abstraction-alignment.md`：在「Follow-up Research」小节之后补一个简短的「Implementation Note」，记录：
  - 实际落地范围比 ticket 01 设想的更宽——`0f44bb8` 把 `&mut AgentState → &AgentState` 覆盖到了整个 `Middleware`/`PhaseHandler` trait 及跨 crate 的全部实现（27 个文件），而不只是 ticket 01 点名的 `extensions.insert` 那一处。
  - 为什么这仍然安全：这是纯签名改动，Rust 编译器保证没有代码通过旧 `&mut` 真正写字段（否则根本编译不过）；已逐一 spot-check 非平凡文件确认无逻辑漂移，且 `cargo build --workspace`/`ci-check-deps.sh`（含 wasm32 link build）/`cargo test -p alva-kernel-core` 全部验证通过。
  - 这条 Implementation Note 本身就是 ticket 01 那条兜底条款要求的回写，之前跳过了，现在补上。
- [x] 验证：纯文档改动，跑 `git diff` 确认只碰了 `.md` 文件。
