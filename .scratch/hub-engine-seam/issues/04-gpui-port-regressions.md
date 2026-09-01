# 04 GPUI 移植走样：Interrupt 按钮保证无效 + Pi 加不进降级链

- 级别：P0
- 影响：GPUI 版 Team 面板的「Interrupt」按钮点了等于没点——worker 继续烧钱跑，界面却刷新成已处理；同时 Pi 引擎在降级链面板里根本不出现，用户配不出带 Pi 的 fallback。
- 状态：DONE（2026-09-01，分支 `fix/engine-seam-tickets`）

两个缺陷同源：desktop 那一份写对了，GPUI 移植时抄错，且 GPUI 侧没有测试兜底，所以一直没人发现。

## 证据

### (a) `interrupt_worker` 传空串当 session id

- `crates/alva-hub-gpui/src/ui/team_actions.rs:240` —— `pub fn interrupt_worker(&mut self, worker_session_id: &str, ...)`，函数拿到了 worker 的 product session id。
- `crates/alva-hub-gpui/src/ui/team_actions.rs:245-251` —— 用 `worker_session_id` 从 `self.active_engines` 里取出 `Arc<dyn EngineRuntime>`。这一步是对的，map 的键就是 product session id。
- `crates/alva-hub-gpui/src/ui/team_actions.rs:255` —— `let _ = engine.cancel("").await;`。**传给 adapter 的是空字符串字面量，不是引擎侧的 native session id**；返回值被 `let _ =` 丢弃。
- `crates/alva-hub-gpui/src/ui/team_actions.rs:256-262` —— 紧接着无条件 `refresh_team_silent` + `cx.notify()`，UI 照常刷新。

五个生产 adapter 的 `cancel` 一律先按 session id 查表，空串必然查不到：

- `crates/alva-engine-adapter-alva/src/adapter.rs:342-351` —— `.get(session_id).ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?`
- `crates/alva-engine-adapter-claude/src/adapter.rs:511-512` —— 委托 `send_control`；`crates/alva-engine-adapter-claude/src/adapter.rs:101-107` 同样 `.get(session_id) ... SessionNotFound`
- `crates/alva-engine-adapter-claude/src/print_adapter.rs:297-304` —— 同上
- `crates/alva-engine-adapter-codex/src/adapter.rs:211-218` —— 同上
- `crates/alva-engine-adapter-pi/src/adapter.rs:153-160` —— 同上

对照组一：**同一个 crate 里的正确写法**。

- `crates/alva-hub-gpui/src/ui/actions.rs:98` `stop_attempt` —— `:113` 取 `detail.session.engine_session_id.clone()`，`:128-129` `if let (Some(engine), Some(session_id)) = (engine, engine_session_id) { let _ = engine.cancel(&session_id).await; }`。
- 它还在 cancel 前做了 `record_attempt_cancelled`（`:117-121`）和 `permission_router.clear_session`（`:124`）。`interrupt_worker` 这两件事**一件都没做**。

对照组二：desktop 产品面。

- `crates/alva-hub-desktop/src/main.rs:910-917` —— `interrupt_worker` 命令体只有一行：`stop_attempt_inner(&app, &state, worker_session_id).await`。
- `crates/alva-hub-desktop/src/main.rs:1219` `stop_attempt_inner` —— `:1238-1241` 取真正的 `engine_session_id`，`:1244` 记录 cancel 事实，`:1247` 清 permission router，`:1248-1251` `engine.cancel(&engine_session_id).await` 且错误向上传播。

按钮入口：`crates/alva-hub-gpui/src/ui/team.rs:231-244`，label `"Interrupt"`，`on_click` 调 `hub.interrupt_worker(&session_id_interrupt, window, cx)`。

零测试覆盖：`crates/alva-hub-gpui/src/ui/team_actions.rs` 共 522 行，`rg -c 'cfg\(test\)'` 无命中，纯生产代码。

### (b) 降级链引擎目录漏掉 Pi

- `crates/alva-hub-gpui/src/ui/fallback.rs:20` —— `const ENGINE_IDS: [&str; 3] = ["alva", "claude", "codex"];`
- `crates/alva-hub-gpui/src/ui/fallback.rs:137` —— `.children(ENGINE_IDS.iter().map(|engine_id| { ... }))`，这个常量是降级链面板「引擎目录」按钮行的**唯一**数据源。
- `crates/alva-hub-gpui/src/ui/app.rs:40` —— `const ENGINE_OPTIONS: [&str; 4] = ["alva", "claude", "codex", "pi"];`，主引擎选择器有 4 个。
- `crates/alva-hub-gpui/src/readiness.rs:15` —— `pub const HUB_ENGINE_IDS: [&str; 4] = ["alva", "claude", "codex", "pi"];`，readiness 报告有 4 个（`:105`、`:326`、`:634` 均以它为准）。
- `crates/alva-hub-gpui/src/permissions.rs:183` —— 权限档位表也按 `crate::readiness::HUB_ENGINE_IDS` 展开，4 个。
- `crates/alva-hub-gpui/src/ui/fallback.rs:294-305` —— `toggle_fallback_target` 内部走 `EngineTarget::for_engine(engine_id)`；`crates/alva-hub-gpui/src/store.rs:77-82` 有完整的 `"pi"` 分支。**数据层完全支持 Pi 做 fallback 目标，挡住它的只有 UI 那个 3 元素常量。**
- desktop 侧同名常量 `crates/alva-hub-desktop/src/readiness.rs:15` 也是 4 个，两边 `HUB_ENGINE_IDS` 一致，只有 gpui 的 `fallback.rs` 自己另起了一份 3 元素副本。

## 复现

### (a) Interrupt 按钮

1. 起 GPUI 版 Hub，建一个 team run，让某个 worker 进入 running（`worker_running == true` 且 `lifecycle == WorkerLifecycle::Active`，否则按钮是 disabled，见 `team.rs:236`）。
2. 点该 worker 行上的「Interrupt」。
3. 观察到：Team 面板立刻刷新、按钮状态照常变化，但引擎子进程/流没有停，worker 继续产出 token。
4. 期望：worker 的当前 turn 被中断，时间线出现 Interrupted，与 desktop 版行为一致。

静态可证（不必起 UI）：`team_actions.rs:255` 恒传 `""` → 五个 adapter 的 `cancel` 一律 `sessions.get("")` → `None` → `Err(SessionNotFound(""))` → 调用点 `let _ =` 吞掉。这条链路上不存在任何一条能让 `""` 命中 session 表的路径（写入端从不以空串为键）。

### (b) Pi 不在降级链目录里

1. 起 GPUI 版 Hub，打开侧栏 "Fallback chain" 面板。
2. 观察到：引擎目录只有 `alva` / `claude` / `codex` 三个切换按钮。
3. 同时打开主引擎选择器和 readiness 面板：两处都能看到 `pi`。
4. 期望：目录里出现 `+ pi`，点击后 Pi 作为 fallback 目标进入策略列表并可保存。

## 工作

- `crates/alva-hub-gpui/src/ui/team_actions.rs:240-263` —— 重写 `interrupt_worker`：
  - 先 `self.store.get_session(worker_session_id)` 取 `detail`，从 `detail.session.engine_session_id` 拿真正的 native session id；拿不到就 `push_local_failure` 提示「引擎尚未建立原生会话」，不要静默返回。
  - cancel 前补上 `record_attempt_cancelled` 与 `permission_router.clear_session`，对齐 `crates/alva-hub-gpui/src/ui/actions.rs:117-124`。
  - `engine.cancel(&engine_session_id).await` 的 `Err` 不再丢弃：走 `push_local_failure`，UI 必须能显示「中断失败」。
  - 首选做法是直接复用 `actions.rs` 的 `stop_attempt`（desktop 就是这么委托的），只在 team 语义确实不同的地方分叉；能复用就不要再抄一份。
- `crates/alva-hub-gpui/src/ui/fallback.rs:20` —— 删除本地 `ENGINE_IDS`，`:137` 改用 `crate::readiness::HUB_ENGINE_IDS`（该 crate 内已通过 `lib.rs:47` 导出）。全仓 `rg -n 'ENGINE_IDS|ENGINE_OPTIONS'` 确认无第三份副本后，顺带评估 `ui/app.rs:40` 的 `ENGINE_OPTIONS` 是否也应收敛到同一常量。
- 给 `crates/alva-hub-gpui/src/ui/team_actions.rs` 建立测试入口（该文件当前 0 测试），至少覆盖本工单修的路径。

## 验收

- [ ] 新增测试：构造一个记录 `cancel` 入参的 fake `EngineRuntime`，走 `interrupt_worker`，断言收到的 session id **等于** store 里的 `engine_session_id` 且非空串；在未修的代码上该测试必须失败。
- [ ] 新增测试：`engine_session_id` 为 `None` 时 `interrupt_worker` 产生一条用户可见失败，而不是静默刷新。
- [ ] 新增测试：`interrupt_worker` 后 `permission_router` 中该 session 的待批项被清空（与 `actions.rs:124` 行为一致）。
- [ ] `rg -n 'cancel\(""\)' crates/ --glob '*.rs'` 输出为空。
- [ ] `rg -n 'const ENGINE_IDS' crates/alva-hub-gpui/src` 输出为空；`rg -n 'HUB_ENGINE_IDS' crates/alva-hub-gpui/src/ui/fallback.rs` 有命中。
- [ ] 新增测试：断言降级链目录渲染出的 engine id 集合 `== HUB_ENGINE_IDS`（长度 4 且包含 `"pi"`）。
- [ ] 手动确认：GPUI Fallback chain 面板出现 `+ pi`，点击后策略列表新增 pi 行并可保存、可上下移动。

## 完成记录

**(a) `interrupt_worker` —— 委托而不是再抄一份**

按工单首选方案改成 `self.stop_attempt(worker_session_id, cx)` + `refresh_team_silent`。desktop 正是这么做的（`main.rs` 的 `interrupt_worker` 命令体只有一行委托）。这一次改动同时补齐了原实现缺的**四件事**：真正的 `engine_session_id`（而不是 `""`）、`record_attempt_cancelled`、`permission_router.clear_session`、以及失败时的 `push_local_failure`——因为它们本来就在 `stop_attempt` 里。

`stop_attempt` 的 `record_attempt_cancelled` 是在 spawn **之前**同步执行的，所以紧接着的 `refresh_team_silent` 能看到已更新的 store 状态，时序正确。

**(b) 引擎目录三份副本收敛成一份**

删掉 `ui/fallback.rs` 的 3 元素 `ENGINE_IDS`，改用 `alva_hub_gpui::HUB_ENGINE_IDS`。顺带把工单只要求"评估"的第三份副本 `ui/app.rs:40` 的 `ENGINE_OPTIONS`（4 元素、内容相同）也一并收敛，3 个使用点重定向。现在全 crate 只有 `readiness.rs:15` 一处定义。

注意路径：`ui/` 下的模块属于 bin crate，`readiness` 在 lib crate，要走 `alva_hub_gpui::HUB_ENGINE_IDS` 这个 lib 导出而不是 `crate::readiness::`。

**验收 grep 全过**：`cancel("")` 全仓无命中；`const ENGINE_IDS` / `const ENGINE_OPTIONS` 在 gpui 无命中；`fallback.rs` 引用 `HUB_ENGINE_IDS`。

## 覆盖情况（变异实测，不是估计）

| 修复 | 变异 | 结果 |
|---|---|---|
| (b) 引擎目录 | 把 `HUB_ENGINE_IDS` 缩回 3 元素 | **RED** ✓ 有覆盖 |
| (a) `interrupt_worker` | 退回 no-op | **GREEN** ✗ **无覆盖** |

新增 `readiness.rs::engine_catalog_covers_every_supported_engine` 钉死目录内容（不只是长度——既有的 `:634` 只断言 `report.len() == HUB_ENGINE_IDS.len()`，两边一起缩水时不会红）。

**(a) 至今无测试覆盖，这是本工单的已知缺口。** 工单验收要求的三个测试（fake engine 记录 cancel 入参 / `engine_session_id` 为 None 时可见失败 / `clear_session` 被调用）都需要 gpui 的 UI 交互测试基建，而：
- `crates/alva-hub-gpui/src/ui/team_actions.rs` 与 `ui/actions.rs` 均零测试；
- 委托目标 `stop_attempt` 本身**也没有测试**；
- `ui/` 下现有的 7 个测试文件全在 `components/`，是纯组件级的，不含 `HubApp` + `Context` 的交互基建。

考虑到该 crate 已是退役路线（`AGENTS.md:279`），建这套基建的投入产出比低。缓解因素是这次修复**消除了重复**而不是修补它：`interrupt_worker` 不再持有自己的 cancel 逻辑，所以"传错 session id"这个具体缺陷不可能再单独回归——但如果有人重新给它写一份独立实现，没有测试会拦住。

若日后要补，最小落点是给 `stop_attempt` 建一次基建，`interrupt_worker` 自动继承。
