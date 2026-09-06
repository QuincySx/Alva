# 17 claude 两个 adapter 疑似同一 R2 漏网（已验证：不成立）

- 级别：P0（~~若验证成立~~ 验证结论：**不成立，关闭**）
- 影响：~~与 01 相同~~ 无法构成：两个 claude adapter 的每个 fatal Error 都与其 Completed 在同一个 mapper 批次里产出，且两个读取循环都在 Completed 上 break。
- 状态：**CLOSED 2026-09-06（验证结论：非 bug + 防御性 pin 测试）**

## 验证结论（2026-09-06）

逐条核对工单 01 的四个待验证项：

1. **claude CLI 在 fatal 之后是否继续输出？** 无关紧要——结构上不可能泄漏：
   `mapping.rs` 的全部生产 fatal 产出点（`BridgeMessage::Error` :88、`SdkMessage::Result` is_error :319、empty-response :324）都把 `Error{recoverable:false}` 与 `Completed` **成对**推入同一个 `map()` 返回的 Vec；`:496/:800` 是测试段。`claude -p` 与 full 的读取循环都在 `if is_completed` 分支 `return`（print `:215`、full `:422`）。fatal 之后不可能再有 Message/工具事件被转发。
2. `mapping.rs` 5 处 fatal 产出点逐一核对：:88（生产，成对）、:319（生产，成对）、:324（生产，成对）、:496/:800（`mod tests`）。无不成对的生产路径。
3. 全模式 `ClaudeAdapter` 与 print 模式读取循环形状不同，但都在 Completed 上 break；bridge `error` 帧（`main().catch` → `emit("error")` → `process.exit(1)`）同样成对且 EOF。
4. bridge 在 abort/error 后是否仍推事件——被 Rust 侧 `kill_rx` / grace 杀（工单 10 落地），且 bridge error 帧自带 Exit。

新增防御性测试 `no_reader_resumes_after_a_fatal_result_frame`：fake CLI 在 result-error 帧之后继续输出一个 assistant 帧，断言该 Message **不**被转发（现状即通过，pin 住 R2 不泄漏）。

结论：**claude 与 codex/pi 不同，fatal 一定成对 Completed，读取循环成对 break——本工单不成立，关闭。** 未来若有人改动 mapper 让 fatal 不成对产出，应同步给两个 claude adapter 的读取循环补 `is_fatal_error` 早停（形如 codex/pi 的 `fatal_seen`），并跑 `no_reader_resumes_after_a_fatal_result_frame`。

## 来源

工单 01 的子 agent review 越界发现。本工单只记录**已核实的静态事实**与**尚未验证的行为假设**，不预设结论。

## 已核实

- `crates/alva-engine-adapter-claude/src/print_adapter.rs:177` 的 `for runtime_event in mapper.map(...)` 转发循环，与 codex/pi 修复前**结构同形**：转发后无任何 R2 停读判定。
- `rg -c 'is_fatal_error' crates/alva-engine-adapter-claude/src/*.rs` → **零命中**。两个 claude adapter 都没有接入契约的 fatal 判定函数。
- `crates/alva-engine-adapter-claude/src/mapping.rs` 有 5 处 `recoverable: false` 产出点：`:88` / `:319` / `:324` / `:496` / `:800`（需逐个区分生产路径与 `mod tests`，01 就踩过这个——codex 的 `:376` 是测试断言不是生产产出点）。
- 工单 01 已确认 `alva-engine-adapter-pi` 与 `alva-engine-adapter-codex` 是仅有的两个使用 `is_fatal_error` 的 crate。

## 尚未验证（动手前必须先做）

1. **claude CLI 在 fatal 之后到底会不会继续输出？** 这是与 codex/pi 的关键差异：`codex exec --json` 明确会在失败轮之后继续打印 item，pi 的一次正常失败必然连发两条 fatal（工单 01 已用变异测试证实）。而 `claude -p --output-format stream-json` 可能在 `result` 消息后直接 EOF——**若如此，本工单不成立，关掉即可**。
2. `mapping.rs` 5 处 fatal 产出点里，哪些在生产路径？各自之后 CLI/bridge 是否还有后续输出？
3. 全模式 `ClaudeAdapter`（Node bridge，`adapter.rs`）与 print 模式是否都受影响？两者的读取循环形状不同，需分别判断。
4. bridge 侧（`bridge/index.mjs`）在 abort/error 之后是否仍会推事件——它**不退出**（见工单 10 的证据），所以全模式的窗口可能比 print 模式更大。

## 工作

- 先做上面 4 项验证；不成立就关掉本工单并在此记录结论。
- 若成立：照工单 01 的形状修——`fatal_seen` flag + 转发后判定 + break 到统一收尾，判定一律用 `alva_engine_runtime::is_fatal_error`，不在 crate 内重写。
- 注意 claude 的收尾形状与 codex/pi **不同**：它是「每个出口 return 并就地发 Completed」（`adapter.rs:356` / `:444` / `:466` / `:486`），没有统一收尾块。照搬 01 的两层 break 未必合适，需先确认 break 后落到哪里。
- 同步更新 `crates/alva-engine-adapter-claude/src/AGENTS.md` 的约束段。

## 验收

- [ ] 验证结论写进本工单（无论成立与否）。
- [ ] 若成立：新增 `stops_reading_after_fatal_error` 测试，**必须做双重变异验证**（移除 break 必红 + 关键 fixture 行漂移成 Unknown 必红）——工单 01 的 pi 测试初版就栽在第二种上，只断言数量无法区分「break 生效」与「本来就只有一条 fatal」。
- [ ] 若成立：`rg -n 'is_fatal_error' crates/alva-engine-adapter-claude/src` 有生产命中。
- [ ] `cargo test -p alva-engine-adapter-claude` 全绿。
