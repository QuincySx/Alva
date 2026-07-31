# alva-hub-desktop/src
> Hub Desktop 的 Rust 产品控制器与 Tauri 装配。

## 地位

位于 L7 Desktop 产品边界。业务执行只依赖 Hub/engine contract，具体 engine
装配和 Tauri IPC 保持在产品入口。

## 逻辑

1. `alva` 为用户 workspace 构造完整 BaseAgent-backed adapter。
2. `run` 接收用户 prompt/workspace，`HubRuntime` 通过注入的
   `EngineRuntime` 执行。
3. runtime events 被投影为稳定 timeline states。
4. `main` 把产品 seam 暴露为 Tauri IPC 和 `hub_timeline` event。

## 约束

- 产品测试从注入 `EngineRuntime` 的最高层 seam 进入。
- `EngineSessionId` 不作为用户会话 ID。
- terminal error 不得再投影为成功状态。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| Crate facade | `lib.rs` | 导出 run seam 与 DTO |
| Alva assembly | `alva.rs` | provider/model/catalog factory 装配 |
| Run controller | `run.rs` | Hub request、事件投影与结果 |
| Desktop entry | `main.rs` | Tauri plugin、command 和 event bridge |
