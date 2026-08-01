# alva-hub-desktop
> ALVA Hub 的 Desktop-first L7 产品入口；独立于原生 Alva Desktop。

## 地位

本 crate 位于 `alva-hub-core` 与 engine adapters 之上。它拥有 Hub 产品交互和
Desktop IPC，不向 kernel、agent-core、`alva-app-cli` 或 `alva-app-tauri`
反向注入多引擎依赖。

## 逻辑

1. 产品控制器通过 `EngineRuntime` seam 驱动 `HubRuntime`。
2. `HubStore` 以 Desktop 私有 SQLite 持久化 product session、活动 branch 和 Turns。
3. Alva adapter transcript 与 Hub 数据库分开保存，用 native session ID 跨进程续跑。
4. Tauri command 把用户请求转换为 Hub request，并把 timeline event 发给 React。
5. React/Vite/TypeScript 前端负责 session list、workspace、composer 和 timeline。

## 约束

- 当前只注册一个 Alva branch；多引擎 branch 和 fallback 由后续 ticket 增量加入。
- 不复用 `alva-app-tauri` 的 Rust backend 或 session database。
- raw adapter stream 不直接泄露给 UI；先投影为产品 timeline vocabulary。
- SQLite 路径来自 Tauri app-data；目录/文件私有、拒绝符号链接、写入走事务。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Product seam | `src/run.rs` | 单次 Hub 运行与 timeline 投影 |
| Alva assembly | `src/alva.rs` | 为选定 workspace 构造完整 Alva adapter |
| Session store | `src/store.rs` | 私有 SQLite HubSession/EngineBranch/Turn 持久化 |
| Tauri entry | `src/main.rs` | IPC command、event emission 与 Desktop runtime |
| Crate facade | `src/lib.rs` | 对 Tauri 层和测试暴露产品类型 |
| Web frontend | `web/` | React/Vite/TypeScript workspace、composer 与 timeline |
| Desktop config | `tauri.conf.json` / `capabilities/` | window、build 与最小权限配置 |
| Presentation assets | `icons/` | walking-skeleton 暂用的 Alva 图标 |
