# alva-hub-core
> Alva Hub 的多引擎产品会话监管器。

## 地位

位于引擎 contract 之上、具体 adapter 之下。它只依赖 `alva-engine-runtime`，绝不依赖 kernel、agent SDK 或 app harness。

## 逻辑

1. 注册 product-visible `EngineId -> Arc<dyn EngineRuntime>`；同名替换 fail closed，避免已有 native session 被路由到新 engine 实例。
2. 生成并保存 Hub `ProductSessionId`，再从 engine event 获取独立的 engine session ID。
3. dispatch、resume、cancel、permission 均先按 product session 路由到对应 engine。
4. 同一 product session 的 execute 在一个写锁内抢占 `Starting` 状态，防止并发 resume；非恢复性 `RuntimeEvent::Error` 会锁定 `Failed`，后续 `Completed` 不得覆盖，恢复性错误则允许正常完成。违反终止契约的 engine stream 会被投影为 `Failed` 并补齐 Error/Completed。
5. execute 转发循环用 `StreamContract` 观测每条 engine 事件：hard violation `tracing::warn`（debug build 另 `eprintln`），soft warning `debug`；**不丢弃**违规事件，便于 adapter 排障；缺 `Completed` 仍合成 Error+Completed。
6. 观测结果写入 `HubSessionSnapshot`：`stream_hard_violations` / `stream_soft_warnings` / `last_stream_violation`；每次 execute 开始时清零。产品层（CLI / 未来 inspector）可读，不改变会话成功/失败语义。

## 约束

- `ProductSessionId != engine_session_id` 是永久边界；handoff 后可扩展为一个 product session 管多个 engine session。
- 不能引入 `alva-kernel-*`、`alva-agent-*`、`alva-app-*` 或任何具体 adapter 依赖。
- engine stream 的事件语义以 `alva-engine-runtime` contract 为准；Hub 只观测与补终端，不重写 adapter 方言。
- stream diagnostics 是可观测性信号，**不得**单独把成功 turn 标成 Failed。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| HubRuntime | `src/runtime.rs` | engine registry、session dispatch、resume/cancel 路由和状态投影 |
| HubSessionSnapshot | `src/runtime.rs` | 产品可见会话状态 + stream contract 诊断计数 |
| HubError | `src/error.rs` | registry/product-session 错误 |
| crate root | `src/lib.rs` | 对外 Hub contract |
