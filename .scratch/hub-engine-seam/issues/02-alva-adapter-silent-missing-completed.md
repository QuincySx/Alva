# 02 alva adapter 两条静默早退路径漏发 Completed，把进程内 bug 伪装成引擎故障并触发付费 fallback

- 级别：P0
- 影响：本地 Alva agent 自己崩了，界面却说「引擎连接中断」，并自动把这一轮换到 Claude / Codex 重跑一遍——用户为一次本地 panic 付了另一家的账。
- 状态：DONE（2026-09-01，分支 `fix/engine-seam-tickets`）

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

## 完成记录

**adapter 侧（主修复）**
- 路径 1（session map 回写落空）：改成先在锁内判定、释放锁后再发 `Error{recoverable:false}` + `Completed`。用 flag 而不是在持锁状态下发事件——当前 `event_tx` 是 unbounded 不会阻塞，但没必要把这个前提焊进代码。按工单所述未补 `remove`（key 已不在 map 里）。
- 路径 2（agent 事件通道提前关闭）：`None` 分支在 `break` 前补同一对事件，用 `if output_open` 与主循环内既有写法保持一致。收尾的 `flush()` 与状态清理未动。
- 收尾逻辑未改：两条路径都在收尾之前 `return` / `break`，落点与既有 5 处正确早退一致。

**hub-core 侧（纵深防御）**
- 合成兜底的消息提成 `pub const SYNTHETIC_TERMINAL_MESSAGE`，措辞改为 `"engine produced no terminal event (adapter contract violation)"`——逐条比对过 `failure.rs` 全部 9 张关键词表均不命中（注意 `contract` ≠ `context`，`produced no terminal` ≠ `no progress`），落到 `FailureClass::Other`，`triggers_fallback == false`。**适配器契约违规不再花用户的钱。**
- 采用工单的方案（a）而非（b）：不新增 `terminal_synthesized` 字段。理由是主修复已在 adapter 侧堵住源头，hub-core 这层是第二道防线，(b) 要改 `HubSession` + snapshot + hub-desktop 消费三处，收益不抵 scope。
- `unwrap_or_default()` 的空串保留（确实没有 native session，是诚实值，下游 `!is_empty()` 守卫正确处理），但补了一条 `tracing::warn!`，不再静默。

**测试**
- `emits_terminal_pair_when_session_is_deleted_while_starting`：`GatedFactory` 在 `build()` 内阻塞，测试从 `adapter.sessions` 读出自铸 id 后 `delete_session`，再放行。
- `emits_terminal_pair_when_agent_channel_closes_early`：`PanickingModel`（`LanguageModel` 仅 3 个必需方法）让 run task panic → sender drop → `recv()` 返回 None。临时换 panic hook 避免污染测试输出。
- 双重变异验证均 CAUGHT：路径 1 退回静默 `return` → `got []`（**精确复现工单描述的「长度 0」症状**）；路径 2 退回静默 `break` → "expected a fatal Error before Completed"。

**顺带修掉一个脆弱测试**
`run::tests::treats_stream_without_terminal_event_as_failed`（run.rs:1455）原本硬编码合成消息字符串，改消息后立刻变红。已改为绑定 `alva_hub_core::SYNTHETIC_TERMINAL_MESSAGE`——它本来就属于本轮要消除的那类字符串耦合，现在措辞漂移会编译期/断言层面被抓到，而不是静默失效。

验证：`cargo test -p alva-engine-adapter-alva -p alva-hub-core -p alva-hub-desktop` = 21 / 22 / 182 全绿。

## Review 修正（三 lens 并行审查后）

review 抓到 1 个 blocking + 3 个 notable，全部已修：

1. **【blocking】漏改姊妹拷贝 `alva-hub-gpui`。** `crates/alva-hub-gpui/src/run.rs` 是 hub-desktop `run.rs` 的整份拷贝，同样硬编码旧合成消息，改措辞后立刻变红；而 CI 跑的是 `cargo test --workspace --exclude alva-app-tauri`，**不排除 gpui**，推上去 Test job 会挂、Coverage（needs: test）连跑都不跑。已绑常量修复，`cargo test -p alva-hub-gpui --lib` = 161 全绿。
   > 讽刺的是这正是本批工单 README 里命名的那个模式——「每一次字符串比较都会被按前端复制粘贴一遍」——我自己踩了一次。
2. **【notable】路径 1 的消息内插 UUID，仍会触发付费 fallback。** `failure.rs::has_http_code` 只要求状态码左右非数字，而 UUID 的十六进制串里出现被字母/连字符包围的 `401`/`403`/`429` 完全可能 → 分类成 Auth/RateLimit → `triggers_fallback == true`。两个 lens 独立做了 20 万次 uuid4 模拟，命中率 0.5%–0.77%（约 1/150）。修法：消息提成 `pub const SESSION_REMOVED_WHILE_STARTING` / `AGENT_STOPPED_PRODUCING_EVENTS`（不含 id），session id 改走 `tracing::warn!` 结构化字段。
3. **【notable】两个新测试都存在空转风险。** 断言只看事件形状不看消息，而 `factory.build` 失败路径发的是**完全同形**的 Error+Completed 对。review 用双重变异实测：bug 回归 + build 恰好失败 → 测试照样绿。已改为钉死消息内容（绑常量），并补 `assert_eq!(map.len(), 1)` 把「只有一个 session」这个隐含前提显式化、补 `SessionStarted` 断言区分「开跑后断流」与「启动即失败」。
4. **【notable】全局 panic hook 交换有害且不必要。** `#[tokio::test]` 是 current_thread runtime，libtest 本就按测试捕获 panic；而 `set_hook` 是进程级状态，libtest 默认并行跑同一 binary 的 21 个测试，窗口内别的测试失败会**丢掉全部诊断信息**（review 做了对照实验证实），且中途 panic 时 hook 永不还原。已删除。
5. 顺带：desktop 与 gpui **两份** `failure.rs` 现在都有护栏测试，覆盖全部三条契约违规消息（此前 gpui 那份完全没有）；矩阵测试里的旧措辞行加了注释说明它是「刻意迁走的历史文案」；工单 12 引用的旧字符串已更新。

**修正后重做变异验证，三条全部 CAUGHT**：
- 常量措辞漂进 Transport 关键词表 → desktop 护栏红（`got transport failure`）
- 同一漂移 → **gpui 护栏也红**（这份此前零保护）
- 路径 1 退回静默 return → adapter 测试红（`got []`）

**一处认知订正**：commit `bb8aa17` 的 message 说把 run.rs 测试绑常量后「措辞漂移会被断言抓到」——review 实测这是**夸大**的。run.rs 那条绑常量后守的是「hub 真的用了这个常量而非另写字面量」，真正的措辞护栏唯一落在 `failure.rs` 的分类断言上。设计本身合理，描述不准确。

验证：`cargo test` = adapter-alva 21 / hub-core 22 / hub-desktop 182 / hub-gpui 161 全绿。clippy 对 adapter-alva 与 hub-core 零 warning（4 条落在 `alva-app-core/src/session_projection.rs`，来自 8fb5357 那批累积工作，非本次引入）。
