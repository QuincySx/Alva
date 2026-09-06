# 18 claude 全模式：审批 ack 只证明"写进了 stdin"，迟到的响应被静默丢弃

- 级别：P0（与工单 03 同类；03 只修了 alva 侧，claude 侧是独立的一半）
- 影响：用户点「允许」，界面收起卡片、审计写下 `Allowed`，而 claude 那边这个工具在 60 秒前就已经被自动 deny、从未执行。
- 状态：**DONE 2026-09-06**（三步协议往返 + 60s 魔数下沉 Rust）

## 来源

工单 03 拆出。03 修好了 alva 引擎侧（`resolve_permission` 的返回值不再被丢弃），但 claude 全模式走的是完全不同的一条路，需要跨语言协议改动，规模与风险都不同，因此单列。

## 已核实

- `crates/alva-engine-adapter-claude/bridge/index.mjs:36-46` —— 迟到的 `permission_response` 命中 `pendingPermissions.get(...)` 返回 `undefined`，被 `if (resolve)` 挡住，**没有 else 分支**，静默丢弃且不回告 Rust 侧。
- `crates/alva-engine-adapter-claude/bridge/index.mjs:72-78` —— 60 秒自动 deny 的魔数，比引擎侧 `SecurityMiddleware` 的 300 秒短 5 倍，且这个数字没有暴露给任何上层。
- `crates/alva-engine-adapter-claude/src/adapter.rs:133-137` —— `forward_controls` 的 ack 语义：`match control.send(&message).await { Ok(()) => { let _ = acknowledgement.send(Ok(())); }` —— **ack 只证明这行 JSON 写进了 stdin**，与 bridge 是否采纳无关。
- `crates/alva-engine-adapter-claude/src/protocol.rs:11-26` —— `BridgeMessage` 是 `#[serde(tag = "type")]` 的闭合 enum（SdkMessage / PermissionRequest / Done / Error），要接住新信号必须加变体。
- bridge 已有 `emit(type, data)` 机制（`bridge/index.mjs:23`），加一条出站帧的成本很低。

## 三个数字互不知情（工单 03 已记录，是本工单的动机）

| 层 | 超时 | 位置 |
|---|---|---|
| claude bridge | **60s** | `bridge/index.mjs:78` |
| Hub stall 看门狗 | 180s | `crates/alva-hub-desktop/src/run.rs` `STALL_WINDOW` |
| 引擎侧 SecurityMiddleware | 300s | `crates/alva-agent-security/src/middleware/security.rs:122` |

只要用户在第三张审批卡上花掉一分钟，第一张卡就已经在 claude 侧过期了，而它看起来和刚弹出来时一模一样。

## 工作

这不是打补丁，是一次协议往返设计，**三步不能拆开做**（只做前两步会让 `respond_permission` 仍然返回 Ok，反而给出"已修"的错觉）：

1. bridge 的 `if (resolve)` 补 `else`，`emit("permission_response_rejected", { request_id, reason: "expired" })`。
2. `BridgeMessage` 加对应变体，`mapping.rs` 消费它。
3. **`respond_permission` 改为等待语义 ack**：维护 `request_id -> oneshot` 映射，收到 accepted / rejected 才 resolve，并为"bridge 从不回复"设一个上限（否则 UI 会挂住）。rejected 翻成 `RuntimeError::PermissionNotFound`，与 alva 侧（工单 03）行为一致。

另外二选一：
- 60s 魔数改为 Rust 侧下发（与引擎侧 approval_timeout 统一口径）；
- 或直接去掉，改由 `signal` 的 abort 路径（`bridge/index.mjs:65-71`）统一收口。

**注意**：真正的根治是让引擎上报审批期限、Hub 不再自己猜——那属于工单 15（EngineDescriptor 的能力协商面），本工单只负责让 claude 侧停止说谎。

## 验收

- [ ] `grep -n 'else' crates/alva-engine-adapter-claude/bridge/index.mjs` 在 36–46 行区间有命中，且该分支调用 `emit(...)`。
- [ ] `grep -rn '60_000' crates/alva-engine-adapter-claude/bridge/index.mjs` 无命中。
- [ ] 新增单测：bridge 回 `permission_response_rejected` 时，`respond_permission` 返回 `Err(RuntimeError::PermissionNotFound(_))`。
- [ ] 新增单测：bridge 不回任何 ack 时，`respond_permission` 在上限内返回 Err 而不是永久挂起。
- [ ] **变异验证**（工单 01/02 的教训）：把 ack 改回"写成功即 Ok"后，上述测试必须变红；仅断言"返回了某个 Err"不够，要钉死是哪一种 Err。
- [ ] 手工验证：Hub Desktop 跑 claude 全模式，等 65 秒后点「允许」，界面给出「该审批已过期」，时间线上**没有** `Allowed` 行。
- [ ] `cargo test -p alva-engine-adapter-claude` 全绿。
