# agent
> Agent 核心子系统 —— 运行时、客户端、记忆、持久化、编排、会话

## 地位
alva-app-core 最庞大的顶级模块，包含 Agent 系统的所有运行时组件。

## 逻辑
`runtime/` 驱动 Agent 的 agentic loop（引擎 + 工具 + 安全），`agent_client/` 管理与外部 Agent 的 ACP 通信，`memory/` 提供持久化知识检索，`persistence/` 实现 SQLite 会话存储，`orchestrator/` 编排多个 Agent 协作，`session/` 为占位模块。

## 约束
- 子模块间有明确的依赖方向：engine -> tools, engine -> storage, orchestrator -> engine

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明所有子模块 |
| runtime/ | `runtime/` | 引擎 + 工具 + 安全 |
| agent_client/ | `agent_client/` | ACP 外部 Agent 客户端 |
| memory/ | `memory/` | FTS + 向量混合记忆系统 |
| persistence/ | `persistence/` | SQLite 会话持久化 |
| orchestrator/ | `orchestrator/` | 多 Agent 编排 |
| session/ | `session/` | 会话管理（占位） |
