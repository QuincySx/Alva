# alva-hub-desktop/src
> Hub Desktop 的 Rust 产品控制器与 Tauri 装配。

## 地位

位于 L7 Desktop 产品边界。业务执行只依赖 Hub/engine contract，具体 engine
装配和 Tauri IPC 保持在产品入口。

## 逻辑

1. `alva` 按用户选择为 workspace 构造 Alva、Claude light 或 Codex adapter。
2. `store` 在私有 SQLite 中原子保存 HubSession、多 EngineBranch、switch、handoff、timeline、Turn 和 Attempt。
3. `run` 创建或恢复 active branch 的 product/native session 映射，并向切换后的首次请求注入完整 handoff。
4. SessionStarted/ToolEnd 增量落盘，Completed 原子提交 Attempt usage 与 Turn 终态。
5. 运行中 switch 持久化为 queued；终态或启动 crash repair 后再原子应用。
6. runtime events 与 switch facts 被投影为稳定 timeline states。
7. `main` 从 Tauri app-data 初始化 store，并暴露运行、切换、列表和详情 IPC。

## 约束

- 产品测试从注入 `EngineRuntime` 的最高层 seam 进入。
- `EngineSessionId` 不作为用户会话 ID。
- terminal error 不得再投影为成功状态。
- restart resume 只从持久化 workspace 和 native session 映射恢复，不信任 UI 覆盖。
- 同一 MainlineSession 的 running EngineAttempt 由 SQLite partial unique index 强制唯一。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| Crate facade | `lib.rs` | 导出 run seam 与 DTO |
| Engine assembly | `alva.rs` | Alva provider/catalog、Claude light 与 Codex safe-default 装配 |
| Run controller | `run.rs` | 动态 Hub request、完整 handoff、事件投影与结果 |
| SQLite store | `store.rs` | identity、branch/switch/handoff/timeline、Turn/Attempt 与崩溃修复 |
| Desktop entry | `main.rs` | Tauri plugin、运行/切换 command 和 event bridge |
