# 01 codex / pi adapter 在 fatal Error 之后不停止读取循环

- 级别：P0
- 影响：一次运行已经在时间线上标成「失败」之后，助手还在继续往同一条时间线里追加正文和工具活动，用户看到一个「失败了却还在干活」的会话，无法判断哪些操作真的执行了。
- 状态：DONE（2026-09-01，分支 `fix/engine-seam-01-fatal-break`）

## 证据

### 契约原文（R2）

- `crates/alva-engine-runtime/src/stream_contract.rs:44-45` —— Hard rules 第 2 条原文：
  `2. **Non-recoverable `Error` must be followed by `Completed`**. After a fatal Error, **only** `Completed` may still appear (no tools/messages).`
  （偏差说明：任务里说这条写在「各自 AGENTS.md 的 R2」。实测两个 adapter crate 的 `src/AGENTS.md` 里都没有 `R2` 这个编号；R2 的正式出处是上面这行 rustdoc，`crates/alva-engine-runtime/src/AGENTS.md` 的「约束」段以中文复述了同一条：「fatal `Error` 后**只能**再发 `Completed`（中间不得夹 Message/Tool）」。`crates/alva-engine-adapter-codex/src/AGENTS.md` 的「约束」段也声明自己遵守 stream contract。）
- `crates/alva-engine-runtime/src/stream_contract.rs:173-181` —— `pub fn is_fatal_error(event: &RuntimeEvent) -> bool`，匹配 `RuntimeEvent::Error { recoverable: false, .. }`。
- `crates/alva-engine-runtime/src/lib.rs:21` —— `is_fatal_error` 已经从 crate 根重导出。
- `rg -n 'is_fatal_error' crates/ --glob '*.rs'` 全仓只有 4 处命中：定义处 `stream_contract.rs:173`、导出处 `lib.rs:21`、以及它自己的单测 `stream_contract.rs:691` / `:695`。**生产代码零调用者**。

### codex adapter

- `crates/alva-engine-adapter-codex/src/adapter.rs:129-168` —— `for runtime_event in mapper.map(event)` 内部只对 `Error { recoverable: true }` 做 retry 升级判断（145-165），fatal 分支不存在。
- `crates/alva-engine-adapter-codex/src/adapter.rs:167` —— `let _ = event_tx.send(runtime_event);` 无条件发送，发完继续下一条；外层 `loop`（`:99`）只在 `escalated`（`:169`）、EOF（`:173`）、读错误（`:174-179`）、kill（`:101-109`）时才 break。fatal Error 不在其中。
- `crates/alva-engine-adapter-codex/src/mapping.rs:59-71` —— `CodexEvent::TurnFailed` 置 `self.failed = true` 并返回 `Error { recoverable: false }`；注释自称「TurnFailed is always terminal」，但 terminal 只体现在 `failed` 标志上，没有让 adapter 停。
- `crates/alva-engine-adapter-codex/src/mapping.rs:77-92` —— `CodexEvent::Error` 走 `is_recoverable_codex_error(&message)`（定义见 `:226-242`，14 个网络类关键词的白名单），不匹配即 `recoverable: false`，同样只置标志。
- `crates/alva-engine-adapter-codex/src/adapter.rs:189-203` —— 循环结束后的统一收尾：取 `session_id`，fatal 时 `result = None`（`:194-198`），发唯一 `Completed`（`:199-203`）。**收尾逻辑本身是对的，缺的只是提前跳到这里。**

### pi adapter

- `crates/alva-engine-adapter-pi/src/adapter.rs:107-118` —— 同形状的 `for runtime_event in mapper.map(event)`，`:117` 无条件 `event_tx.send`，且连 codex 的 escalation break 都没有。外层 `loop`（`:81`）只在 kill（`:91`）、EOF（`:120`）、读错误（`:126`）时 break。
- `crates/alva-engine-adapter-pi/src/adapter.rs:132-145` —— 收尾同样已经统一补 `Completed`（`:141-145`），fatal 时 `result = None`（`:136-140`）。
- pi 有三条独立的 fatal 产出点：
  - `crates/alva-engine-adapter-pi/src/mapping.rs:104-107`（`auto_retry_end` success=false，`recoverable: false` 在 `:106`）
  - `crates/alva-engine-adapter-pi/src/mapping.rs:134-139`（`assistantMessageEvent.type == "error"`，`recoverable: false` 在 `:138`）
  - `crates/alva-engine-adapter-pi/src/mapping.rs:157-162`（`message_end` 的 `stopReason` ∈ {`error`,`aborted`}，`recoverable: false` 在 `:161`）
- 后两条在**一次正常失败的输出里必然连续出现**：`crates/alva-engine-adapter-pi/src/mapping.rs:55-60` 把 `message_update` 派给 `map_delta`、`message_end` 派给 `map_message_end`，两者都会置 `failed` 并各发一条 fatal。wire tag 见 `crates/alva-engine-adapter-pi/src/protocol.rs:9`（`#[serde(tag = "type")]`）、`:20-26`、`:79`、`:127-128`。

### 后果链

- `crates/alva-hub-core/src/runtime.rs:516-533` —— Hub 对每条引擎事件调 `contract.observe(&event)`，违规只计数 + 告警，事件仍然继续转发（`:543-556` 无条件 forward）。
- `crates/alva-engine-runtime/src/stream_contract.rs:244-249` —— fatal 之后任何非 `Completed` 事件立即返回 `EventAfterFatalError`。
- `crates/alva-engine-runtime/src/stream_contract.rs:296-315` —— `awaiting_completed_after_fatal` 只在 `:298` 置 true，只在 `:315`（收到 `Completed`）清 false。**中间任何事件都不会清它**，所以 fatal 之后每一条事件都记一次违规，`stream_hard_violations`（`crates/alva-hub-core/src/runtime.rs:734`）随事件数线性增长。
- 用户可见面（**这里修正了任务描述**）：`crates/alva-hub-desktop/src/run.rs:922-940` 的 fatal 分支有 `if terminal_error.is_none()` 去重，所以**第二条 fatal 不会再弹一次 Failed 横幅**。真正漏出来的是别的东西——`crates/alva-hub-desktop/src/run.rs:789-811`（`Message`）、`:812-828`（`MessageDelta`）、`:829-849`（`ToolStart`）**都没有 `terminal_error` 守卫**，无条件 `emit`；而 `crates/alva-hub-desktop/src/run.rs:640-680` 的消费循环只在 `Completed`（`:963` / `:974`）、流结束（`:678`）或 stall 超时（`:672`）才 break，fatal 不 break。于是 Failed 卡片之后，助手正文和 `<tool> running` 活动继续往同一条时间线追加。

## 复现

### A. pi（可执行，纯 mock 二进制，不需要真 pi）

1. 造一个假 `pi`，让它按 pi 正常失败时的输出打三行：

   ```sh
   cat > /tmp/fake-pi <<'EOF'
   #!/bin/sh
   echo '{"type":"session","id":"pi-sess-1"}'
   echo '{"type":"message_start","message":{"role":"assistant"}}'
   echo '{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"role":"assistant","errorMessage":"upstream refused"}}}'
   echo '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"upstream refused"}}'
   EOF
   chmod +x /tmp/fake-pi
   ```

2. 用 `PiAdapterConfig { pi_path: "/tmp/fake-pi".into(), .. }`（字段见 `crates/alva-engine-adapter-pi/src/config.rs:50`）构造 `PiAdapter`，`execute` 一次，收集整条流。
3. 观察到：事件序列是 `SessionStarted` → `Error{recoverable:false}` → `Error{recoverable:false}` → `Completed`。把这条序列喂给 `alva_engine_runtime::validate_event_stream`，得到 `EventAfterFatalError`。
4. 期望：`SessionStarted` → `Error{recoverable:false}` → `Completed`，`validate_event_stream` 返回 `Ok`。

### B. codex（可执行）

1. 造一个假 `codex`，在 `turn.failed` 之后继续输出一条正文：

   ```sh
   cat > /tmp/fake-codex <<'EOF'
   #!/bin/sh
   echo '{"type":"thread.started","thread_id":"cx-1"}'
   echo '{"type":"turn.started"}'
   echo '{"type":"turn.failed","error":{"message":"sandbox denied write"}}'
   echo '{"type":"item.completed","item":{"type":"agent_message","id":"m1","text":"let me try another path"}}'
   EOF
   chmod +x /tmp/fake-codex
   ```

2. 用 `CodexAdapterConfig { codex_path: "/tmp/fake-codex".into(), .. }`（字段见 `crates/alva-engine-adapter-codex/src/config.rs:32`）跑一次 `execute`。
3. 观察到：`Error{recoverable:false}` 之后仍然发出 `Message`（agent_message），再 `Completed`；`validate_event_stream` 报 `EventAfterFatalError`。接到 Hub Desktop 上，时间线是「Failed: sandbox denied write」后面紧跟一条助手气泡「let me try another path」。
4. 期望：fatal 之后只剩 `Completed`，那条 `Message` 不出现。

## 工作

- `crates/alva-engine-adapter-codex/src/adapter.rs`：把 `:99` 的 `loop` 加标签（如 `'read: loop`），在 `:167` 发送前后插入 fatal 判定 —— 先 `event_tx.send(runtime_event)`，再 `if is_fatal_error(&sent) { break 'read; }`。注意判定要在发送后做（fatal 本身必须送达），并且要能穿透 `for` + `tokio::select!` 两层跳到 `:189` 的收尾。`escalated` 那条路径（`:151-164`）已有等价语义，可以顺手统一成同一个 break。
- `crates/alva-engine-adapter-codex/src/adapter.rs`：`use alva_engine_runtime::is_fatal_error;`，不要在 adapter 里重写 `matches!(.., recoverable: false)`——让 `is_fatal_error` 第一次真正进生产代码。
- `crates/alva-engine-adapter-pi/src/adapter.rs`：同样处理 `:81` 的 `loop` 与 `:117` 的发送点。pi 没有 escalation 变量，改动更小。
- 两个 adapter 的收尾（codex `:189-203`、pi `:132-145`）**不需要改**：`mapper.failed()` 在 fatal 时已为 true，`result` 已经是 `None`，`Completed` 已经唯一。break 之后自然落到这里。
- `crates/alva-engine-adapter-codex/src/AGENTS.md` / `crates/alva-engine-adapter-pi/src/AGENTS.md`：在「约束」段补一句「fatal `Error` 后立即停止读取循环，只补 `Completed`」，让 R2 在 crate 层面有落点。
- （可选，同一根因的第二道防线）`crates/alva-engine-runtime/src/stream_contract.rs:244-249`：`EventAfterFatalError` 目前对同一条 fatal 无限次重复计数。可以在首次报出后把 `awaiting_completed_after_fatal` 降级为「已报过」状态，让 `stream_hard_violations` 反映事件种类而不是事件条数。此项独立于主修复，不做也不影响 A/B 复现转绿。

## 验收

- [ ] 新增 `crates/alva-engine-adapter-pi/src/adapter.rs` 单测 `stops_reading_after_fatal_error`：用复现 A 的假 `pi` 脚本（`tempfile` 落盘 + `chmod 0o755`），断言收集到的事件里 `Error { recoverable: false, .. }` **恰好一条**，且最后一条是 `Completed`。
- [ ] 新增 `crates/alva-engine-adapter-codex/src/adapter.rs` 单测 `stops_reading_after_fatal_error`：用复现 B 的假 `codex` 脚本，断言 fatal 之后没有 `RuntimeEvent::Message`，事件总数为 3（`SessionStarted` / `Error` / `Completed`）。
- [ ] 两个测试都追加一句 `assert!(alva_engine_runtime::validate_event_stream(&events).is_ok())`。
- [ ] `rg -n 'is_fatal_error' crates/ --glob '*.rs'` 在 `crates/alva-engine-adapter-codex/src/adapter.rs` 与 `crates/alva-engine-adapter-pi/src/adapter.rs` 各至少一处命中（生产代码，非 `#[cfg(test)]` 块内）。
- [ ] `cargo test -p alva-engine-adapter-codex -p alva-engine-adapter-pi` 全绿。
- [ ] `cargo test -p alva-hub-core -p alva-hub-desktop` 全绿（确认 break 不破坏既有 Hub 侧断言）。
- [ ] `grep -n 'fatal' crates/alva-engine-adapter-codex/src/AGENTS.md crates/alva-engine-adapter-pi/src/AGENTS.md` 各有命中。

## 完成记录

- 两个 adapter 各加 `fatal_seen` flag，沿用 codex 已有的 `escalated` 两层 break 形状（未用 labeled break 穿透 `tokio::select!`，风险更低）。判定用 `alva_engine_runtime::is_fatal_error`，**该函数首次进入生产代码**。
- 判定在 `event_tx.send` **之后**做（fatal 本身必须送达），通过先取 `let fatal = is_fatal_error(&runtime_event);` 规避 send 的 move。
- 收尾未改：`mapper.failed()` 在 fatal 时已为 true，`result` 已是 `None`，`Completed` 仍唯一。
- 两个测试均通过**变异验证**（去掉 break 后必红）：
  - codex：`adapter.rs:302` 的 "no Message may follow a fatal Error" 断言触发。
  - pi：`left: 2, right: 1` —— 直接证实工单所述「一次正常失败必然产出两条 fatal」。
- 未做工单里标注的可选项（`stream_contract.rs` 的 `EventAfterFatalError` 重复计数降级）——它独立于本修复，留给工单 16。

## Review 修正（子 agent review 后）

第一版有两个问题，均已修：

1. **pi 测试有实质漏洞**（review 实测发现）：原断言只查 `fatal_count == 1`，无法区分「break 生效」与「本来就只产出一条 fatal」。把 fixture 的 `message_update` 改名模拟协议漂移（落入 `PiEvent::Unknown`）后，原测试**照样通过**——fatal 仍是 1 条（来自 `message_end`）。
   修法：两行 fixture 用不同错误文案，断言送达的是**第一条**（`vec!["upstream refused"]`）。现在同时钉死「恰好一条」和「是哪一条」。
   重做双重变异验证，两种都必红：
   - 移除 break → `left: ["upstream refused", "must not be forwarded"]`
   - `message_update` 漂移成 Unknown → `left: ["must not be forwarded"]`
2. **codex loop 尾部 `:192` 是死分支**：review 确认该处条件恒为 false（置位点都在 arm 内且紧接着就 break 了）。第一版把它改成 `if escalated || fatal_seen`，等于给既有死代码新增一个死项。已回滚为原样 `if escalated { break; }` 并加注释说明不可达，行为完全等价。

review 同时确认（我已独立复核）：控制流正确、fatal 确实先送达、两层 break 确实落到统一收尾、所有 fatal 发射点无遗漏、`mapper.failed()` 蕴含关系成立、kill 分支不受影响、`.kill_on_drop(true)` 保证提前停读不留孤儿进程。

**顺带发现，已开工单 17**：claude 的两个 adapter 零 `is_fatal_error` 命中，`print_adapter.rs:177` 转发循环与修复前的 codex/pi 同形，疑似同一 bug 类漏网——但 claude CLI 是否在 fatal 后继续输出尚未证实，工单 17 要求先验证再动手。
