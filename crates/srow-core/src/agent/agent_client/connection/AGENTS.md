# agent/agent_client/connection
> ACP 子进程连接管理

## 地位
负责 ACP 外部 Agent 子进程的发现、启动、通信和孤儿清理。

## 逻辑
`AgentDiscovery` 在 PATH 中查找外部 Agent CLI，`AcpProcessManager` 管理进程池并通过 broadcast channel 分发消息，`AcpProcessHandle` 封装单个子进程的 stdin/stdout/stderr I/O 和状态跟踪，`orphan.rs` 在启动时扫描清理遗留进程。

## 约束
- ACP 协议使用 JSON Lines（每行一个 JSON 对象）通过 stdin/stdout 通信
- 子进程注入 `SROW_PROCESS_MANAGER_PID` 环境变量用于孤儿检测
- 孤儿清理当前为占位实现

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| discovery | `discovery.rs` | AgentDiscovery、ExternalAgentKind、AgentCliCommand |
| factory | `factory.rs` | AcpProcessManager、ProcessManagerConfig |
| processes | `processes.rs` | AcpProcessHandle、ProcessState |
| orphan | `orphan.rs` | 孤儿进程清理（占位） |
