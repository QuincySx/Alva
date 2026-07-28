# alva-hub-core
> Alva Hub 的多引擎产品会话监管器。

## 地位

位于引擎 contract 之上、具体 adapter 之下。它只依赖 `alva-engine-runtime`，绝不依赖 kernel、agent SDK 或 app harness。

## 逻辑

1. 注册 product-visible `EngineId -> Arc<dyn EngineRuntime>`；同名替换 fail closed，避免已有 native session 被路由到新 engine 实例。
2. 生成并保存 Hub `ProductSessionId`，再从 engine event 获取独立的 engine session ID。
3. dispatch、resume、cancel、permission 均先按 product session 路由到对应 engine。
4. 同一 product session 的 execute 在一个写锁内抢占 `Starting` 状态，防止并发 resume；违反终止契约的 engine stream 会被投影为 `Failed` 并补齐 Error/Completed。

## 约束

- `ProductSessionId != engine_session_id` 是永久边界；handoff 后可扩展为一个 product session 管多个 engine session。
- 不能引入 `alva-kernel-*`、`alva-agent-*`、`alva-app-*` 或任何具体 adapter 依赖。
- engine stream 的事件语义以 `alva-engine-runtime` contract 为准。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| HubRuntime | `src/runtime.rs` | engine registry、session dispatch、resume/cancel 路由和状态投影 |
| HubError | `src/error.rs` | registry/product-session 错误 |
| crate root | `src/lib.rs` | 对外 Hub contract |
