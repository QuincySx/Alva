# alva-hub-desktop
> ALVA Hub 的 Desktop-first L7 产品入口；独立于原生 Alva Desktop。

## 地位

本 crate 位于 `alva-hub-core` 与 engine adapters 之上。它拥有 Hub 产品交互和
Desktop IPC，不向 kernel、agent-core、`alva-app-cli` 或 `alva-app-tauri`
反向注入多引擎依赖。

## 逻辑

1. 产品控制器通过 `EngineRuntime` seam 驱动 `HubRuntime`。
2. `HubStore` 以 Desktop 私有 SQLite 持久化 product session、多 EngineBranch、switch、handoff、timeline、Turn 和 EngineAttempt。
3. Attempt 运行中增量记录 native session 与工具开始/完成证据，终态记录 usage/失败原因。
4. 用户显式选择 Alva/Claude/Codex；源 branch parked 并保留 native session，目标 branch 创建或恢复。
5. 首次进入目标 branch 注入完整 HandoffPackage；返回 parked branch 先 native resume。
6. 运行中切换排队到安全终态；启动修复 running Attempt 后自动应用可执行的 pending switch。
7. Tauri command 把用户运行/切换请求转换为 Hub 产品操作，并把 timeline event 发给 React。
8. React/Vite/TypeScript 前端负责 session list、workspace、engine selector、composer 和 timeline。

## 约束

- 手动多引擎 branch 已实现；自动 fallback 仍由后续 ticket 增量加入。
- 不复用 `alva-app-tauri` 的 Rust backend 或 session database。
- raw adapter stream 不直接泄露给 UI；先投影为产品 timeline vocabulary。
- SQLite 路径来自 Tauri app-data；目录/文件私有、拒绝符号链接、写入走事务，并以实例锁保护启动修复。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Product seam | `src/run.rs` | 动态 engine Hub 运行、完整 handoff 注入与 timeline 投影 |
| Engine assembly | `src/alva.rs` | 为 workspace 构造 Alva、Claude light 或 Codex adapter |
| Session store | `src/store.rs` | 私有 SQLite branch/switch/handoff/timeline/Turn/Attempt 持久化与恢复 |
| Tauri entry | `src/main.rs` | 运行/切换 IPC、event emission 与 Desktop runtime |
| Crate facade | `src/lib.rs` | 对 Tauri 层和测试暴露产品类型 |
| Web frontend | `web/` | React/Vite/TypeScript workspace、composer 与 timeline |
| Desktop config | `tauri.conf.json` / `capabilities/` | window、build 与最小权限配置 |
| Presentation assets | `icons/` | walking-skeleton 暂用的 Alva 图标 |
