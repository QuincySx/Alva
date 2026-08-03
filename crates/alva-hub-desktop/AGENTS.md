# alva-hub-desktop
> ALVA Hub 的 Desktop-first L7 产品入口；独立于原生 Alva Desktop。

## 地位

本 crate 位于 `alva-hub-core` 与 engine adapters 之上。它拥有 Hub 产品交互和
Desktop IPC，不向 kernel、agent-core、`alva-app-cli` 或 `alva-app-tauri`
反向注入多引擎依赖。

## 逻辑

1. 产品控制器通过 `EngineRuntime` seam 驱动 `HubRuntime`。
2. `HubStore` 以 Desktop 私有 SQLite 持久化 product session、session-scoped FallbackPolicy snapshot、多 EngineBranch、switch、handoff、timeline、Turn 和 EngineAttempt。
3. Attempt 运行中增量记录 native session 与工具开始/完成证据，终态记录 usage/失败原因。
4. 用户显式选择 Alva/Claude/Codex；源 branch parked 并保留 native session，目标 branch 创建或恢复。
5. 首次进入目标 branch 注入完整 HandoffPackage；返回 parked branch 以 watermark 生成 delta 并先 native resume，失效时原子升级 full handoff 后 fresh 执行。
6. 运行中切换排队到安全终态；启动修复 running Attempt 后自动应用可执行的 pending switch。
7. Tauri command 把用户运行/切换请求转换为 Hub 产品操作，并把 timeline event 发给 React。
8. React/Vite/TypeScript 前端负责 session list、workspace、engine selector、composer 和 timeline。
9. 用户保存全局或 session fallback chain 前可看到 target 的 model/provider、账号、账单路径和自动使用授权；手动切换不授予自动 fallback 权限。
10. Engines dashboard 自动发现三个 engine 并展示版本兼容性、capabilities、auth 与 stream health；不可用原因是结构化 reason，可从 composer 与详情到达；刷新只读。

11. terminal 失败先经稳定 FailureClass 矩阵分类（missing/incompatible/auth/model/rate-quota-billing/context/crash/transport/other）；仅分类到 operational continuity failure 才在同一 Turn 自动切换到 fallback chain 下一个授权 target：源 branch parked、handoff 先持久化再执行、串行新 Attempt、非阻塞 fallback timeline card。
12. fallback chain 按 policy 顺序有界执行：每个 EngineTarget（engine/model/provider 三元组）每 attempt cycle 最多考虑一次、不回环；不健康或能力不兼容 target 被跳过并落 `engine_fallback_skipped` fact；链路耗尽落 `fallback_exhausted` card，列出每次 attempt/skip 的终态与原因。
13. stall watchdog：无 engine/tool/permission/stream 活动的 180s 可证明窗口后确认 stall 并分类为 continuity failure 进入有界 fallback；open tool、pending 审批与恢复中的输出暂停/重置计时。
14. fallback card 提供可逆控制：View handoff 展示实际发送的结构化 HandoffPackage（delivered/pending 状态）；Switch back 走手动 switch 恢复 parked source（后续 run 遵守 delta/full resume 规则）；Stop 取消当前活动 Attempt 并落 `attempt_cancelled` fact，重启不重放。
15. HubSession 持有稳定 PermissionProfile（interactive/workspace_write/unrestricted），branch 切换与 fallback 不改变；engine 只有能等价或更严格执行该 profile 才有资格（映射契约在 `permissions.rs`），只能弱化的 target 标记 ineligible 并显示原因，fallback、手动 switch 与 engine 装配三处都 fail-closed；PermissionRequest/Decision 按 request_id 路由到发起请求的那个 EngineSessionId，决定单次使用且 attempt 终态/Stop 后失效，无路由身份或已 doomed 的等待以有界 stall 收敛。

16. HandoffPackage 跨 engine 前经统一过滤（credentials/env 值清洗、账号账单标签与 team snapshot 剥离），过滤在持久化前完成故 View handoff 即 adapter 实收内容；full transcript 每 session 显式开启（默认关），启用后仍过同一过滤。

## 约束

- 自动 fallback 只消费当前 session policy snapshot 中显式授权的 target；无授权则 Turn 直接失败。
- canary 测试钉死敏感字段不达另一 EngineRuntime。
- 未分类（Other）失败与成功但质量不佳的结果都不触发自动 fallback。
- 不复用 `alva-app-tauri` 的 Rust backend 或 session database。
- raw adapter stream 不直接泄露给 UI；先投影为产品 timeline vocabulary。
- SQLite 路径来自 Tauri app-data；目录/文件私有、拒绝符号链接、写入走事务，并以实例锁保护启动修复。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Product seam | `src/run.rs` | 动态 engine Hub 运行、完整 handoff 注入与 timeline 投影 |
| Engine assembly | `src/alva.rs` | 为 workspace 构造 Alva、Claude light 或 Codex adapter |
| Engine readiness | `src/readiness.rs` | 只读 probe、结构化 readiness 分类与 dashboard DTO |
| Permission contract | `src/permissions.rs` | session PermissionProfile、adapter 映射资格与 PermissionRouter |
| Session store | `src/store.rs` | 私有 SQLite policy snapshot、branch/switch/handoff/timeline/Turn/Attempt 持久化与恢复 |
| Tauri entry | `src/main.rs` | 运行/切换 IPC、event emission 与 Desktop runtime |
| Crate facade | `src/lib.rs` | 对 Tauri 层和测试暴露产品类型 |
| Web frontend | `web/` | React/Vite/TypeScript workspace、composer 与 timeline |
| Desktop config | `tauri.conf.json` / `capabilities/` | window、build 与最小权限配置 |
| Presentation assets | `icons/` | walking-skeleton 暂用的 Alva 图标 |
