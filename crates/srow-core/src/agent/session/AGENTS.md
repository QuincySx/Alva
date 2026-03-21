# agent/session
> Agent 会话管理 —— 占位模块

## 地位
预留的会话管理层，当前为空。实际 ACP 会话逻辑位于 `agent::agent_client::session`。

## 逻辑
无实质实现。

## 约束
- 未来可能扩展为统一会话管理器

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 session_manager 子模块 |
| session_manager | `session_manager.rs` | 占位 |
