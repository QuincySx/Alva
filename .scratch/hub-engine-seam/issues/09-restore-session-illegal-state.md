# 09 restore_session 可从公共 API 直接种出非法状态（收窄为终态）

- 级别：P1
- 影响：产品从磁盘恢复会话时若传错一个状态值，这个会话一落地就是"没有任何流在跑、却显示正在运行"，从此发消息永远报 "already running"，且没有任何恢复动作。
- 状态：TODO

## 前提：今天 blast radius 有限，路线 A 落地当天变线上并发 bug

今天不触发**纯粹靠调用方自律**：全仓三个 `restore_session` 生产调用点都传 `Completed`——

- `crates/alva-hub-cli/src/main.rs:158-163`（`HubSessionStatus::Completed`）
- `crates/alva-hub-desktop/src/run.rs:531-536`（`HubSessionStatus::Completed`）
- `crates/alva-hub-gpui/src/run.rs:513-518`（`HubSessionStatus::Completed`）

（`rg -n 'restore_session' crates/ --glob '*.rs'` 全命中列表：上述三处 + `runtime.rs:286` 定义 + 5 处测试调用。）

而且 `crates/alva-hub-desktop/src/run.rs:526` 与 `crates/alva-hub-gpui/src/run.rs:508` 每轮 `HubRuntime::new()`，就算种出非法状态也随该轮一起消失。`docs/specs/hub-headless-service.md:143` 的**路线 A**（gateway 加 hub 路由组、常驻进程、handler 调 `alva-hub-desktop::lib` 纯函数）落地后，rehydrate 会成为服务启动/请求路径上的常规动作，调用点变多、跨模块，"自律"不再是防线；且与**工单 08**叠加后，种出来的 `Running` 会话没有任何恢复动作，只能重启服务。

## 证据

### 状态参数无约束，直落 session map

- `crates/alva-hub-core/src/runtime.rs:286-292` —— 签名第四个参数是不受限的 `status: HubSessionStatus`：

  ```rust
  pub fn restore_session(
      &self,
      product_session_id: ProductSessionId,
      engine_id: impl Into<EngineId>,
      engine_session_id: impl Into<String>,
      status: HubSessionStatus,
  ) -> Result<(), HubError> {
  ```
- `crates/alva-hub-core/src/runtime.rs:335-338` —— 新建分支**原样写入，零校验**：

  ```rust
  let mut session = HubSession::new(engine_id);
  session.engine_session_id = Some(engine_session_id);
  session.status = status;
  sessions.insert(product_session_id, session);
  ```
- `crates/alva-hub-core/src/runtime.rs:295-299` —— 函数里现有的两道校验只管 `engine_session_id` 非空与 engine 已注册，都不看 `status`。
- 值域：`crates/alva-hub-core/src/runtime.rs:88-93` 的 `HubSessionStatus` 有 `Starting | Running | Completed | Failed` 四个值，前两个都能被塞进去。

### 已有分支证明"非终态是危险的"这件事作者已经知道一半

- `crates/alva-hub-core/src/runtime.rs:326-331` —— **已存在**的 session 分支带守卫：只有当前不是 `Starting|Running` 时才覆盖 `status`。也就是说函数已经在保护"别把跑着的会话状态改掉"，却完全没有保护"别凭空造一个跑着的会话"。缺口精确地落在 `:335-338` 的新建分支。

### 落地即永久 SessionBusy

- `crates/alva-hub-core/src/runtime.rs:429-434` —— `execute` 读到 `Starting|Running` 直接 `Err(HubError::SessionBusy(...))`。
- `crates/alva-hub-core/src/runtime.rs:367-372` —— `switch_engine` 同样的守卫，同样 `SessionBusy`。
- `crates/alva-hub-core/src/runtime.rs:640-642` —— `cancel` 会去调引擎，但这个 session 背后**根本没有** spawned task（`restore_session` 不 spawn 任何东西），转发到引擎的取消对一个不存在的 turn 无意义。
- 结合工单 08：全仓 `rg -n 'abandon|reaper|CancellationToken' crates/alva-hub-core/src` 零命中，非终态没有任何产品可达的出口。
- `crates/alva-hub-core/src/error.rs:31-32` —— 用户看到的文案是 `product session '{0}' is already running`，而实际没有任何东西在 running。

### 测试基座正靠这个洞造状态

- `crates/alva-hub-core/src/runtime.rs:1823-1844` —— `switch_engine_refuses_a_running_session` 就是用 `restore_session(..., HubSessionStatus::Running)`（`:1829-1834`）当作"伪造一个 Running 会话"的便捷手段。这条测试同时是缺口存在的实证，也是本次收窄必须一并迁移的对象。

### 顺带：status 词表全仓不对齐，用户主动取消与成功不可区分

`rg -n 'enum .*Status|Cancelled|Canceled|Interrupted' crates/alva-hub-core/src crates/alva-hub-desktop/src crates/alva-hub-cli/src crates/alva-engine-runtime/src --glob '*.rs'` 的结果显示三套词表：

- `crates/alva-hub-core/src/runtime.rs:88-93` —— `HubSessionStatus { Starting, Running, Completed, Failed }`，**没有** `Cancelled` / `Interrupted`。
- `crates/alva-hub-desktop/src/run.rs:47-60` —— `TimelineStatus { UserMessage, AssistantMessage, ActivityStarted, ActivityCompleted, Running, Completed, Failed, Interrupted, Fallback, Exhausted, PermissionRequired, Worker }`，**有** `Interrupted`（用在 `crates/alva-hub-desktop/src/main.rs:371,1255`）。
- `crates/alva-engine-runtime/src/error.rs:27-28` —— `RuntimeError::Cancelled`，`Display` 为 `"Cancelled"`（`crates/alva-engine-runtime/src/tests.rs:396-397` 钉住）。

后果：用户主动取消一轮，若 adapter 老实按契约在收尾发 `RuntimeEvent::Completed`，`update_session`（`crates/alva-hub-core/src/runtime.rs:704-706`）会把它记成 `Completed`——在 hub 层与一次真正成功的对话**完全不可区分**。hub-desktop 靠自己那套 `TimelineStatus::Interrupted` 在产品层区分（`crates/alva-hub-desktop/src/run.rs:3123` 的 `"Cancelled by user"`、`main.rs:1255`），hub-cli 则完全没有这个概念。

## 复现

### 主缺陷（静态可证，且可直接落测）

推导链：`restore_session` 的 `status` 参数无校验（`:286-292`）→ 新建分支原样写入（`:335-338`）→ `execute`/`switch_engine` 的守卫（`:429-434`、`:367-372`）读到非终态就拒绝 → 没有任何出口（工单 08 的证据段）。

可执行复现，写成 `crates/alva-hub-core/src/runtime.rs` 里的一条 `#[tokio::test]`：

1. `let hub = HubRuntime::new();` 注册 `FakeEngine` 为 `"alva"`。
2. `hub.restore_session(product_id.clone(), "alva", "native-running", HubSessionStatus::Running).unwrap();`
3. 观察到：
   - `hub.session(&product_id).unwrap().status == HubSessionStatus::Running`，而进程内**没有**任何 spawned task 对应它（`restore_session` 全程不 spawn）
   - `hub.execute(HubRequest::new("alva", RuntimeRequest::new("hi")).resume(product_id.clone()))` → `Err(HubError::SessionBusy(_))`，文案 `already running`
   - `hub.switch_engine(&product_id, "alva")` → 同样 `Err(HubError::SessionBusy(_))`（`runtime.rs:1823-1844` 已把这条钉住）
   - 再次 `restore_session(..., HubSessionStatus::Completed)` 也救不回来：`:326-331` 的守卫恰好禁止覆盖 `Starting|Running`
4. 期望：步骤 2 应当在**编译期**（类型收窄）或运行期（`Err`）就被拒绝，不允许存在"没有流在跑的 Running 会话"。

### 顺带项（静态可证）

`HubSessionStatus`（`:88-93`）的值域里不存在任何表达"用户主动取消"的值，因此"取消后的 turn"与"成功完成的 turn"在 `HubSessionSnapshot.status`（`:101`）上必然取同一个值 `Completed`。无需运行即成立。

## 工作

- `crates/alva-hub-core/src/runtime.rs` 定义终态子枚举并收窄签名：

  ```rust
  /// A rehydrated session can only be in a terminal state — there is no live
  /// stream behind a restored entry.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RestoredStatus { Completed, Failed }
  ```

  `restore_session`（`:286-292`）第四参数改为 `RestoredStatus`，函数体内转成 `HubSessionStatus` 后再写 `:330`、`:337`。选择类型收窄（而非运行期 `Err`）是因为 rehydrate 的语义本来就只有终态成立，编译期挡住比运行期报错更彻底；`crates/alva-hub-core/src/lib.rs:11` 的导出列表补上 `RestoredStatus`。
- 迁移三个生产调用点：`crates/alva-hub-cli/src/main.rs:158-163`、`crates/alva-hub-desktop/src/run.rs:531-536`、`crates/alva-hub-gpui/src/run.rs:513-518`，三处都从 `HubSessionStatus::Completed` 改为 `RestoredStatus::Completed`（语义零变化）。
- 迁移测试调用点：`runtime.rs:1224-1228`、`:1258-1262`、`:1266`、`:1479-1483` 直接换成 `RestoredStatus::Completed`；`:1823-1844` 的 `switch_engine_refuses_a_running_session` 不能再用 `restore_session` 伪造 `Running`，改为经由真实路径进入 `Running`——注册 `PendingEngine`（`:875-908`）后 `execute` 一轮不 drain，再断言 `switch_engine` 报 `already running`。断言文本 `"already running"` 保持逐字不变。
- 更新 `crates/alva-hub-core/src/AGENTS.md`：「逻辑」第 4 条（`restore_session`）补一句"只接受终态"，「约束」补一条"非终态只能由 execute 认领产生"。
- **顺带项单独评估、不在本工单落地**：给 `HubSessionStatus` 加 `Cancelled` 是跨 crate 的词表对齐（要同时看 `crates/alva-hub-desktop/src/run.rs:47-60` 的 `TimelineStatus::Interrupted` 与 `crates/alva-engine-runtime/src/error.rs:27-28` 的 `RuntimeError::Cancelled`，并回答"adapter 用什么事件表达取消"这个 contract 问题）。本工单只负责把上面证据段写清楚，留作后续工单的输入。

## 验收

- [ ] `cargo test --workspace` 全绿（收窄是破坏性签名变更，三个生产调用点 + 5 个测试调用点必须全部迁完才能编译）。
- [ ] `rg -n 'restore_session' crates/ --glob '*.rs'` 的每一处调用第四参数都是 `RestoredStatus::*`，无一处仍传 `HubSessionStatus::*`。
- [ ] 编译期证明：临时把某个调用点改回 `HubSessionStatus::Running`，`cargo check -p alva-hub-core` 必须报类型错误（而不是通过）。
- [ ] `switch_engine_refuses_a_running_session`（`runtime.rs:1823`）改用 `PendingEngine` + 真实 `execute` 后仍通过，且 `"already running"` 断言逐字不变。
- [ ] 新增测试 `restored_session_is_immediately_executable`：`restore_session(..., RestoredStatus::Completed)` 之后 `hub.execute(...)` 直接返回 `Ok`（rehydrate 后必须可用）。
- [ ] `HubSessionSnapshot` 公开字段集合零改动（`git diff` 中 `pub struct HubSessionSnapshot` 块无变化），`HubSessionStatus` 枚举本体零改动（顺带项不在本工单）。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无新增告警；依赖防火墙检查通过。
- [ ] `crates/alva-hub-core/src/AGENTS.md` 已更新。
