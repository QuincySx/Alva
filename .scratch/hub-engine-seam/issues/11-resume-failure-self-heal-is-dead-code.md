# 11 resume 失败的自愈路径在生产里永远不会触发

- 级别：P1
- 影响：Alva 引擎的会话在应用重启（或换用另一个记录库）后可能被永久焊死——每次发消息都拿同一个失效的 native id 去 resume、每次都失败，用户只能新建会话，历史那条对话再也发不出任何一句话。
- 状态：TODO

## 证据

### 自愈机制本身是完整的、且有测试

- `crates/alva-hub-desktop/src/run.rs:239-260` —— `native_resume` 为真且 `terminal_error` 命中判定时，调用 `store.upgrade_failed_native_resume(&prepared, failure_reason)?`，把这一轮从 delta（native resume）降级成 full handoff，然后**用新的 prepared 重跑一次** `build_engine` + `execute_persisted_turn`。逻辑本身写得很讲究。
- `crates/alva-hub-desktop/src/store/turn.rs:561-566` —— `upgrade_failed_native_resume` 存在，且第一行就是 `if !prepared.native_resume { ... }` 的守卫。
- `crates/alva-hub-desktop/src/run.rs:1874` —— 测试 `invalid_native_resume_replaces_delta_with_full_handoff_before_fresh_execution` 覆盖了整条路径并通过。
- `crates/alva-hub-gpui/src/run.rs:546-548` —— gpui 有一份字面相同的实现，同样的问题。

### 触发器是四个字符串子串匹配

`crates/alva-hub-desktop/src/run.rs:562-568`：

```rust
fn is_invalid_native_resume_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("session not found")
        || error.contains("invalid session")
        || error.contains("unknown session")
        || (error.contains("resume") && error.contains("session"))
}
```

### 四个子串与真实 adapter 错误文案的逐条比对

`terminal_error` 的来源有两类：`hub.execute` 返回 `Err`（`crates/alva-hub-desktop/src/run.rs:596-622`，取 `error.to_string()`），或流上的 `RuntimeEvent::Error` 文本（`:925-939`）。逐条比对：

| 真实错误来源 | 文本 | `session not found` | `invalid session` | `unknown session` | `resume`+`session` |
|---|---|---|---|---|---|
| `crates/alva-engine-adapter-alva/src/adapter.rs:154-158` | `Alva session '<id>' is not in this process and no native record store is configured` | 否 | 否 | 否 | 否（无 `resume`） |
| `crates/alva-engine-adapter-alva/src/adapter.rs:174-178` | `Alva session '<id>' not found in memory or native record store` | **否**（是 `session '<id>' not found`，中间隔着 id，字面子串不成立） | 否 | 否 | 否 |
| `crates/alva-engine-adapter-alva/src/adapter.rs:192-195` | `Alva record lookup failed: <err>` | 否 | 否 | 否 | 否 |
| `crates/alva-engine-adapter-alva/src/adapter.rs:215-218` | `Alva record store failed: <err>` | 否 | 否 | 否 | 否 |
| `crates/alva-engine-adapter-codex/src/adapter.rs:80-83`、`:175-178` | `error.to_string()`（`CodexProcess` spawn / IO 错误，`crates/alva-engine-adapter-codex/src/process.rs:87-95` 等） | 否 | 否 | 否 | 否 |
| `crates/alva-engine-adapter-claude/src/print_adapter.rs:253-256` | `Claude CLI stream ended without a result` | 否 | 否 | 否 | 否 |
| `crates/alva-engine-adapter-claude/src/print_adapter.rs:277-280` | `Claude CLI stream read failed` | 否 | 否 | 否 | 否 |
| `crates/alva-hub-core/src/error.rs:18` `HubError::SessionNotFound` | `product session '<id>' was not found` | **否**（`session '<id>' was not found`） | 否 | 否 | 否 |
| `crates/alva-hub-core/src/error.rs:28` `HubError::SessionNotReady` | `product session '<id>' has no engine session yet` | 否 | 否 | 否 | 否 |
| `crates/alva-hub-core/src/error.rs:37` `HubError::ResumeUnsupported` | `engine '<id>' does not support session resume` | 否 | 否 | 否 | **是** |

结论：

- 四个子串里前三个在生产路径上**零命中**。唯一能产出 `Session not found: ...` 的是 `RuntimeError::SessionNotFound` 的 Display（`crates/alva-engine-runtime/src/error.rs:15-16`），但全仓核对过所有 `RuntimeError::SessionNotFound` 的构造点——`crates/alva-engine-adapter-{claude,codex,pi,alva}` 里它只出现在 `cancel` / `respond_permission` / `AlvaAdapter::inspection_session`（`crates/alva-engine-adapter-alva/src/adapter.rs:72,77`）里，**没有任何 adapter 的 `execute()` 会返回它**。而 `execute()` 的 `Err` 才是能走进 `terminal_error` 的那条路。
- 唯一真正会命中的是第四条 `resume` + `session`，命中的是 `HubError::ResumeUnsupported`——那是「引擎压根不支持 resume」的能力错误，不是「native id 失效」。用它触发降级在结果上不算错，但它与本机制要修的故障完全不是一回事，且不会在 Alva 的失效 id 场景下出现。
- 反向证据：唯一能走到自愈的是测试 mock。`crates/alva-hub-desktop/src/run.rs:1139-1168` 的 `ResumeFailureThenFreshEngine` 在 `:1151` 直接 `return Err(RuntimeError::SessionNotFound(...))`，即从 `execute()` 返回 —— 这正是生产 adapter 从不做的事。`crates/alva-hub-desktop/src/store/tests.rs:943` 和 `crates/alva-hub-gpui/src/store.rs:5775` 也是直接把字面量 `"Session not found: expired-native"` 喂给 `upgrade_failed_native_resume`，绕过了触发器。

### 焊死是怎么形成的

- `crates/alva-hub-desktop/src/store/turn.rs:467` —— `let native_resume = engine_session_id.is_some();`，即只要库里存着 native id，下一轮就一定走 native resume。
- 该 id 由 `crates/alva-hub-desktop/src/run.rs:767` 的 `store.record_engine_session` 写入，写入后没有任何路径会在 resume 失败时把它清掉（`upgrade_failed_native_resume` 是唯一的清理动作，而它永远不被触发）。
- 于是每轮都用同一个死 id 重来一次，永远失败。

### 相关：hub-core 的注释在两个 GUI 产品里已经指错方向

- `crates/alva-hub-core/src/runtime.rs:279-282` —— `restore_session` 的文档写着 "Used by products that rehydrate from disk after process restart. The engine must still understand `engine_session_id` (e.g. Claude `--resume`); **in-memory engines like Alva only work if that native session still lives in the same adapter process.**"
- 事实：`crates/alva-hub-desktop/src/alva.rs:307` 在构造 `AlvaAdapter` 时接入 `SessionRepository::open(history_dir.join("alva-agent-records.db"))`，`:309-311` 用 `.with_session_repository(repository)` 挂上。`crates/alva-hub-gpui/src/alva.rs` 同构。
- `crates/alva-engine-adapter-alva/src/adapter.rs:148-196` —— 进程内找不到时会退到 record store 查（`:174-178` 才是「内存和 record store 都没有」的分支）；`crates/alva-engine-adapter-alva/src/adapter.rs:708`（`resumes_from_native_records_after_process_drop`）测试证实了跨进程 resume 可行。
- 所以注释说的「只在同一个 adapter 进程里有效」对配了 repository 的产品是错的，会误导后续维护者以为 Alva 跨重启 resume 本就该失败。

## 次要段：codex 在 thread_id 缺失时伪造 native session id

- `crates/alva-engine-adapter-codex/src/mapping.rs:43-51`：

  ```rust
  CodexEvent::ThreadStarted { thread_id } => {
      let session_id = thread_id.unwrap_or_else(|| Uuid::new_v4().to_string());
      self.session_id = Some(session_id.clone());
      vec![RuntimeEvent::SessionStarted { session_id, model: None, tools: Vec::new() }]
  }
  ```

- `crates/alva-engine-adapter-codex/src/adapter.rs:238-247` —— codex 的 `capabilities().resume == true`。
- `crates/alva-hub-core/src/runtime.rs:695-700` —— Hub 把 `SessionStarted.session_id` 原样记成 `engine_session_id`；`crates/alva-hub-desktop/src/run.rs:767` 也把它落盘。
- `crates/alva-hub-core/src/runtime.rs:442-448` —— 下一轮 execute 看到 `Some(engine_sid)` 且引擎支持 resume，就把它填进 `runtime_request.resume_session`，最终拼成 `codex exec resume <codex 从没见过的 uuid>`。
- 这个伪造的 id 一旦落盘，就和上面的焊死场景合流：resume 必然失败，而自愈永远不触发。

## 复现

### A. Alva 会话焊死（可执行）

1. 在 desktop 里用 alva 引擎开一个会话，发一条消息跑完，确认库里已有 `engine_session_id`。
2. 让那个 native session 失效：删掉 `<history_dir>/alva-agent-records.db`（`crates/alva-hub-desktop/src/alva.rs:307` 指定的路径），或换一个 `history_dir` 启动。
3. 重启 desktop，打开同一个会话，再发一条消息。
4. 观察到：时间线报 `Alva session '<id>' not found in memory or native record store`（`crates/alva-engine-adapter-alva/src/adapter.rs:176`）。再发一条，同样的错误逐字重复；库里的 `engine_session_id` 始终不变。
5. 期望：第一次失败后应自动降级成 full handoff 并重跑成功（这正是 `run.rs:239-260` 想做的事）。

### B. 触发器与真实文案不匹配（静态可证，也可做成单测）

1. 取 `crates/alva-engine-adapter-alva/src/adapter.rs:154-158 / 174-178 / 192-195 / 215-218` 四条文案。
2. 逐条喂给 `crates/alva-hub-desktop/src/run.rs:562-568` 的 `is_invalid_native_resume_error`。
3. 观察到：四条全返回 `false`。
4. 期望：至少「session 在内存和 record store 里都没有」这一条必须返回 `true`（或者根本不该用字符串判定）。

### C. codex 伪造 id（静态可证）

1. `mapping.rs:43-51` 在 `thread_id == None` 时生成随机 uuid 并作为 `SessionStarted.session_id` 发出。
2. `runtime.rs:695-700` / `run.rs:767` 把它当作真实 native id 记录。
3. `runtime.rs:442-448` 下一轮拿它去 resume。
4. 期望：`thread_id` 缺失时不得伪造 native id——要么不发 `SessionStarted`，要么发一个明确标记为「不可 resume」的会话。

## 工作

- `crates/alva-engine-runtime/src/event.rs` / `crates/alva-engine-runtime/src/error.rs`：新增结构化信号（建议 `RuntimeEvent::Error` 旁挂一个 `ResumeRejected` 变体，或 `RuntimeError::ResumeRejected { session_id }`），让「这个 native session 已经不可用」成为契约里的一等公民，而不是靠文案猜。
- `crates/alva-engine-adapter-alva/src/adapter.rs:148-196`：四条 resume 失败路径改发新信号；同时**禁止**在 resume 失败的 `Completed` 上回写那个死 native id。codex / claude-print 的 resume 失败路径同步改造。
- `crates/alva-hub-core/src/runtime.rs`：把该信号透传给产品层，并在收到时清掉 `session.engine_session_id`（当前只有 `switch_engine` 的 `:381` 会清）。
- `crates/alva-hub-desktop/src/run.rs:239-260` 与 `crates/alva-hub-gpui/src/run.rs`（同构段）：改为消费结构化信号触发 `upgrade_failed_native_resume`；`is_invalid_native_resume_error`（`run.rs:562-568` / gpui `:546-548`）删除或降级为兼容期的次要旁路。
- `crates/alva-hub-desktop/src/store/turn.rs`：补一个「忘掉这条 branch 的 native session」的兜底动作，保证即使自愈失败，下一轮也不会再用死 id（把 `:467` 的 `native_resume` 判定接上）。
- `crates/alva-engine-adapter-codex/src/mapping.rs:43-51`：`thread_id` 缺失时不再 `Uuid::new_v4()` 伪造；改为不发 `SessionStarted`，或走一条明确的「本轮无 native session」路径。
- `crates/alva-hub-core/src/runtime.rs:279-282`：把注释改对——说明配置了 `SessionRepository` 的 Alva adapter 支持跨进程 resume（引用 `crates/alva-hub-desktop/src/alva.rs:305-311`），未配置时才退化为进程内。

## 验收

- [ ] `cargo test -p alva-hub-desktop` 新增 `real_alva_resume_failure_text_triggers_upgrade`：把 `crates/alva-engine-adapter-alva/src/adapter.rs:154-158 / 174-178 / 192-195 / 215-218` 四条真实文案逐条作为 mock 引擎的 `RuntimeEvent::Error` 发出，断言四条都走进 `upgrade_failed_native_resume` 并最终 `!outcome.failed`（改造前该测试必须失败）。
- [ ] `cargo test -p alva-engine-adapter-alva` 新增测试：resume 一个不存在的 native id，断言流上出现结构化的 resume-rejected 信号，且 `Completed.session_id` **不等于**请求里那个死 id。
- [ ] `cargo test -p alva-hub-desktop` 新增 `failed_native_resume_clears_stored_engine_session_id`：一轮 resume 失败后，从 store 读回该 branch 的 `engine_session_id` 为 `None`（或 `prepare_resumed_turn` 得到的 `native_resume == false`）。
- [ ] `cargo test -p alva-engine-adapter-codex` 新增 `thread_started_without_id_does_not_fabricate_session_id`：`CodexEvent::ThreadStarted { thread_id: None }` 不产出携带随机 uuid 的 `SessionStarted`。
- [ ] `grep -n 'is_invalid_native_resume_error' crates/alva-hub-desktop/src/run.rs crates/alva-hub-gpui/src/run.rs` 无输出，或输出仅剩兼容期旁路且带注释说明。
- [ ] `sed -n '279,282p' crates/alva-hub-core/src/runtime.rs` 中不再出现 "only work if that native session still lives in the same adapter process"。
- [ ] 手工：按「复现 A」执行，第 4 步会话能正常继续（降级为 full handoff 后拿到回复）。
