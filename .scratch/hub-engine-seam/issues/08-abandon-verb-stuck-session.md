# 08 会话卡在 Starting/Running 无出口（补 abandon 动词与可取消的 turn）

- 级别：P1
- 影响：消费者一旦提前放弃某轮（超时、Ctrl-C、关标签页），这个会话就**永久**不能再用——继续发消息报 "already running"，切引擎也报同样的错，而且此时连取消都调不动，只能重启进程。
- 状态：TODO

## 前提：今天 blast radius 有限，路线 A 落地当天变线上并发 bug

会话卡死要求 `HubRuntime` 实例在这一轮之后还活着。今天两个旗舰产品都不满足：`crates/alva-hub-desktop/src/run.rs:526` 与 `crates/alva-hub-gpui/src/run.rs:508` **每轮**都 `HubRuntime::new()`，卡住的 session map 随函数返回一起被丢弃。全仓唯一长生命周期实例是 `crates/alva-hub-cli/src/main.rs:118`，被 REPL 循环（`:426-583`，`:572` 复用同一个 `hub`）持有——所以卡死在 hub-cli 上是**真实可达**的，只是单用户串行不容易撞。

`docs/specs/hub-headless-service.md:143` 的**路线 A**（gateway 加 hub 路由组、handler 调 `alva-hub-desktop::lib` 纯函数，常驻进程）落地后，"消费者提前放弃"就是最普通的一次 HTTP 连接断开或浏览器刷新，卡死会话会开始堆积且不可回收。

## 证据

### 唯一入口、唯一出口

- `crates/alva-hub-core/src/runtime.rs:470` —— `existing.status = HubSessionStatus::Starting;`。全仓把 session 推入非终态的**唯一**入口（另一处 `:145` 是 `HubSession::new` 的初值，走的是同一个 `execute` 认领路径 `:475-478`）。
- `crates/alva-hub-core/src/runtime.rs:505-626` —— 出口全在 `tokio::spawn` 出来的 task 里：`update_session`（`:542`，靠引擎发 `Completed`）、`mark_session_failed`（`:600`，靠 `while let Some(event) = engine_stream.next().await`（`:516`）**结束**）。两条路都要求引擎流自己终止。
- `crates/alva-hub-core/src/runtime.rs:505` —— `tokio::spawn(async move { ... })` 的 `JoinHandle` 直接丢弃，`HubRuntime` 不持有任何 task 句柄，外部没有任何抓手能中止它。
- 全仓搜索确认无逃生阀：`rg -n 'abandon|reaper|CancellationToken' crates/alva-hub-core/src crates/alva-hub-cli/src` **零命中**。既没有超时、也没有 reaper、也没有强制 reset。
- `crates/alva-hub-core/src/runtime.rs:407-628`（`execute`）与 `:632-645`（`cancel`）是 `HubRuntime` 全部与会话生命周期相关的公开方法；`crates/alva-hub-core/src/lib.rs:11` 导出的类型集合里没有任何释放动词。

### 卡住的会话恰好是最没法取消的那种

- `crates/alva-hub-core/src/runtime.rs:640-642` —— `cancel` 要求 `session.engine_session_id` 存在，否则 `Err(HubError::SessionNotReady(...))`：

  ```rust
  let engine_session_id = session
      .engine_session_id
      .ok_or_else(|| HubError::SessionNotReady(product_session_id.to_string()))?;
  ```
- `crates/alva-hub-core/src/runtime.rs:695-699` —— `engine_session_id` 只在 `RuntimeEvent::SessionStarted` 到达时才被填上。
- 因此：流从没发过 `SessionStarted` 就卡住（引擎起不来、网络挂起、adapter 死在握手阶段）→ `engine_session_id` 恒为 `None` → `cancel` 必然 `SessionNotReady`。**越早卡住的会话越取消不掉。**
- `crates/alva-hub-core/src/error.rs:31-32` —— 用户看到的字面文案是 `product session '{0}' is already running`。

### 现有测试把卡死状态当作正确行为钉住了

- `crates/alva-hub-core/src/runtime.rs:1491-1507` —— `concurrent_product_session_execute_is_rejected_while_starting`：注册 `PendingEngine`（`:875-908`，`execute` 返回 `Box::pin(stream::pending())`），发起第一个 execute 后立刻发起第二个，断言 `Err(HubError::SessionBusy(_))`。测试**只**验证闸门关上，全仓没有任何测试验证这个 claim 还能被释放。
- `crates/alva-hub-core/src/runtime.rs:899-906` —— `PendingEngine` 声明 `cancel: true`，所以上面那条「能力齐备也救不回来」的结论对它同样成立。

### 现有绕法都不是修复

- `crates/alva-hub-cli/src/main.rs:932` —— `let terminate_repl = exit_override.is_some();`，配合 `:421-423`、`:580-582` 直接**退出整个 REPL**。这是弃疗，不是释放会话。
- `crates/alva-hub-desktop/src/run.rs:526`、`crates/alva-hub-gpui/src/run.rs:508` —— 每轮丢弃整个 `HubRuntime`。这是靠不复用来回避问题，路线 A 恰好要复用。
- `crates/alva-hub-desktop/src/run.rs:643-670` —— 产品层自己搭了 `STALL_WINDOW` 超时并在超时后调 `hub.cancel(...)`（`:666-668`）。它对本工单描述的早期卡死无效（`cancel` 会返回 `SessionNotReady`，只在 `:669-670` 打一条 `tracing::warn!` 就算了），而且这套超时逻辑长在产品层、hub-cli 完全没有。

## 复现

`crates/alva-hub-core/Cargo.toml` 的 dev-dependencies 已含 `tokio` 的 `rt-multi-thread` + `macros` 与 `futures`，`PendingEngine`（`runtime.rs:875`）现成，可直接写成 `crates/alva-hub-core/src/runtime.rs` 里的一条 `#[tokio::test]`：

1. `let hub = HubRuntime::new();` 注册 `PendingEngine` 为 `"pending"`。
2. `let stream = hub.execute(HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone())).unwrap();`
3. `drop(stream);` —— 这就是超时 / SIGINT / UI 关标签页那条路径：消费者不再读了。
4. `tokio::time::sleep(Duration::from_millis(100)).await;`
5. 观察到：
   - `hub.session(&product_id).unwrap().status == HubSessionStatus::Starting`
   - 再次 `hub.execute(...)` → `Err(HubError::SessionBusy(_))`，文案 `product session '...' is already running`
   - `hub.switch_engine(&product_id, <任何引擎>)` → 同样 `Err(HubError::SessionBusy(_))`（守卫在 `runtime.rs:367-372`）
   - `hub.cancel(&product_id).await` → `Err(HubError::SessionNotReady(_))`（`runtime.rs:640-642`，流从未发过 `SessionStarted`）
6. 再 sleep 任意时长，重复步骤 5 —— 结果不变，**永久**。
7. 期望：存在某个公开动作能把这个会话放回可用状态（下一次 `execute` 成功认领），且被放弃那一轮的 spawned task 能自行退出而不是永远挂在 `stream::pending()` 上。

补充（`runtime.rs:543-556` 已有的注释说"即使消费者 drop 了也继续 drain 到 Completed"）：那段设计只在引擎流**会**终止时成立；`PendingEngine` 演示的是引擎流不终止的情形，drain 逻辑无从触发。

## 工作

- `crates/alva-hub-core/src/runtime.rs` 新增公开动词：

  ```rust
  pub fn abandon(&self, product_session_id: &ProductSessionId) -> Result<HubSessionSnapshot, HubError>
  ```

  在写锁内把 `status` 置为终态（`Failed`），使下一次 `execute`（闸门在 `:429-434`）与 `switch_engine`（闸门在 `:367-372`）都能重新认领。会话不存在时返回 `HubError::SessionNotFound`。
- `crates/alva-hub-core/src/runtime.rs:407-628`（`execute`）引入 per-turn `tokio_util::sync::CancellationToken`：认领时创建、存进 `HubSession`（新增字段，不进 `HubSessionSnapshot`），`clone` 给 spawned task；`:516` 的 `while let Some(event) = engine_stream.next().await` 改成 `tokio::select!` 包住 `engine_stream.next()` 与 `token.cancelled()`，token 触发时跳出循环走既有的 `:597-620` 合成终端事件路径（消费者若还在读，仍能收到 Error + Completed 而不是静默断流）。`abandon` 负责 `cancel()` 这个 token。
- 与工单 07 的 `claim_seq` 协同：`abandon` 也要 `claim_seq += 1`，让被放弃那一轮的尾部写入（`restore_failed_handoff` / `update_session`）被 seq 守卫挡掉。两张工单若分别落地，后落地的一张负责补这条联动测试。
- `crates/alva-hub-core/src/runtime.rs:632-645`（`cancel`）拆两级：
  1. 能路由到 native session（`engine_session_id` 存在且 `capabilities().cancel`）→ 维持现有行为，转发给引擎；
  2. 路由不了（`engine_session_id` 为 `None`，或引擎不支持 cancel）→ **不再**直接 `Err`，而是释放 Hub 侧的 claim（等价于内部调用 `abandon`），并把「没能通知到引擎」作为返回信息交给调用方（新增一个表达"已本地释放、未送达引擎"的返回值或 `HubError` 变体）。语义要点：产品发起的取消永远至少能让 Hub 侧回到可用状态。
- `crates/alva-hub-core/Cargo.toml` 增加 `tokio-util = { version = "0.7", features = ["rt"] }`（仅为 `CancellationToken`；不引入任何 kernel/agent/app 依赖，遵守 `crates/alva-hub-core/src/AGENTS.md:21` 的依赖约束）。
- `crates/alva-hub-cli/src/main.rs`：把 `:932` 的「卡住就退 REPL」改为先尝试 `abandon`，只有 abandon 也失败才 `terminate_repl`。
- 更新 `crates/alva-hub-core/src/AGENTS.md`：「逻辑」补一条 abandon/两级 cancel，「约束」补一条"非终态必须有产品可达的出口"。

## 验收

- [ ] `cargo test -p alva-hub-core` 与 `cargo test -p alva-hub-cli` 全绿。
- [ ] 新增测试 `abandoned_session_can_be_reclaimed`：照复现步骤 1-5 走到卡死，调用 `hub.abandon(&product_id)` 后，`hub.execute(...)` 返回 `Ok`（不再是 `SessionBusy`）。
- [ ] 新增测试 `abandon_stops_the_spawned_turn`：`PendingEngine` 场景下 `abandon` 后，被放弃那一轮的消费者流在有限时间内产出终端事件并结束（不再永远挂起）。
- [ ] 新增测试 `cancel_releases_claim_when_engine_session_is_unknown`：`engine_session_id` 为 `None` 时 `hub.cancel(...)` **不**返回 `SessionNotReady`，且事后 `hub.execute(...)` 能重新认领。
- [ ] 现有测试 `concurrent_product_session_execute_is_rejected_while_starting`（`runtime.rs:1491-1507`）断言逐字不变且仍通过——闸门本身不能被削弱。
- [ ] 手工验证 hub-cli：启动 REPL，用一个不会返回的引擎发起一轮，Ctrl-C 打断后在同一个 REPL 里继续发消息，能正常开新一轮而不是看到 `already running`。
- [ ] `rg -n 'abandon' crates/alva-hub-core/src crates/alva-hub-cli/src` 有命中（今天为零）。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无新增告警；依赖防火墙检查通过（hub-core 仍只依赖 `alva-engine-runtime`）。
