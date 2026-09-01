# 02 alva adapter 两条静默早退路径漏发 Completed，把进程内 bug 伪装成引擎故障并触发付费 fallback

- 级别：P0
- 影响：本地 Alva agent 自己崩了，界面却说「引擎连接中断」，并自动把这一轮换到 Claude / Codex 重跑一遍——用户为一次本地 panic 付了另一家的账。
- 状态：TODO

## 证据

### 两条静默早退

- `crates/alva-engine-adapter-alva/src/adapter.rs:262-266` —— 拿到 agent 后回写 session map：

  ```rust
  let mut sessions = sessions.lock().expect("Alva adapter session lock poisoned");
  let Some(session) = sessions.get_mut(&session_id) else {
      return;
  };
  ```

  key 已不在时直接 `return`，**一条事件都不发**，`event_tx` 随 task 结束被 drop，流直接 EOF。
- `crates/alva-engine-adapter-alva/src/adapter.rs:306-309` —— 主循环的 `events.recv()` 分支：

  ```rust
  event = events.recv() => {
      let Some(event) = event else {
          break;
      };
  ```

  agent 事件通道关闭（BaseAgent 任务 panic / sender 被 drop）时 `break`，落到 `:327` 的 `flush()` 和 `:329-336` 的状态清理，**中间没有任何 `Completed`**。对比 `:320-322`：正常路径靠 `mapped` 里出现 `Completed` 才 break，那条路径的 `Completed` 是 mapper 给的。

### 并发来源

- `crates/alva-engine-adapter-alva/src/adapter.rs:462-467` —— `delete_session` 第一件事就是 `self.sessions.lock()...remove(native_session_id)`。`execute` 里 `:119-140` 插入 key、`tokio::spawn`（`:147`）之后才在 `:262` 回写，中间隔着 `factory.build(...).await`（`:237`）。这两者之间任意一次 `delete_session`（用户在等待期间删会话）都会让 `:264` 的 `get_mut` 落空。

### 同文件的正确做法（对照组）

同一个 spawn 块里另有 **5 处**早退，每处都是 `Error { recoverable: false }` + `Completed { result: None }` 成对再 `return`（偏差说明：任务描述说「另外 6 处」，实测是 5 处）：

- `crates/alva-engine-adapter-alva/src/adapter.rs:154-169`
- `crates/alva-engine-adapter-alva/src/adapter.rs:174-189`
- `crates/alva-engine-adapter-alva/src/adapter.rs:190-205`
- `crates/alva-engine-adapter-alva/src/adapter.rs:213-228`
- `crates/alva-engine-adapter-alva/src/adapter.rs:242-256`

即：`:265` 和 `:308` 是这个文件里仅有的两条不守规矩的出口。

### 后果链

- `crates/alva-hub-core/src/runtime.rs:597-620` —— 流结束时 `!saw_completed`，Hub 合成兜底：
  - `:599-600` `let engine_session_id = mark_session_failed(&sessions, &output_session_id).unwrap_or_default();`
  - `crates/alva-hub-core/src/runtime.rs:711-721` —— `mark_session_failed` 把 `session.status = HubSessionStatus::Failed`，返回 `session.engine_session_id.clone()`，类型是 `Option<String>`。会话不存在、或从没收到过 `SessionStarted`（路径 1 正是如此）时，`unwrap_or_default()` 得到**空串**。
  - `:605-608` 合成 `Error { message: "engine stream ended without terminal Completed event", recoverable: false }`
  - `:610-618` 合成 `Completed { session_id: <空串>, result: None, usage: None }`
- 空 `session_id` 的下游影响：`crates/alva-hub-desktop/src/run.rs:970-972` 有 `if !session_id.is_empty()` 守卫，所以 `engine_session_id` 保持 `None`；`crates/alva-hub-desktop/src/run.rs:708-711` 又要求非空 `engine_session_id` 才给审批卡挂控件——这一轮之后的任何 `PermissionRequest` 都会变成无控件的死等。
- **付费 fallback**：`crates/alva-hub-desktop/src/run.rs:922-940` 收到合成 fatal，`failure_class = Some(classify_failure(&message))`（`:937`）。`crates/alva-hub-desktop/src/failure.rs:146-161` 的 Transport 关键词表里第 8 个是 `"stream ended"`，合成消息 `"engine stream ended without terminal Completed event"` 小写后**逐字包含**它，且前面所有分支（Auth/RateLimit/Context/ModelUnavailable/EngineMissing/EngineIncompatible/Crash/Stall）都不匹配 → `FailureClass::Transport`。
- `crates/alva-hub-desktop/src/failure.rs:45-47` —— `triggers_fallback(class) = class != FailureClass::Other`，`Transport` 为真。
- `crates/alva-hub-desktop/src/run.rs:272` —— `while let Some(class) = execution.fallback_class()`，fallback 链启动，`:285-286` 取下一个被授权的引擎目标重跑本轮。ADR 依据见 `docs/adr/0002-reversible-multi-engine-fallback.md`。

一句话：进程内 `BaseAgent` panic → adapter 静默闭嘴 → Hub 合成一条**看起来像网络问题**的错误 → 分类成 Transport → 自动换引擎重跑。

## 复现

### 路径 1（可执行）

1. 在 `crates/alva-engine-adapter-alva/src/adapter.rs` 的测试模块里，用一个 factory，其 `build()` 内部先 `tokio::task::yield_now().await` 再返回 agent（把 `:237` 的 await 点变成可控的让出）。
2. 调 `adapter.execute(request)` 拿到 stream，**不消费**；随即在同一 runtime 上 `adapter.delete_session(&session_id).await`。
3. `while let Some(e) = stream.next().await` 收集。
4. 观察到：收集到的 `Vec<RuntimeEvent>` 为空（长度 0）。
5. 期望：长度 2，`[Error { recoverable: false, .. }, Completed { session_id: <非空>, result: None, .. }]`。

### 路径 2（可执行）

1. 用一个模型 provider，让它在第一次 `prompt_text` 之后 panic（或直接 drop `BaseAgent` 的事件 sender），使 `:306` 的 `events.recv()` 返回 `None`。现成参照：`crates/alva-engine-adapter-alva/src/adapter.rs:536` 起的测试模块里已有构造 BaseAgent 的脚手架。
2. `adapter.execute(request)` 并消费整条流。
3. 观察到：流里没有 `Completed`；把它喂给 `alva_engine_runtime::validate_event_stream` 得 `MissingCompleted`。
4. 期望：最后一条是 `Error { recoverable: false }` + `Completed`，`validate_event_stream` 返回 `Ok`。

### 端到端（静态可证 + 可选手工验证）

上面任一路径接到 Hub Desktop：`crates/alva-hub-core/src/runtime.rs:597` 合成 → `classify_failure("engine stream ended without terminal completed event")` 命中 `crates/alva-hub-desktop/src/failure.rs:153` 的 `"stream ended"` → `Transport` → `crates/alva-hub-desktop/src/run.rs:272` 的 fallback 循环启动。手工验证：在 Hub Desktop 里给会话授权 codex 作为 fallback 目标，触发路径 2，观察时间线是否出现第二个引擎的 attempt。

## 工作

- `crates/alva-engine-adapter-alva/src/adapter.rs:262-266`：`else` 分支改成与 `:154-169` 同形——先 `event_tx.send(RuntimeEvent::Error { message: format!("Alva session '{session_id}' was removed while it was starting"), recoverable: false })`，再 `event_tx.send(RuntimeEvent::Completed { session_id: session_id.clone(), result: None, usage: None })`，然后 `return`。注意此处 key 已经不在 map 里，不要再补 `remove`。
- `crates/alva-engine-adapter-alva/src/adapter.rs:306-309`：`None` 分支在 `break` 之前补同一对事件，message 要点明是引擎侧提前收摊（例如 `"Alva agent stopped producing events before the turn completed"`），不要复用 Hub 的措辞。`break` 之后 `:327` 的 `flush()` 与 `:329-336` 的清理保持不变。
- `crates/alva-engine-adapter-alva/src/AGENTS.md`：在「约束」段写死「`execute` 的**每一条**出口都必须发 `Error{recoverable:false}` + `Completed`，静默 `return` / `break` 一律禁止」。
- `crates/alva-hub-core/src/runtime.rs:605-608`：给合成的兜底错误一个可区分的标记，让下游能分辨「引擎根本没说话」和「引擎明确拒绝服务」。两个可选做法，选一个即可：
  - 改 message 措辞，避开 Transport 关键词表（`crates/alva-hub-desktop/src/failure.rs:146-161`），例如 `"the engine produced no terminal event (adapter contract violation)"`；
  - 或在 `HubSession` 上新增一个 `terminal_synthesized: bool`，由 `crates/alva-hub-desktop/src/run.rs` 读取后在 `classify_failure` 之前短路成 `FailureClass::Other`。
  后者更彻底：`triggers_fallback(Other) == false`（`crates/alva-hub-desktop/src/failure.rs:46`），适配器契约违规就再也不会花用户的钱。
- `crates/alva-hub-core/src/runtime.rs:599-600`：`unwrap_or_default()` 产出空串这件事需要显式处理——要么保留空串但在 `crates/alva-hub-desktop/src/run.rs:970` 之外补一条日志，要么让合成的 `Completed.session_id` 回退到 `output_session_id`。与上一条一起决定。

## 验收

- [ ] 新增单测 `crates/alva-engine-adapter-alva/src/adapter.rs::emits_terminal_pair_when_session_is_deleted_while_starting`（复现路径 1）：断言事件数为 2，第一条 `Error { recoverable: false, .. }`，第二条 `Completed`。
- [ ] 新增单测 `crates/alva-engine-adapter-alva/src/adapter.rs::emits_terminal_pair_when_agent_channel_closes_early`（复现路径 2）：断言最后两条是 `Error { recoverable: false }` + `Completed`。
- [ ] 两个测试都断言 `alva_engine_runtime::validate_event_stream(&events).is_ok()`。
- [ ] `rg -n 'return;|break;' crates/alva-engine-adapter-alva/src/adapter.rs` 在第 95–340 行范围内的每一处出口，向上 15 行内都能 grep 到 `RuntimeEvent::Completed`（人工逐条核对，7 处全覆盖）。
- [ ] 新增 `crates/alva-hub-desktop/src/failure.rs` 单测：断言 `classify_failure(<Hub 合成兜底的最终 message 文本>)` 返回的 class 满足 `triggers_fallback(class) == false`（或按所选方案，断言合成路径不会进入 `crates/alva-hub-desktop/src/run.rs:272` 的 fallback 循环）。
- [ ] `cargo test -p alva-engine-adapter-alva -p alva-hub-core -p alva-hub-desktop` 全绿。
- [ ] `grep -n 'Completed' crates/alva-engine-adapter-alva/src/AGENTS.md` 有命中。
