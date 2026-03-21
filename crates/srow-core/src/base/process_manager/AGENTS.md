# base/process_manager
> 进程管理器基础设施再导出

## 地位
提供 ACP 子进程管理器的便捷导入路径，实际实现位于 `agent::agent_client::connection`。

## 逻辑
通过 `pub use` 再导出 `AcpProcessManager`、`ProcessManagerConfig`、`ProcessState`。

## 约束
- 仅做再导出，不包含新逻辑

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 再导出 AcpProcessManager、ProcessManagerConfig、ProcessState |
