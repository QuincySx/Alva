# alva-acp
> ACP (Agent Client Protocol) 客户端，管理外部 Agent CLI 进程的通信与生命周期

## 地位
外部 Agent 集成的协议层。定义 ACP 消息格式（13 种入站 + 5 种出站），管理 Agent CLI 子进程的发现、启动、通信和会话状态。通过 AgentDelegate trait 支持将外部 Agent 作为工具委托执行。

## 逻辑
```
AgentDiscovery → 发现可用的 Agent CLI
       │
       ▼
AcpProcessManager → 启动/管理子进程（stdin/stdout JSON lines）
       │
       ▼
AcpSession → 维护会话状态 + PermissionManager 权限审批
       │
       ▼
AgentDelegate trait → 外部 Agent 作为委托工具执行任务
```

### protocol/ 子模块消息分类
- **bootstrap**: BootstrapPayload、ModelConfig、SandboxLevel — 初始化配置
- **message**: AcpInboundMessage（13 种）/ AcpOutboundMessage（5 种）— 消息枚举
- **permission**: PermissionRequest、PermissionData、RiskLevel — 权限审批
- **lifecycle**: 进程生命周期事件
- **content**: 内容块类型
- **tool**: 工具调用相关消息
- **special**: 特殊控制消息

## 约束
- 通信基于 stdin/stdout JSON lines 协议，每行一个 JSON 消息
- AcpSession 维护有限状态机（Idle → Running → WaitingForPermission → ...）
- PermissionManager 处理异步权限审批流程
- AgentDelegate 是 async trait，实现者需保证线程安全

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 协议消息 | `protocol/` | ACP 消息类型定义（bootstrap / message / permission / lifecycle / content / tool / special） |
| 连接管理 | `connection.rs` | AgentDiscovery、AcpProcessManager、AcpProcessHandle、进程状态管理 |
| 会话 | `session.rs` | AcpSession 会话状态机 + PermissionManager |
| 委托 | `delegate.rs` | AgentDelegate trait、AcpAgentDelegate、DelegateResult |
| 错误 | `error.rs` | AcpError 错误类型 |
