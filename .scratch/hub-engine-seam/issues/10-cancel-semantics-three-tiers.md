# 10 cancel 语义是三档而不是一档（Hub 与 UI 当成同一件事）

- 级别：P1
- 影响：用户点「停止」后界面立刻显示"已停止"，但引擎子进程可能仍在跑并继续计费；另有一段窗口期里停止按钮直接报错，卡住 180 秒后自动换引擎重跑，而第一个引擎的进程没有任何人再去杀。
- 状态：TODO

## 前提：契约本身没有定义 cancel 的语义

`crates/alva-engine-runtime/src/runtime.rs:32-33` —— `EngineRuntime::cancel` 的全部文档是一行 `/// Cancel a running session.`，返回 `Result<(), RuntimeError>`。契约既没说 `Ok(())` 代表「已停止」还是「请求已投递」，也没要求 cancel 之后流上必须出现终止事件。四个 adapter 于是各自实现出三种强度不同的语义，而调用方（Hub、desktop、CLI）统一按「`Ok` = 已停止」处理。

## 证据

### 三档强度（逐个核对过实现）

**① 硬杀进程（真的秒级死）**

- `crates/alva-engine-adapter-codex/src/adapter.rs:211-224` —— `cancel` 从 `sessions` map 取出 `KillSender` 并 `send(())`；`:101-108` 的 `kill_rx.recv()` 分支调 `process.kill()`，然后发 `Error{recoverable:false}` 并 `break`（`:190-206` 尾部补 `Completed`）。
- `crates/alva-engine-adapter-codex/src/process.rs:87-95` —— `kill()` 就是 `self.child.start_kill()`（tokio 的 SIGKILL）。
- `crates/alva-engine-adapter-pi/src/adapter.rs:153-164` 同构；`crates/alva-engine-adapter-pi/src/process.rs:86-94` 同为 `start_kill()`。
- `crates/alva-engine-adapter-claude/src/print_adapter.rs:297-311` 同构；`:142-165` 的 kill 分支调 `cleanup_session`，后者 `:60-71` 末尾 `process.kill().await`，并在同一分支内发 `Error{recoverable:false}` + `Completed`。

**② 进程内 token（协作式，卡在系统调用里的工具不会被打断）**

- `crates/alva-engine-adapter-alva/src/adapter.rs:342-355` —— `cancel` 取出 `session.agent`（`Arc<BaseAgent>`）后只调 `agent.cancel()`；没有进程、没有 kill。
- `crates/alva-app-core/src/base_agent/agent.rs:144-150` —— `BaseAgent::cancel` 只是 `current_cancel.lock().cancel()`，即置一个 `CancellationToken`。工具只有在自己检查 token 时才会停。
- 同一函数 `:348-352`：`agent` 是 `Option`，为 `None` 时返回 `RuntimeError::NotReady`——即 execute 已返回、agent 还没构造好的窗口里，cancel 直接失败。

**③ 纯协作提示（Rust 侧完全没有兜底）**

- `crates/alva-engine-adapter-claude/src/adapter.rs:511-513` —— `cancel` 的全部实现是 `self.send_control(session_id, BridgeOutbound::Cancel).await`。
- `crates/alva-engine-adapter-claude/src/protocol.rs:118-130` —— `BridgeOutbound::Cancel` 序列化为 `{"type":"cancel"}`，写到 Node bridge 的 stdin。
- `crates/alva-engine-adapter-claude/src/adapter.rs:96-125` + `:128-144` —— 所谓 ack **只证明写成功**：`forward_controls`（`:128-144`）在 `control.send(&message).await` 返回 `Ok(())` 后立刻 `acknowledgement.send(Ok(()))`；`send_control` 等的就是这个 oneshot（`CONTROL_ACK_TIMEOUT` = 3s，`:33`）。bridge 是否照做、cc 是否停，Rust 侧一无所知。
- `crates/alva-engine-adapter-claude/bridge/index.mjs:47-48` —— `cancel` 分支只有 `abortController.abort()`；对照 `:49-51` 的 `shutdown` 分支才有 `setTimeout(() => process.exit(0), 1000)`。两个分支的不对称是静态可证的。
- `crates/alva-engine-adapter-claude/src/adapter.rs:217-231` —— `kill_session` 存在，且在 `:304 :319 :348 :384 :419 :488` 六处被调用（错误路径、idle 超时、Done 路径）——**唯独 cancel 路径没有调**。cancel 之后没有任何定时器、没有 kill、没有「若干秒内未见终止事件则升级」。
- `crates/alva-engine-adapter-claude/src/adapter.rs:559-757`（`mod tests`）—— 该模块共 9 个测试（`build_bridge_config_*` 3 个、`capabilities_report_resume_support`、`stale_cleanup_keeps_a_newer_sender_for_the_same_session`、`duplicate_active_session_id_*`、`control_ack_timeout_is_bounded_and_reports_failure`、`cwd_sdk_resolves_*`），**没有任何一个覆盖 cancel 的效果**。对照组：`crates/alva-engine-adapter-claude/src/print_adapter.rs:389` 有 `cancel_emits_one_fatal_error_then_completed`。

**③ 的可达范围（对既有说法的更正）**：全仓只有 `crates/alva-hub-cli/src/main.rs:1298`（`ClaudeMode::Full`）构造 `ClaudeAdapter`。两个 GUI 产品（`crates/alva-hub-desktop/src/alva.rs:99`、`crates/alva-hub-gpui/src/alva.rs:94`）都只用 `ClaudePrintAdapter`，即 ① 档。所以「desktop `--claude-mode full` 点 Stop」这个场景**今天不存在**——③ 档目前只能从 `alva-hub --claude-mode full` 走到，其取消入口是 Ctrl-C / 轮次超时 / 审批超时（`crates/alva-hub-cli/src/main.rs:683,698,744,751,762,769`）。

- `crates/alva-hub-cli/src/main.rs:940-961` —— `cancel_and_drain`：`CANCEL_TIMEOUT`（`:33`，5s）内等 `hub.cancel`，再用 5s 排空事件流，超时就打印 `alva-hub: engine stream did not close after cancellation` 然后返回、进程退出。也就是说 CLI 明确接受「cancel 后流没关」这个结果并照常退出。
- `crates/alva-engine-adapter-claude/src/process.rs:70` —— bridge 子进程带 `kill_on_drop(true)`，进程退出时 tokio 会杀掉直接的 node 子进程；但这只覆盖 node 本身，SDK 再往下 spawn 的 `claude` 孙进程不在其内，且要求 `BridgeProcess` 真被 drop——cancel 之后事件循环仍停在 `process.recv()` 上，没有人 drop 它。
- `crates/alva-engine-adapter-claude/src/idle_watchdog.rs:34-35` —— 兜底的 idle watchdog 默认 30 分钟；但 `:77-80` 的 `waiting_on_upstream()` 要求 `open_tools.is_empty() && pending_permissions.is_empty()`，而 `:122-127` 的 `schedule_idle` 在 `!waiting_on_upstream` 时把 deadline 推到 `far_future_deadline()`（`:117-119`，一年后）。**只要有一个工具没收到 ToolEnd，30 分钟兜底就是被完全解除的**，不是"最迟 30 分钟收尸"。

### 上层把三档当同一件事

- `crates/alva-hub-core/src/runtime.rs:631-645` —— `HubRuntime::cancel` 只检查 `capabilities().cancel`，然后 `engine.cancel(&engine_session_id).await?` 直接返回。返回 `Ok` 不代表引擎停了，Hub 也不等任何终止事件。
- `crates/alva-hub-desktop/src/main.rs:1243-1266` —— Stop 命令：`record_attempt_cancelled`（`:1243-1244`）→ `clear_session` → `engine.cancel(...)`（`:1248-1251`）→ `app.emit("hub_timeline", TimelineStatus::Interrupted)`（`:1252-1264`）。整条链上没有等待流的终止事件；只要 `cancel()` 返回 `Ok`，用户就看到"已停止"。

### 同源第二条：SessionStarted 之前存在结构性不可取消窗口

- `crates/alva-hub-core/src/runtime.rs:640-642` —— `HubRuntime::cancel` 硬依赖 `session.engine_session_id`，为 `None` 时返回 `HubError::SessionNotReady`。
- `crates/alva-hub-core/src/runtime.rs:695-700` —— 该字段只在收到 `RuntimeEvent::SessionStarted` 时才写。
- `crates/alva-hub-desktop/src/main.rs:1238-1241` —— desktop 走的是自己的库存记录：`detail.session.engine_session_id.ok_or("The engine has not started a native session yet; try again in a moment.")`。
- `crates/alva-hub-desktop/src/run.rs:748-775` —— 该记录由 `store.record_engine_session`（`:767`）写入，同样只在 `RuntimeEvent::SessionStarted` 分支里。
- 后果链（静态可证）：`crates/alva-hub-desktop/src/run.rs:25`（`STALL_WINDOW = 180s`）→ `:641-671` 的 stall 分支把失败记为 `FailureClass::Stall`（`:661`），然后 best-effort 调 `hub.cancel`（`:664-670`）；若 `SessionStarted` 从未到达，这里必然拿到 `SessionNotReady`，代码只 `tracing::warn!` 然后 `break`。
- `crates/alva-hub-desktop/src/failure.rs:45-47` —— `triggers_fallback(Stall)` 为真，于是 `crates/alva-hub-desktop/src/run.rs:271` 的 `while let Some(class) = execution.fallback_class()` 进入换引擎重跑。
- 第一个引擎为什么不会自己死：desktop 只是 `break` 出循环、丢掉 `stream`，但 `stream` 是 Hub 的输出流，不是引擎流。`crates/alva-hub-core/src/runtime.rs:543-556` 明确写了消费者掉线后「Keep draining the engine stream even when the product consumer drops it」——只把 `output_open` 置 false，仍继续 `engine_stream.next().await`。所以 adapter 侧的 `event_tx` 永远不会关闭，`crates/alva-engine-adapter-claude/src/print_adapter.rs:132-141` 那个 `_ = event_tx.closed()` 自清理分支永远不触发。第一个引擎的子进程活到自己跑完为止。

## 复现

### A. 不可取消窗口 + 换引擎后旧进程存活（可执行）

1. 用一个启动很慢或首个 `SessionStarted` 迟迟不来的引擎目标发起一轮（例：把 codex 可执行文件替换成一个 `sleep 600` 的脚本，或用一个 base_url 指向黑洞地址的 provider）。
2. 在 desktop 里对该会话点「停止」。
3. 观察到：报错 `The engine has not started a native session yet; try again in a moment.`（`main.rs:1241`）。
4. 继续等待 180 秒。观察到：时间线出现 `engine stalled: no activity for 180s`，随后自动切到下一个授权引擎重跑。
5. 此时 `ps` 查看：第一个引擎的子进程仍在运行。
6. 期望：停止按钮在 execute 返回后立刻可用；换引擎之前第一个引擎必须被终止。

### B. ③ 档 cancel 只是提示（静态可证 + 半可执行）

1. 静态链：`adapter.rs:511-513` → `protocol.rs:125-126` → `forward_controls`（`adapter.rs:128-144`，写成功即 ack）→ `bridge/index.mjs:47-48`（只 abort，无 exit，对照 `:49-51`）→ Rust 侧无任何后续动作（`kill_session` 六处调用点均不含 cancel 路径）。
2. 半可执行：`alva-hub --claude-mode full` 跑一轮长任务，在工具执行中按 Ctrl-C。观察 stderr 是否出现 `alva-hub: engine stream did not close after cancellation`（`main.rs:959`），并用 `ps` 检查 node/claude 子进程。
3. 期望：cancel 之后有界时间内流上必须出现 `Error{recoverable:false}` + `Completed`，且子进程树被回收。

## 工作

- `crates/alva-engine-runtime/src/runtime.rs:32-33`：把 `cancel` 的文档契约写死——`Ok(())` 只表示「取消请求已投递」；实现方在投递后**必须**保证流上产出 `Error{recoverable:false}` 后跟 `Completed`，否则视为违约。同时在 `crates/alva-engine-runtime/src/stream_contract.rs` 侧补一条可断言的规则或测试辅助。
- `crates/alva-hub-core/src/runtime.rs:631-645`：`HubRuntime::cancel` 改为投递后等待该会话流上的终止事件（带上限超时），或至少把「已投递」与「已终止」拆成两个可观测状态供上层使用。
- `crates/alva-hub-desktop/src/main.rs:1243-1266`：Stop 不再在 `cancel()` 返回 `Ok` 时立即推 `TimelineStatus::Interrupted`；改为在收到终止事件后才落 `Interrupted`，投递成功先落一个"正在停止"态。
- `crates/alva-engine-adapter-claude/src/adapter.rs:511-513`：cancel 后启动 grace 计时（建议 5s），未见 `Done`/`Result` 就调用已有的 `kill_session`（`:217-231`）。顺带把 `bridge/index.mjs:47-48` 的 cancel 分支与 `:49-51` 的 shutdown 分支语义对齐或注释清楚。
- `crates/alva-hub-core/src/runtime.rs:640-642` 的窗口：让 `execute` 返回时就能路由 cancel。codex/pi 已经把 `provisional_id` 注册进 map（`crates/alva-engine-adapter-codex/src/adapter.rs:71-75`、`crates/alva-engine-adapter-pi/src/adapter.rs:57-61`），但这两处都在 `tokio::spawn` 出来的 task 里，**与 `execute` 返回并不同步**——需要把注册提到 `execute` 的同步段，并把这个 id 通过返回值/Hub session 交给上层，让 `cancel` 在 `SessionStarted` 之前也能命中。
- `crates/alva-hub-desktop/src/run.rs:663-670`：stall 分支的 cancel 失败不能只 `warn!` 就换引擎；`SessionNotReady` 必须走一条能真正终止旧引擎的路径（依赖上一条的 provisional handle），否则不允许进入 fallback。

## 验收

- [ ] `cargo test -p alva-engine-adapter-claude` 新增 `cancel_kills_bridge_when_no_terminal_event_within_grace`：用 fake bridge 脚本（收到 `{"type":"cancel"}` 后故意不退出、不发 Done），断言 grace 超时后子进程已退出且流上出现 `Error{recoverable:false}` + `Completed`。
- [ ] `cargo test -p alva-engine-adapter-claude` 新增测试断言 cancel 后 3 秒内流上必然出现终止事件对（与 `print_adapter.rs:389` 的 `cancel_emits_one_fatal_error_then_completed` 对齐，四个 adapter 各一条同名测试）。
- [ ] `cargo test -p alva-hub-core` 新增 `cancel_before_session_started_is_routable`：mock 引擎在 `execute` 返回后延迟发 `SessionStarted`，在延迟窗口内调 `HubRuntime::cancel` 返回 `Ok` 而非 `SessionNotReady`。
- [ ] `cargo test -p alva-hub-desktop` 新增 `stall_cancel_failure_blocks_fallback`：mock 引擎从不发 `SessionStarted`，断言 180s stall 后**不会**进入换引擎分支，或进入前记录了一次成功的终止动作。
- [ ] `cargo test -p alva-hub-desktop` 新增 `stop_marks_interrupted_only_after_terminal_events`：mock 引擎 `cancel()` 返回 `Ok` 但延迟发终止事件，断言 `TimelineStatus::Interrupted` 在终止事件之后才发出。
- [ ] `grep -n 'kill_session' crates/alva-engine-adapter-claude/src/adapter.rs` 的调用点中包含 cancel 路径（当前六处均不在 cancel 路径）。
- [ ] 手工：按「复现 A」执行，第 5 步 `ps` 中第一个引擎的子进程已不存在。
