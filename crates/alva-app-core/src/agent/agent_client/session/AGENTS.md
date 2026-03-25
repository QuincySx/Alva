# agent/agent_client/session
> ACP 会话管理

## 地位
管理单次 ACP 交互会话的状态机，处理入站消息分发和 HITL 权限审批流。

## 逻辑
`AcpSession` 维护会话状态（Ready/Running/WaitingForPermission/Completed/Cancelled/Error/Crashed），`handle_inbound` 驱动状态转换并将内容块转发为 `EngineEvent`，`PermissionManager` 缓存 always-allow/always-deny 决策。

## 约束
- 权限审批通过 oneshot channel 异步挂起
- AllowOnce/RejectOnce 不缓存

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| client | `client.rs` | AcpSession、AcpSessionState |
| permission_manager | `permission_manager.rs` | PermissionManager：ACP 权限缓存 |
