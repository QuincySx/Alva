# alva-hub-desktop/src
> Hub Desktop 的 Rust 产品控制器与 Tauri 装配。

## 地位

位于 L7 Desktop 产品边界。业务执行只依赖 Hub/engine contract，具体 engine
装配和 Tauri IPC 保持在产品入口。

## 逻辑

1. `alva` 为用户 workspace 构造完整 BaseAgent-backed adapter，并启用 transcript history。
2. `store` 在私有 SQLite 中原子保存 HubSession、活动 EngineBranch 和 Turn。
3. `run` 创建或恢复 product/native session 映射，再通过注入的 `EngineRuntime` 执行。
4. runtime events 被投影为稳定 timeline states，并在终态提交 Turn。
5. `main` 从 Tauri app-data 初始化 store，并暴露运行、列表和详情 IPC。

## 约束

- 产品测试从注入 `EngineRuntime` 的最高层 seam 进入。
- `EngineSessionId` 不作为用户会话 ID。
- terminal error 不得再投影为成功状态。
- restart resume 只从持久化 workspace 和 native session 映射恢复，不信任 UI 覆盖。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| Crate facade | `lib.rs` | 导出 run seam 与 DTO |
| Alva assembly | `alva.rs` | provider/model/catalog factory 装配 |
| Run controller | `run.rs` | Hub request、事件投影与结果 |
| SQLite store | `store.rs` | product/native identity、branch 和 Turn 事务持久化 |
| Desktop entry | `main.rs` | Tauri plugin、command 和 event bridge |
