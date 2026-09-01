# 12 引擎中途死掉后未闭合的工具卡片永远停在 running（契约算出来了，处置端是空的）

- 级别：P1
- 影响：工具执行中引擎断线或超时后，界面上那张工具卡片永久显示"正在运行"，而会话本身已经失败或已经换到另一个引擎；刷新、重开应用都恢复不了，因为这个"运行中"状态已经落盘。这恰好是自动 fallback 产品最常见的触发时刻。
- 状态：TODO

## 证据

### 契约已经把这个信息算出来了

- `crates/alva-engine-runtime/src/stream_contract.rs:146-155` —— `SoftStreamWarning::UnclosedTool { id }`：注释就是 "`ToolStart` without matching `ToolEnd` when the stream finished."
- `crates/alva-engine-runtime/src/stream_contract.rs:340-352` —— `soft_warnings()` 遍历 `self.open_tools` 逐个产出 `UnclosedTool { id }`，`id` 是完整的 tool call id。信息是齐的。

### 处置端是空的

- `grep -rn 'ToolEnd' crates/alva-hub-core/src/` —— **零命中**。全仓没有任何地方合成 `ToolEnd`。
- `crates/alva-hub-core/src/runtime.rs:558-587` —— `contract.finish()` 的 `Ok` 分支拿到 `warnings`，只做两件事：`record_soft_warnings(&sessions, ..., soft_count)`（`:563`，只记一个计数）和逐条 `debug!`（`:565-572`）。`warning` 本身除了被格式化进 log 之外没有任何消费者。
- 更糟的是 `finish()` 的 `Err` 分支（`:574-587`）**根本不调 `soft_warnings()`**。而 `crates/alva-engine-runtime/src/stream_contract.rs:323-337` 里 `finish()` 在 `awaiting_completed_after_fatal`（发了 fatal Error 但没跟 `Completed`）或 `!self.completed` 时都返回 `Err`。也就是说：引擎干脆断流的场景连"算出来"这一步都没发生。
- `crates/alva-hub-core/src/runtime.rs:597-620` —— 兜底合成只补 `Error{recoverable:false}` + `Completed` 两个事件，不补任何 `ToolEnd`。

### 契约状态每轮清零，而 product session 跨轮跨引擎

- `crates/alva-hub-core/src/runtime.rs:514` —— `let mut contract = StreamContract::new();` 在每次 `execute` 的 spawned task 里新建，生命周期 = 单条引擎流。
- `crates/alva-hub-core/src/runtime.rs:152-156` —— `reset_stream_diagnostics()` 把 `stream_hard_violations` / `stream_soft_warnings` / `last_stream_violation` 清零。
- 调用点：`crates/alva-hub-core/src/runtime.rs:386`（`switch_engine` 尾部）和 `:472`（`execute` 里对已有 session 的分支，注释 "New execute turn: diagnostics describe the stream about to run."）。
- 于是 A 引擎那条流的诊断在 B 引擎开跑前就被清掉了，跨引擎的未闭合工具没有任何跨轮承载体。

### 前端的工具卡片状态怎么来的（后果的落点）

- `crates/alva-hub-desktop/src/run.rs:829-869` —— `RuntimeEvent::ToolStart` 分支发 `TimelineStatus::ActivityStarted`，`activity.succeeded = None`（`:844`），并 `store.record_side_effect(...)` 落盘一条 `succeeded: None` 的记录（`:850-869`，`succeeded: None` 在 `:857`）。
- `crates/alva-hub-desktop/src/run.rs:871-897` —— `RuntimeEvent::ToolEnd` 分支才发 `ActivityCompleted` 并把 `succeeded` 写成 `Some(bool)`（落盘处 `:908`）。
- `crates/alva-hub-desktop/web/src/App.tsx:316` —— 前端从库里重建时间线时：`status: effect.succeeded === null ? "activity_started" : "activity_completed"`。
- `crates/alva-hub-desktop/web/src/App.tsx:728-729` —— `activity_started` 渲染成 `status.usingTool`（"正在使用工具 …"）。
- 合起来：没有 `ToolEnd` ⇒ `succeeded` 永远是 `None` ⇒ 卡片永远是 `activity_started` ⇒ **重开应用也一样**，因为它是从落盘记录重建的。

### 顺带：`stream_hard_violations` 在两个主力 GUI 产品里零消费

- `grep -rn 'stream_hard_violations\|stream_soft_warnings\|last_stream_violation' crates/alva-hub-desktop/` —— 0 行；`crates/alva-hub-gpui/` —— 0 行。
- 全仓唯一消费点是 `crates/alva-hub-cli/src/main.rs:867-883`（打到 stderr，且 hard violation 会置 `failed = true`）。
- `crates/alva-hub-core/src/runtime.rs:110-116` —— `HubSessionSnapshot` 已经带着这三个字段，数据是现成的，只差透到 `crates/alva-hub-desktop/web/src/bridge.ts`。
- 剩下的只有 `tracing::warn!` / `debug!`（`crates/alva-hub-core/src/runtime.rs:516-524`、`:565-572`、`:581-586`、`:589-595`），而桌面产品通常不挂 subscriber —— 适配器回归在主力产品上完全不可观测。

## 复现

### A. 工具执行中引擎死掉（可执行）

1. 用一个能在工具执行中被杀掉的引擎跑一轮：让 agent 调用一个长时间运行的工具（例如 `execute_shell` 跑 `sleep 300`）。
2. 在工具运行期间 `kill -9` 掉引擎子进程（codex / claude CLI / pi 的 child）。
3. 观察到：时间线出现失败卡片（`alva_hub_core::SYNTHETIC_TERMINAL_MESSAGE`，合成分支见 `crates/alva-hub-core/src/runtime.rs` 的 `if !saw_completed`（工单 02 已把措辞提成常量，旧字面量不再存在）），但那张工具卡片仍显示"正在使用工具 …"。
4. 关掉 desktop 再打开，进入同一会话。观察到：工具卡片**仍然**是"正在运行"（因为 `record_side_effect` 落的是 `succeeded: None`）。
5. 期望：引擎流终止时，未闭合的工具应被标记为失败并给出原因。

### B. fallback 场景（可执行，A 的加强版）

1. 配置好至少两个授权引擎（policy 里 A、B 都可自动使用）。
2. 让 A 在工具执行中卡住 180 秒触发 stall（`crates/alva-hub-desktop/src/run.rs:25,641-671`）。
3. 观察到：自动切到 B 重跑；A 那张工具卡片仍停在"正在运行"，而会话已经在 B 上继续。
4. 期望：切引擎前 A 的未闭合工具已被闭合。

### C. 诊断在 GUI 上不可观测（静态可证）

1. `grep -rn 'stream_hard_violations' crates/alva-hub-desktop/ crates/alva-hub-gpui/` → 0 行。
2. 唯一消费点 `crates/alva-hub-cli/src/main.rs:867-883` 只在 CLI。
3. 期望：`HubSessionSnapshot` 上已有的三个字段能透到桌面前端（哪怕只在 Inspector 里）。

## 工作

- `crates/alva-hub-core/src/runtime.rs:558-587`：在 `contract.finish()` 的**两个**分支上统一取 `contract.soft_warnings()`（`Err` 分支目前完全不取，需要一并修）。对每个 `SoftStreamWarning::UnclosedTool { id }`，在发 `Completed` 之前合成并推送：

  ```rust
  RuntimeEvent::ToolEnd {
      id,
      name: /* 由 contract 记录的 ToolStart name，见下条 */,
      result: ToolOutput::error("engine ended with tool still running"),
      duration_ms: None,
  }
  ```

  `ToolOutput::error` 已存在（`crates/alva-llm-wire/src/tool_payload.rs:119`，与 `::text` 同一个 `impl`），产出的值 `is_error == true`，正是 `crates/alva-hub-desktop/src/run.rs:878` 判定 `succeeded` 用的字段。合成事件要走和真实事件同一条 `event_tx.send(HubEvent { .. })` 路径（`:544-556`），这样 `update_session` 与产品层投影都能看到。
- `crates/alva-engine-runtime/src/stream_contract.rs:146-155`：`SoftStreamWarning::UnclosedTool` 目前只带 `id`，而 `RuntimeEvent::ToolEnd`（`crates/alva-engine-runtime/src/event.rs:62-67`）需要 `name`——该处文档本身就写着 "Adapters must maintain a `HashMap<tool_use_id, tool_name>` to populate `name`"。给 `UnclosedTool` 补上 `name`（`StreamContract` 在 `observe(ToolStart)` 时已经能拿到），或在 hub-core 侧自己维护 id→name 映射。**契约的硬规则不需要改**，只是把已有的软告警补全。
- `crates/alva-hub-core/src/runtime.rs:597-620`：兜底合成 `Error` + `Completed` 的顺序里，`ToolEnd` 必须排在 `Completed` 之前。
- 顺带（可拆成独立小改动）：把 `HubSessionSnapshot`（`crates/alva-hub-core/src/runtime.rs:110-116`）的 `stream_hard_violations` / `stream_soft_warnings` / `last_stream_violation` 透到 `crates/alva-hub-desktop/web/src/bridge.ts`，在 Inspector（`crates/alva-hub-desktop/web/src/components/engine-inspector.tsx`）里显示，让适配器回归在 GUI 上可见。

## 验收

- [ ] `cargo test -p alva-hub-core` 新增 `unclosed_tool_is_closed_before_completed_on_clean_finish`：mock 引擎发 `SessionStarted` → `ToolStart{id:"t1"}` → `Error{recoverable:false}` → `Completed`，断言 Hub 输出流里在 `Completed` 之前出现 `ToolEnd{id:"t1"}` 且 `result.is_error == true`。
- [ ] `cargo test -p alva-hub-core` 新增 `unclosed_tool_is_closed_when_stream_dies_without_completed`：mock 引擎发 `SessionStarted` → `ToolStart{id:"t1"}` 后直接结束流（`finish()` 走 `Err` 分支），断言合成序列为 `ToolEnd{id:"t1"}` → `Error` → `Completed`（当前 `Err` 分支连 `soft_warnings()` 都不调，此测试改造前必然失败）。
- [ ] `cargo test -p alva-hub-core` 新增断言：多个未闭合工具时每个 id 各得一条 `ToolEnd`，且不会给已闭合的工具重复合成。
- [ ] `cargo test -p alva-hub-desktop` 新增 `dead_engine_marks_side_effect_failed`：跑完上述场景后从 store 读回该 attempt 的 side effect，`succeeded == Some(false)`（而非 `None`）——这条直接钉住"重开应用后卡片不再是 running"。
- [ ] `grep -rn 'ToolEnd' crates/alva-hub-core/src/` 有命中（当前为 0）。
- [ ] 手工：按「复现 A」执行，第 3、4 步工具卡片显示为失败并带原因文案。
- [ ] （顺带项）`grep -rn 'streamHardViolations\|stream_hard_violations' crates/alva-hub-desktop/web/src` 有命中，且 Inspector 中可肉眼看到该计数。
