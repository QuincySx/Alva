# 05 hub-server 错误路径泄漏引擎句柄与权限路由条目

- 级别：P0
- 影响：一次运行只要在收尾阶段出错（最常见：SQLite 忙/锁超时），这个会话的引擎句柄就永远留在服务端；用户之后对该会话点「停止」，服务端返回成功、时间线显示已中断，实际上什么都没停——正在跑的那一轮继续烧钱，同时该会话的旧待批权限也没被清掉。
- 状态：TODO

## 证据

### 泄漏点：错误分支 return 越过了紧随其后的清理块

- `crates/alva-hub-server/src/main.rs:775` —— `let outcome_fut = run_persisted_with_target(...)`。
- `crates/alva-hub-server/src/main.rs:780-802` —— 传进去的 build 闭包体，`:799-802` 是

  ```rust
  active_engines_for_build
      .lock()
      .unwrap()
      .insert(product_session_id.to_string(), Arc::clone(&engine));
  ```

  **无条件插入**，且降级链每换一个目标就再插一次（键相同、值被覆盖）。
- `crates/alva-hub-server/src/main.rs:896-902` —— 错误分支直接 return：

  ```rust
  let outcome = match outcome_fut.await {
      Ok(outcome) => outcome,
      Err(e) => {
          warn!(error = %e, "run failed");
          return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
      }
  };
  ```
- `crates/alva-hub-server/src/main.rs:903-909` —— 清理块**在 return 之后**，Err 路径永远走不到：

  ```rust
  // clean engine registry for this run
  {
      let mut map = active_engines_for_build.lock().unwrap();
      // remove any entries inserted for this product session (including fallbacks)
      map.retain(|k, _| k != &outcome.product_session_id);
      permission_router.clear_session(&outcome.product_session_id);
  }
  ```

  注意清理还依赖 `outcome.product_session_id`——错误路径上连这个 id 都拿不到，所以这不是「忘了写一行」，而是「按当前写法写不出来」：`Err` 变体里没有 session id。

对照组：desktop 无条件清理。

- `crates/alva-hub-desktop/src/main.rs:390-397` —— `.await` 之后直接清，不在任何 match 分支里：

  ```rust
  .await;
  let mut engines = state.active_engines.lock().unwrap();
  for product_session_id in registry.lock().unwrap().drain(..) {
      engines.remove(&product_session_id);
      state.permission_router.clear_session(&product_session_id);
  }
  drop(engines);
  outcome
  ```

  它靠一个独立的 `registry` 记录本轮插入过哪些 id，因此不依赖 `outcome`，Ok/Err 都能清干净。**hub-server 缺的就是这个 registry。**

### 可达的 Err 来源（`run_persisted_with_target` 内每一个 `?`）

`crates/alva-hub-desktop/src/run.rs:171` 的 `run_persisted_with_target` 返回 `Result`，以下位置任一失败即整体 `Err`：

- `:236` 主目标 `execute_persisted_turn(...).await?`
- `:257` 原生 resume 失效后重试的 `execute_persisted_turn(...).await?`
- `:293` / `:308` / `:338` `store.record_fallback_skip(...)?`（三处 skip 记录）
- `:360` `store.fallback_failed_attempt(&prepared, &failure_reason, &target)?`
- `:387` 降级目标的 `execute_persisted_turn(...).await?`
- `:418` `store.record_fallback_exhaustion(...)?`
- `:436` `store.finish_turn(...)?`
- `:443` `store.apply_pending_switch(&prepared.product_session_id)?`

`:436` 与 `:443` 是**每一轮都会执行的收尾写库**，且发生在引擎已经插进 map 之后。SQLite 忙/锁超时（并发请求、外部进程持有写锁）让这两行返回 `Err` 是最容易复现的路径，也是最坏的一种：这一轮实际已经跑完或跑到一半，句柄却永久滞留。

### 后果落点：stop 路由拿到的是滞留的旧句柄

- `crates/alva-hub-server/src/main.rs:1047` —— `.route("/api/sessions/:id/stop", post(stop_attempt))`
- `crates/alva-hub-server/src/main.rs:531` `stop_attempt`，其 `:559` 是

  ```rust
  let engine = match state.active_engines.lock().unwrap().get(&id).cloned() {
  ```

  只按 product session id 取，不校验这个句柄是否属于**当前**这一轮。
- `crates/alva-hub-server/src/main.rs:579-587` —— 先 `record_attempt_cancelled`、`permission_router.clear_session(&id)`，再 `engine.cancel(&engine_session_id).await`；`:588` 起的 `Ok(())` 分支广播 `TimelineStatus::Interrupted`。

于是形成这样一个窗口：同一 product session id 再次运行时，新一轮的 build 闭包会覆盖 map 里的旧值——但**在新一轮插入之前**（准备、鉴权、provider 解析这些阶段），以及新一轮压根没走到 build 的情况下，`stop` 拿到的仍是上一轮泄漏的死 adapter。此时 `engine_session_id` 来自 store，死 adapter 的 `sessions` 表里查不到它，`cancel` 返回 `SessionNotFound`；即便侥幸查到，取消的也是上一轮早已结束的流。两种情况用户看到的都是「已中断」。

## 复现

### 主问题

1. 起 hub-server，向 `POST /api/run` 提交一次运行。
2. 让收尾写库失败：最直接的是在 `crates/alva-hub-desktop/src/run.rs:436` 的 `store.finish_turn(...)` 前注入一次 `Err`；不改代码的话，用外部进程对 SQLite 文件持写锁直到 busy_timeout 到期。
3. 观察到：服务端日志出现 `run failed`，`POST /api/run` 返回 400；**`state.active_engines` 中该 product session id 的条目仍在**，`permission_router` 中该 session 的待批项也仍在。
4. 对同一个会话 `POST /api/sessions/:id/stop`（前提是 store 中该会话 status 仍为 `running`）。
5. 观察到：接口走 `main.rs:588` 的 Ok 分支，SSE 推出 `Interrupted`，而引擎侧毫无反应。
6. 期望：`Err` 路径与 `Ok` 路径一样清空 map 与 permission router；后续 `stop` 因为找不到 live engine 而按 `main.rs:559-568` 返回 `"no live engine"`，用户看到的是真实结果。

静态可证部分（无需注入）：`main.rs:900` 的 `return` 与 `:903-909` 的清理块之间不存在任何其它出口，控制流上 Err 分支 100% 越过清理；而且清理块引用 `outcome.product_session_id`，Err 分支不持有该值。

### 附带问题：被否决的引擎已经写进影子表

1. 配一条降级链，让某个 fallback 目标带上主引擎不具备的 `required_capabilities`（见工单 06）。
2. 触发降级。
3. 观察到：`crates/alva-hub-desktop/src/run.rs:322-326` **先** `build_engine(&candidate, ...)`（hub-server 的闭包此刻已 `insert`），`:328-351` **才**检查 capabilities 并 `record_fallback_skip` + `continue`（`continue` 在 `:350`）。被否决的候选引擎已经进了 `active_engines`，`continue` 之后无人移除。
4. 期望：被否决的候选不留在影子表里；`stop` 不可能路由到一个从未 `execute` 过的 adapter。

## 工作

- `crates/alva-hub-server/src/main.rs` —— 照搬 desktop 的 registry 方案：
  - 在 `:775` 之前建一个 `Arc<Mutex<Vec<String>>>` registry；build 闭包（`:799-802`）插入 map 的同时把 `product_session_id` push 进 registry。
  - 把 `:903-909` 的清理块移到 `:896` 的 match **之前**，或改写为 `let outcome = outcome_fut.await;` → 清理 → `match outcome { ... }`，用 registry 而不是 `outcome.product_session_id` 做键，保证 Ok/Err 都执行。
  - 更稳妥的做法是抽一个 guard 类型（Drop 时清理），让「插入过就一定清理」不依赖后续维护者记得。
- `crates/alva-hub-server/src/main.rs:559` —— stop 路由取到句柄后增加归属校验：至少确认 store 里该会话的 `engine_session_id` 与当前这一轮一致，或引入 run token 比对，避免拿到上一轮的句柄。
- `crates/alva-hub-desktop/src/run.rs:322-351` —— 把 capability 检查提到 `build_engine` **之前**，或在 `continue` 之前显式撤销这次 build 造成的注册（需要给 build 闭包配一个撤销钩子）。gpui 侧同一段代码在 `crates/alva-hub-gpui/src/run.rs:313-342`，同改。

## 验收

- [ ] 新增 hub-server 层测试：让 `run_persisted_with_target` 返回 `Err`，断言调用后 `state.active_engines` 中该 product session id **不存在**、`permission_router` 中该 session 无残留；在未修的代码上该测试必须失败。
- [ ] 新增测试覆盖 `Ok` 路径同样清空（防止修复时把正常路径的清理删掉）。
- [ ] 新增测试：`Err` 之后立刻 `POST /api/sessions/:id/stop`，断言返回 `"no live engine"`（`main.rs:559-568` 分支），而不是 200 + `Interrupted`。
- [ ] 新增测试：一个带 `required_capabilities` 且必然被否决的 fallback 候选，跑完后断言该候选 engine 未出现在 `active_engines` 中。
- [ ] 人工逐处确认并在 PR 描述中列出：`rg -n 'active_engines' crates/alva-hub-server/src/main.rs` 的每一处 insert 都能在同一函数内找到配对的无条件清理路径。
- [ ] `cargo test -p alva-hub-server` 与 `cargo test -p alva-hub-desktop` 全绿。
