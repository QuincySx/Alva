# base
> 基础设施层

## 地位
提供 `alva-app-core` 的跨模块基础设施组件，当前仅包含进程管理器再导出。

## 逻辑
`process_manager/` 将 ACP 进程管理相关类型从 `agent::agent_client::connection` 再导出到统一路径。

## 约束
- 定位为基础设施聚合层，不应包含业务逻辑

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 process_manager 子模块 |
| process_manager/ | `process_manager/` | ACP 进程管理器再导出 |
