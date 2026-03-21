# agent/agent_client
> ACP（Agent Communication Protocol）客户端 —— 外部 Agent 集成层

## 地位
srow-core 与外部编码 Agent（Claude Code、Qwen Code、Codex CLI、Gemini CLI）通信的完整客户端实现，包括进程管理、协议定义、会话控制和消息持久化。

## 逻辑
`connection/` 负责子进程发现和生命周期，`protocol/` 定义 ACP 消息格式，`session/` 管理会话状态机和权限审批，`storage/` 记录通信历史，`delegate.rs` 将 ACP 调用封装为 `AgentDelegate` trait 和 `AcpDelegateTool`（工具形态）。

## 约束
- ACP 协议基于 stdin/stdout JSON Lines
- AcpError 可转换为 EngineError

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | AcpError、pub use 再导出 |
| delegate | `delegate.rs` | AgentDelegate trait、AcpAgentDelegate、AcpDelegateTool |
| connection/ | `connection/` | 子进程发现/启动/通信 |
| protocol/ | `protocol/` | ACP 消息协议 |
| session/ | `session/` | 会话状态机 + 权限管理 |
| storage/ | `storage/` | ACP 消息持久化 |
