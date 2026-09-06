# alva-hub-core
> Alva Hub 的多引擎产品会话监管器。

## 地位

位于引擎 contract 之上、具体 adapter 之下。它只依赖 `alva-engine-runtime`，绝不依赖 kernel、agent SDK 或 app harness。

## 逻辑

1. 注册 product-visible `EngineId -> Arc<dyn EngineRuntime>`；同名替换 fail closed，避免已有 native session 被路由到新 engine 实例。
2. 生成并保存 Hub `ProductSessionId`，再从 engine event 获取独立的 engine session ID。
3. dispatch、resume、cancel、permission 均先按 product session 路由到对应 engine。
4. `restore_session`：产品从 journal 再水合 product→engine session 映射（幂等；engine/session 冲突 fail-closed）。
5. `switch_engine`：同一 `ProductSessionId` 切到另一已注册 engine；保留 previous_*；设 `pending_handoff`；清空 native session。
6. 同 product session execute：有 native id 则 resume；无则 handoff 首轮。`pending_handoff` 时注入 `HubRequest.handoff_context`（或默认 banner）一次；若 engine 同步启动失败、stream 产生 fatal error 或缺失 `Completed`，恢复待切换状态并缓存同一 context 供重试（恢复时终端状态保持 `Failed`，不把 fatal turn 伪造成 `Completed`）。
7. execute 转发循环用 `StreamContract` 观测；缺 `Completed` 仍合成 Error+Completed；引擎流留下未闭合工具时，在终端对之前合成失败的 `ToolEnd`，让产品工具卡片不会永久停在 running。
8. `abandon`：消费者放弃某轮（超时 / Ctrl-C / 关标签页）的公开出口——取消该轮 `CancellationToken`、推进 `claim_seq` 并置 `Failed`，下一次 execute/switch 可重新认领。
9. `cancel` 两级：能路由到 native session 且引擎支持 cancel 时才转发给引擎；否则等价 `abandon` 释放 Hub 侧 claim（会话不会被永久卡死）。
10. 每次 execute/abandon 递增 `claim_seq`，一切 spawned 轮的尾部写入（update/mark/restore/diagnostics）都要校验 `claim_seq == my_seq`，被取代的轮不得覆写新轮状态。
11. `HubSessionSnapshot` 含 stream 诊断 + previous engine + `pending_handoff`。

## 约束

- `ProductSessionId != engine_session_id` 是永久边界；handoff 后可扩展为一个 product session 管多个 engine session。
- 不能引入 `alva-kernel-*`、`alva-agent-*`、`alva-app-*` 或任何具体 adapter 依赖。
- engine stream 的事件语义以 `alva-engine-runtime` contract 为准；Hub 只观测与补终端，不重写 adapter 方言。
- stream diagnostics 是可观测性信号，**不得**单独把成功 turn 标成 Failed。
- 非终态必须有产品可达的出口：任何进入 `Starting`/`Running` 的 claim 都能被 `abandon`/`cancel` 释放，不存在只能靠重启进程恢复的卡死会话。`restore_session` 只接受终端状态，防止产品种出永不恢复的 running 会话。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| HubRuntime | `src/runtime.rs` | engine registry、session dispatch、resume/cancel 路由和状态投影 |
| HubSessionSnapshot | `src/runtime.rs` | 产品可见会话状态 + stream contract 诊断计数 |
| HubError | `src/error.rs` | registry/product-session 错误 |
| crate root | `src/lib.rs` | 对外 Hub contract |
