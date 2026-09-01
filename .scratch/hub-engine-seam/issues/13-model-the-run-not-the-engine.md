# 13 给「一次运行」建模，而不是给 engine 建模

- 级别：P2
- 影响：用户点 Stop 或回答权限弹窗时，Hub 内核根本没有能停下那一次运行的句柄；三个产品各自手抄了一张表来补这个洞，三份清理纪律已经分叉，任何一份漏删都会留下一个停不掉的后台进程。
- 状态：TODO

## 结论先行

**真正没有生命周期的不是 engine，是「一次运行」。给 `HubRuntime` 加一个 `unregister_engine` 修不了任何东西，反而会造出僵尸运行。**

## 证据

### A. adapter 没有可释放的资源，drop 停不了任何东西

- `rg -n 'impl Drop' crates/alva-engine-adapter-*/ crates/alva-hub-*/ --glob '*.rs'` —— 全仓只有一处命中：`crates/alva-engine-adapter-claude/src/adapter.rs:585` 的 `impl Drop for EnvironmentRestore`，而该文件的 `#[cfg(test)]` 在 `:549`，所以这唯一一个 `Drop` 是测试夹具，**5 个生产 adapter 零 `Drop` 实现**。
- 四个 adapter 的结构体只装配置和一张 kill 通道表：
  - `crates/alva-engine-adapter-pi/src/adapter.rs:28-31` —— `PiAdapter { config, sessions: Arc<Mutex<HashMap<String, KillSender>>> }`
  - `crates/alva-engine-adapter-codex/src/adapter.rs:40-43` —— 同形状
  - `crates/alva-engine-adapter-claude/src/print_adapter.rs:77-80` —— 同形状
  - `crates/alva-engine-adapter-alva/src/adapter.rs:36-40` —— `AlvaAdapter { factory, sessions, repository }`
- 子进程真正活在每次 `execute` 派生的分离 task 里：`crates/alva-engine-adapter-pi/src/adapter.rs:57` 的 `tokio::spawn`，`Child` 是该 task 的栈上局部；`crates/alva-engine-adapter-codex/src/process.rs:41` 的 `.kill_on_drop(true)`（`claude/src/process.rs:70`、`claude/src/print_process.rs:47`、`pi/src/process.rs:48` 同）**只在那个 task 自己结束时才触发**。
- `sessions` 是 `Arc`，在 `crates/alva-engine-adapter-pi/src/adapter.rs:54` 被 clone，随后随闭包移进 spawn 出去的 task 并在 `:58-61` 使用。

推导链：drop 最后一个 `Arc<dyn EngineRuntime>` 只析构 config 结构体；streaming task 不受影响、kill 通道表因 task 内的 `Arc` 仍然存活、子进程继续跑。**停止零进程，也不释放任何有意义的资源。**

### B. HubRuntime 对已经跑起来的 run 没有任何句柄

- `crates/alva-hub-core/src/runtime.rs:483` —— `let engine_stream = match engine.execute(runtime_request)`，`engine` 是 `execute()` 内的局部 `Arc` clone（来自 `:672-680` 的 `fn engine()`）。
- `crates/alva-hub-core/src/runtime.rs:505` —— `tokio::spawn(async move { ... })`，返回的 `JoinHandle` 被直接丢弃，**没有 `AbortHandle`**。
- 该 `async move` 闭包捕获的是 `sessions` / `engine_id` / `output_session_id` / `event_tx` / `engine_stream` / `failed_handoff_rollback`（见 `:497-506`），**没有捕获 `engine`**。所以 `execute()` 一返回，那个 `Arc<dyn EngineRuntime>` 的强引用就消失了，运行期间没有任何东西替这次 run 持有引擎。
- 这不是 adapter 作弊：`crates/alva-engine-runtime/src/AGENTS.md` 的约束段明写「`execute()` 返回的 Stream ... 生命周期为 `'static`，不借用 `&self`」。**数据面已经和实例寿命解耦了 —— 缺的是控制面的句柄。**

### C. 控制面挂在 `&self` 上、按 id 反查 registry

- `crates/alva-hub-core/src/runtime.rs:632-644` —— `cancel()`：先 `self.session(id)`，再 `self.engine(&session.engine_id)?`。
- `crates/alva-hub-core/src/runtime.rs:649-670` —— `respond_permission()`：同样先反查 session，再反查 engine。
- `crates/alva-hub-core/src/runtime.rs:672-680` —— `fn engine()`：registry 里查不到就返回 `HubError::EngineNotFound`。

### D. 反证：朴素的 `unregister_engine` 会造出僵尸运行

`rg -n 'unregister' crates/ --glob '*.rs'` 在 Hub 层零命中，今天没有这个方法。若按直觉加 `unregister_engine(id)`（从 `self.engines` 移除条目）：

- 正在跑的 run **不会停**：证据 B 已证明 stream 是 `'static`、spawn task 不持 `engine`、registry 与那次运行之间没有因果关系。
- 但 `cancel()` 会在 `:637` 的 `self.engine(...)` 处立刻返回 `EngineNotFound`，`respond_permission()` 在 `:658` 同样失败。

结果是一个**停不掉、也批不了权限的僵尸运行**。这是纯静态可证的推导。

### E. 三个产品各自手抄了影子表来补这个洞

- `crates/alva-hub-desktop/src/main.rs:43` —— `active_engines: Mutex<HashMap<String, Arc<dyn EngineRuntime>>>`，注释自陈「Entries live only for the duration of a run」。
- `crates/alva-hub-gpui/src/ui/app.rs:68` —— `pub active_engines: Arc<Mutex<HashMap<String, Arc<dyn EngineRuntime>>>>`，注释写着「React/Tauri `active_engines`」，明说是抄的。
- `crates/alva-hub-server/src/main.rs:47-49` —— 同形状。

三份的键都是 **product session id**，不是 engine id。这正是「一次运行」的身份。

- 手写的 insert / drain：`crates/alva-hub-desktop/src/main.rs:272-277`（insert，同时另存一份 `build_registry` id 列表）与 `:391-395`（`drain(..)` 后逐个 `remove`）；另有 `:697-702` 与 `:719-726` 两处独立 remove 路径。
- Stop 与权限回填完全绕过 `HubRuntime`：
  - `crates/alva-hub-desktop/src/main.rs:1231-1251` —— 从 `active_engines` 取 adapter，直接 `engine.cancel(&engine_session_id)`。
  - `crates/alva-hub-gpui/src/ui/actions.rs:129` —— `engine.cancel(&session_id)`；`crates/alva-hub-gpui/src/ui/team_actions.rs:255` 同。
  - `crates/alva-hub-server/src/main.rs:559,587` —— 同。
  - `crates/alva-hub-desktop/src/permissions.rs:355-360` —— `PermissionRouter::respond` 直接 `pending.engine.respond_permission(...)`。
- `HubRuntime::cancel` 的生产调用者只剩两类：`crates/alva-hub-cli/src/main.rs:945`，以及 desktop / gpui 的 stall watchdog（`crates/alva-hub-desktop/src/run.rs:665-671`、`crates/alva-hub-gpui/src/run.rs:639-644`）—— 都不是用户按下的 Stop。`HubRuntime::respond_permission` 的生产调用者**只有** `crates/alva-hub-cli/src/main.rs:757`。

### F. HubRuntime 今天只活一轮，掩盖了后果

- `crates/alva-hub-desktop/src/run.rs:137-139` 与 `:526-528` —— `let hub = HubRuntime::new();` 是函数局部变量，随即 `register_engine` 一个引擎。
- `crates/alva-hub-gpui/src/run.rs:126-128` 与 `:508-510` —— 同。

每轮新建、每轮丢弃，所以「registry 与 run 生命周期缠在一起」今天不炸。常驻进程一旦落地（hub-server 方向）它立刻变成线上 bug。

## 复现

**静态可证**：证据 D 的推导链已完整（B 证明 spawn task 不持 engine 引用、stream 不借 `&self`；C 证明 cancel 路径必过 registry 反查）。若要经验确认，最短路径：

1. 在 `crates/alva-hub-core/src/runtime.rs` 的 `impl HubRuntime` 里临时加：
   ```rust
   pub fn unregister_engine(&self, id: &EngineId) {
       self.engines.write().unwrap().remove(id);
   }
   ```
2. 在 `#[cfg(test)]` 段（`:785` 之后）写一条测试：注册 `PendingEngine`（`:878` 已有，它的 stream 永不终结）→ `execute()` 拿到 stream → `unregister_engine` → `cancel(&product_id).await`。
3. 观察到：`cancel` 返回 `Err(HubError::EngineNotFound)`；同时 stream 仍在被 poll，`PendingEngine` 派生的 task 从未收到任何停止信号。
4. 期望：`cancel` 命中这次运行本身并把它停掉；注销引擎只应阻止**新**路由，不应让既有运行失去控制面。

## 工作

### 第一批 —— 在 hub-core 引入 `RunHandle`（不改任何产品）

- `crates/alva-hub-core/src/runtime.rs`：新增
  ```rust
  pub struct RunHandle {
      abort: tokio::task::AbortHandle,
      engine: Arc<dyn EngineRuntime>,
      engine_id: EngineId,
      engine_session_id: Arc<RwLock<Option<String>>>,
  }
  ```
  `engine_session_id` 用共享槽，是因为它要等 `SessionStarted` 到达才知道；`update_session`（`:682` 起）已经在维护同一信息。
- 把 `:505` 的 `tokio::spawn(...)` 改为持有 `JoinHandle`，取其 `abort_handle()` 存进 `RunHandle`。
- `HubRuntime` 新增字段 `runs: RwLock<HashMap<ProductSessionId, RunHandle>>`；`execute()` 在 spawn 后写入，spawn task 收尾处（`:625` 附近，合成 `Completed` 之后）移除。
- `execute` 改为返回 `(HubEventStream, RunHandle)`，或先保留旧签名并新增 `execute_with_handle`，避免一次性打断 `hub-cli` / desktop / gpui 四个调用点。

### 第二批 —— 控制面改从 `runs` 取

- `crates/alva-hub-core/src/runtime.rs:632-644` 的 `cancel()`：先查 `runs`，命中就用 handle 里的 `engine` 强引用 + `engine_session_id`；查不到才回落到今天的 session/registry 反查（保留 `SessionNotFound` / `SessionNotReady` 语义）。
- `:649-670` 的 `respond_permission()` 同样改造。
- 关键性质：**handle 持强引用，所以一次运行能否被控制不再取决于 registry 里还有没有那个条目。**

### 第三批 —— `abandon`（与工单 08 合流）

- 落 08 的 `abandon` 动词：`abort()` 掉 stream task + 对 engine 发 `cancel` + 把 session 投影到终态。08 是本工单的最小前置，因为在它之前 `RunHandle::abort` 没有消费者。

### 第四批 —— 三份影子表收进 hub-core

- 删 `crates/alva-hub-desktop/src/main.rs:43` 的 `active_engines` 字段，以及 `:272-277` / `:391-395` / `:697-702` / `:719-726` 的手写 insert/remove；`stop_hub_attempt`（`:1231-1251`）改调 `HubRuntime::cancel`。
- 同样处理 `crates/alva-hub-gpui/src/ui/app.rs:68`（配套 `ui/actions.rs:129`、`ui/team_actions.rs:255`、`ui/pump.rs:32`、`ui/app.rs:1729`）。
- 同样处理 `crates/alva-hub-server/src/main.rs:47-49`（配套 `:559`、`:751-752`、`:774-780`、`:799`、`:820-826`、`:905`）。
- `PermissionRouter`（`crates/alva-hub-desktop/src/permissions.rs:339-361`）改为向 `HubRuntime::respond_permission` 转交，不再自持 `Arc<dyn EngineRuntime>`。
- **产品层从此没有手写 insert/remove 的机会** —— 这是清理纪律分叉（工单 05）的结构性解法。

### 第五批 —— 这才谈得上 `unregister`

- `unregister_engine(id)` 的正确语义 = 从目录移除新路由 + 等现存 run 自然收敛（drain）：registry 条目先标记 draining → 新 `execute` 拒绝 → `runs` 中该 engine 的条目清空后才真正移除。这正是 Cordis 的 UNLOADING 态在做的事。
- 改写 `crates/alva-hub-core/src/runtime.rs:236-239` 的 `register_engine` 文档注释：现有理由（「existing product sessions carry native session IDs owned by the originally registered instance」）在有了 `RunHandle` 之后不再成立；新理由是路由歧义。

## 验收

- [ ] `rg -n 'struct RunHandle' -A 8 crates/alva-hub-core/src/runtime.rs` 能看到含 `AbortHandle` 与 `Arc<dyn EngineRuntime>` 的定义。
- [ ] 新测试 `cancel_reaches_a_running_run_after_engine_is_unregistered`：注册一个记录调用的 engine（照 `runtime.rs:1130` 的 `FakeEngine` 写法）→ `execute` → `unregister_engine` → `cancel` 返回 `Ok`，且 engine 记录到了这次 cancel。
- [ ] 新测试 `abandon_aborts_the_stream_task`：`execute` 一个永不终结的 engine → `abandon` → 对应 `JoinHandle` 在 200ms 内 `is_finished()`。
- [ ] `rg -n 'active_engines' crates/alva-hub-desktop/src crates/alva-hub-gpui/src crates/alva-hub-server/src --glob '*.rs'` 输出为空。
- [ ] `rg -n '\.cancel\(' crates/alva-hub-desktop/src/main.rs crates/alva-hub-gpui/src/ui/actions.rs crates/alva-hub-server/src/main.rs` 中不再出现对 `Arc<dyn EngineRuntime>` 的直接调用，只剩 `HubRuntime::cancel`。
- [ ] `cargo test -p alva-hub-core -p alva-hub-desktop -p alva-hub-gpui -p alva-hub-cli` 全绿。
- [ ] `bash scripts/ci-check-deps.sh` 全绿（hub-core 仍只依赖 `alva-engine-runtime`）。
- [ ] 真机：在 Tauri 里发起一轮长跑，点 Stop，`ps aux | rg 'codex|claude|pi'` 中对应子进程在 3s 内消失。

## 依赖与切分

- **前置**：工单 08（`abandon` 动词）是最小前置 —— 没有它，`RunHandle::abort` 没有消费者，第三批无从落地。
- **平行**：工单 14 可与本单并行；14 决定 `switch_engine` 的去留，不触碰 `runs` 表。
- **后置**：工单 15 的 `EngineDescriptor` 应排在本单第二批之后，否则 descriptor 会被塞进一个即将改签名的 `register_engine`。
- **切分点**：第一批（RunHandle，纯新增、零产品改动）→ 第二批（控制面改路，hub-core 内闭环）→ 第三批（abandon）→ 第四批（desktop / gpui / server 各一个独立 commit）→ 第五批（unregister/drain，可无限期缓办）。**第一到第三批合起来已让 `HubRuntime` 自洽，第四批哪怕只做 desktop 一个产品也是净收益。**
