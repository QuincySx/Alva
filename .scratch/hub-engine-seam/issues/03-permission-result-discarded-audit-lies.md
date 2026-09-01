# 03 审批结果被丢弃：引擎早已拒绝，UI 和审计记录却说「已允许」

- 级别：P0
- 影响：用户点了「允许」，界面收起卡片、审计留下一条「Allowed」，但引擎那边这个工具早已超时被拒——事后翻记录会认为是自己批准的操作，实际从未发生（反之亦然）。
- 状态：TODO

## 证据

### 第一条：alva adapter 丢弃 `resolve_permission` 的返回值

- `crates/alva-engine-adapter-alva/src/adapter.rs:384-385` ——

  ```rust
  agent.resolve_permission(request_id, "hub", decision).await;
  Ok(())
  ```

  返回值被整个丢掉，无条件 `Ok(())`。
- `crates/alva-app-core/src/base_agent/agent.rs:397-412` —— `pub async fn resolve_permission(...) -> bool`；无 `SecurityGuard` 时返回 `false`（`:411`），有则转发给 guard（`:409`）。
- `crates/alva-agent-security/src/guard.rs:496-504` —— `SecurityGuard::resolve_permission` 转给 `permission_manager.resolve(...)`。
- `crates/alva-agent-security/src/permission.rs:121-145` —— `resolve` 的语义就是「pending 里还在不在」：`if let Some(pending) = self.pending.remove(request_id) { ...; true } else { false }`。**`false` = 这个审批已经不存在了，我没能把你的决定送出去。**

### 引擎侧到底何时把它从 pending 里拿掉

- `crates/alva-agent-security/src/middleware/security.rs:110` / `:122` / `:141` —— `approval_timeout: Duration`，两个构造函数都默认 `Duration::from_secs(300)`。
- `crates/alva-agent-security/src/middleware/security.rs:357-371` —— `tokio::select!` 在「用户决定 / 运行被取消 / 超时」三者间竞速。
- `crates/alva-agent-security/src/middleware/security.rs:436-450` —— `TimedOut` 分支：`guard.cancel_permission(&request_id)`（`:438`，把 pending 移除）、发 `ResolveStatus::TimedOut`、返回 `Err(MiddlewareError::Blocked { reason: "approval for '<tool>' timed out" })`。

即：300 秒之后，工具已经被 **Blocked**，pending 已清空。此后用户点「允许」，`resolve` 返回 `false`，adapter 照样报 `Ok`。

### 谎言落到哪里

- `crates/alva-hub-desktop/src/permissions.rs:339-361` —— `PermissionRouter::respond` 先从自己的 pending map 里 `remove`（`:349`，**卡片就此消失**），再把决定交给 engine，engine 的 `Ok` 原样返回。
- `crates/alva-hub-desktop/src/main.rs:434-455` —— 单条审批命令 `respond_hub_permission`：`respond(...).await?` 通过后，`:439-449` 拼出 `format!("{} '{}'...", if request.allow { "Allowed" } else { "Denied" }, ...)`，`:450-452` 调 `record_permission_decision` 落盘。
- `crates/alva-hub-desktop/src/main.rs:499-518` —— 分组审批命令 `respond_permission_group` 对 `outcome.answered` 里每一个成员做同样的事（`:502` 同样的 `"Allowed"` / `"Denied"` 三元）。`answered` 的来源是 `crates/alva-hub-desktop/src/permissions.rs:387-393`：**`Ok(())` 就算 answered**。
- `crates/alva-hub-desktop/src/store/team.rs:726-753` —— `record_permission_decision` 把这句话作为 `"permission_decision"` 类型的时间线行**持久化**（`:738-748`）。这不是一条日志，是会话审计轨迹里的一行。

### 第二条：claude 全模式 bridge 的 60 秒自动 deny

- `crates/alva-engine-adapter-claude/bridge/index.mjs:72-78` ——

  ```js
  // Timeout: auto-deny after 60s if no response
  setTimeout(() => {
    if (pendingPermissions.has(requestId)) {
      pendingPermissions.delete(requestId);
      resolve({ behavior: "deny", message: "Permission timeout" });
    }
  }, 60_000);
  ```

  60 秒，比引擎侧的 300 秒短 5 倍，且这个数字没有暴露给任何上层。
- `crates/alva-engine-adapter-claude/bridge/index.mjs:36-46` —— 迟到的 `permission_response` 命中 `pendingPermissions.get(...)` 返回 `undefined`，被 `if (resolve)` 挡住，**没有 else，静默丢弃**，也不回告 Rust 侧。
- `crates/alva-engine-adapter-claude/src/adapter.rs:515-536` —— `respond_permission` 只是 `send_control(...)` 发一条 `BridgeOutbound::PermissionResponse`。
- `crates/alva-engine-adapter-claude/src/adapter.rs:133-137` —— `forward_controls` 的 ack 语义：`match control.send(&message).await { Ok(()) => { let _ = acknowledgement.send(Ok(())); }` ——**ack 只证明这行 JSON 写进了 stdin**，与 bridge 是否采纳无关。同一路径的 `Ok(Ok(())) => Ok(())` 在 `:115`。

## 复现

### A. alva 引擎超时（可执行，无需等 300 秒）

1. 构造 `SecurityMiddleware` 时用 `.with_approval_timeout(Duration::from_millis(20))`——这条构造器本来就存在（`crates/alva-agent-security/src/middleware/security.rs:155-158`），既有测试也这么用（`:719`、`:937`）。
2. 用它建 `BaseAgent`，接进 `AlvaAdapter`，跑一个会触发审批的工具，收到 `RuntimeEvent::PermissionRequest`。
3. 等 100ms（超过 20ms 阈值）。
4. 调 `adapter.respond_permission(&session_id, &request_id, PermissionDecision::Allow { updated_input: None }).await`。
5. 观察到：返回 `Ok(())`。而工具那边早已经拿到 `MiddlewareError::Blocked { reason: "approval for '<tool>' timed out" }`。
6. 期望：返回 `Err(RuntimeError::PermissionNotFound(request_id))`，让 `crates/alva-hub-desktop/src/permissions.rs:360` 把它变成 `Err`，`crates/alva-hub-desktop/src/main.rs:437` 的 `?` 提前返回，那条 `"Allowed ..."` 审计行**根本写不出来**。

### B. claude bridge 60 秒（可执行，需要 claude 全模式可用）

1. 在 Hub Desktop 里跑一个 claude 全模式会话，触发一次工具审批。
2. 掐表等 65 秒不点。
3. 点「允许」。
4. 观察到：卡片消失，会话时间线新增一行 `Allowed '<tool>' ... for <requester>`；同时 claude 侧这个工具收到的是 `{ behavior: "deny", message: "Permission timeout" }`，从未执行。
5. 期望：这一次点击返回错误（大意是「这条审批已在引擎侧超时」），审计要么不写，要么写成 `Timed out（用户的允许迟到了）`。

### C. 为什么用户一定会撞上（可执行的算术）

`crates/alva-hub-desktop/src/permissions.rs:290-337` 的分组只合并**等价**请求；三个并发会话各自弹卡时，用户逐张处理。bridge 的窗口是 60 秒（`bridge/index.mjs:78`），Hub 侧 stall 看门狗是 180 秒（`crates/alva-hub-desktop/src/run.rs:25` `STALL_WINDOW`），引擎侧是 300 秒（`crates/alva-agent-security/src/middleware/security.rs:122`）。三个数字互不知情。只要用户在第三张卡上花掉一分钟——一次去查文件内容的往返——第一张卡就已经在 claude 侧过期了，而它看起来和刚弹出来时一模一样。

## 工作

- `crates/alva-engine-adapter-alva/src/adapter.rs:384-385`：接住返回值——

  ```rust
  if !agent.resolve_permission(request_id, "hub", decision).await {
      return Err(RuntimeError::PermissionNotFound(request_id.into()));
  }
  Ok(())
  ```

  `RuntimeError::PermissionNotFound` 已存在于 `alva-engine-runtime`（见 `crates/alva-engine-runtime/src/AGENTS.md` 的 `RuntimeError` 八变体清单），无需新增错误类型。
- `crates/alva-hub-desktop/src/main.rs:438-455` 与 `:499-518`：把审计文案与「引擎确实收下了」绑定。`respond` 返回 `Err` 时不写审计（`:437` 的 `?` 已经做到）；分组路径要确认 `crates/alva-hub-desktop/src/permissions.rs:391` 的 `answered` 只在真正送达时 push——修好 adapter 之后这一点自动成立，但需要一条测试钉住。
- 把引擎侧超时暴露给 Hub，让审批卡自己失效：
  - `crates/alva-agent-security/src/middleware/security.rs` 增一个 `approval_timeout()` 读取器；
  - `RuntimeEvent::PermissionRequest` 或 `RuntimeCapabilities` 上带出这个期限（`crates/alva-engine-runtime/src/event.rs`），让 `crates/alva-hub-desktop/src/permissions.rs` 的 pending 条目带上 deadline，到点自行从 `groups()` 里消失并在时间线上留一条 `Timed out`。
  - 注意口径统一：目前 60s（claude bridge）/ 180s（Hub stall）/ 300s（security middleware）三套，最终应由引擎侧上报，Hub 不再自己猜。
- `crates/alva-engine-adapter-claude/bridge/index.mjs:36-46`：`else` 分支要回发一条控制帧（如 `{"type":"permission_response_rejected","request_id":...,"reason":"expired"}`），让 Rust 侧能把它翻成 `PermissionNotFound`。相应地 `crates/alva-engine-adapter-claude/src/adapter.rs:515-536` 的 `respond_permission` 不能再以「写成功」为终点，需要等一条语义 ack。
- `crates/alva-engine-adapter-claude/bridge/index.mjs:72-78`：60 秒这个魔数要么提到配置里由 Rust 侧下发，要么直接去掉（改由 `signal` 的 abort 路径 `:65-71` 统一收口）。60 秒短于引擎侧 300 秒是当前最容易撞上的一档。

## 验收

- [ ] 新增单测 `crates/alva-engine-adapter-alva/src/adapter.rs::respond_permission_after_engine_timeout_reports_not_found`（复现 A，用 `with_approval_timeout(20ms)`）：断言返回 `Err(RuntimeError::PermissionNotFound(_))`。
- [ ] 新增单测 `crates/alva-hub-desktop/src/permissions.rs`：engine 的 `respond_permission` 返回 `Err` 时，`respond_many` 的 `outcome.answered` 为空、`outcome.failures` 非空。
- [ ] 新增单测 `crates/alva-hub-desktop`：审批被引擎拒收时，`record_permission_decision` **未被调用**——用一个记账 store 断言 `"permission_decision"` 类型的时间线行数为 0。
- [ ] `rg -n 'resolve_permission' crates/alva-engine-adapter-alva/src/adapter.rs` 命中行处于 `if !` 或 `match` 条件中，而不是独立语句（人工确认返回值已被消费）。
- [ ] `crates/alva-engine-adapter-claude/bridge/index.mjs` 中 `pendingPermissions.get(...)` 的 `if (resolve)` 有对应 `else` 分支，且该分支调用 `emit(...)`（`grep -n 'else' crates/alva-engine-adapter-claude/bridge/index.mjs` 在 36–46 行区间有命中）。
- [ ] `grep -rn '60_000' crates/alva-engine-adapter-claude/bridge/index.mjs` 无命中（魔数已移除或改为下发参数）。
- [ ] `cargo test -p alva-engine-adapter-alva -p alva-engine-adapter-claude -p alva-hub-desktop` 全绿。
- [ ] 手工验证复现 B：等 65 秒后点「允许」，界面给出明确的「该审批已过期」提示，会话时间线上没有 `Allowed` 这一行。
