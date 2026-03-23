# srow-core
> 应用核心引擎，整合运行时工具、安全体系、持久化、MCP/Skill 集成和环境管理

## 地位
系统的业务核心层。将 agent-types / agent-core / protocol-* 等底层 crate 与具体业务需求结合，提供完整的 Agent 运行时（含 16 种内置工具）、多层安全防护、SQLite 持久化、MCP 工具桥接、技能系统集成和嵌入式运行时环境管理。采用 DDD 分层架构（domain / ports / adapters）。

## 逻辑
```
                        ┌─────────────────────────────────────────────┐
                        │              srow-core                      │
                        │                                             │
  agent/                │  runtime/tools/     9 标准 + 7 浏览器工具    │
    ├─ agent_client/    │  runtime/security/  guard → permission →    │
    │   (ACP 集成)      │                     sandbox → sensitive_paths│
    ├─ memory/          │  persistence/       SQLite schema +         │
    │   (FTS+向量搜索)  │                     migrations              │
    └─ session/         │                                             │
        (会话管理)      │                                             │
                        │                                             │
  mcp/                  │  MCP 运行时管理 + 工具适配 + 配置           │
  skills/               │  技能加载/存储/注入 + AgentTemplate         │
  environment/          │  嵌入式运行时（Bun/Node/Python/Chromium）   │
                        │                                             │
  domain/  ports/  adapters/  (DDD 分层)                              │
                        └─────────────────────────────────────────────┘
```

## 约束
- 所有工具必须实现 `agent_types::Tool` trait（通过 `ports::tool` 重导出）
- `SrowToolContext`（在 `ports::tool`）实现 `agent_types::ToolContext`，提供 workspace/session_id/allow_dangerous
- `Provider` trait 及相关模型能力 trait 由 agent-types 定义，`ports::tool` 重导出
- SecurityGuard 是安全决策入口，必须在工具执行前调用
- SQLite 使用 migrations 模块管理 schema 演进
- 环境管理器（EnvironmentManager）负责嵌入式运行时的安装和版本解析
- DDD 层级：domain（纯实体）→ ports（trait 接口）→ adapters（具体实现）
- srow-app 仅依赖 srow-core（通过 Facade 模式），不直接依赖 agent-types/agent-core/agent-graph

## 业务域清单
| 名称 | 子目录 | 职责 |
|------|--------|------|
| 运行时工具 | `agent/runtime/tools/` | 9 种标准工具（bash、file_edit、grep、create_file、list_files、ask_human、internet_search、read_url、view_image）+ 7 种浏览器工具 |
| 安全体系 | `agent/runtime/security/` | SecurityGuard、PermissionManager、SandboxConfig、SensitivePathFilter、AuthorizedRoots |
| ACP 客户端 | `agent/agent_client/` | ACP 协议集成（connection / protocol / session / storage / delegate） |
| 记忆系统 | `agent/memory/` | MemoryService — FTS + 向量混合搜索、文件同步、embedding |
| 持久化 | `agent/persistence/` | SqliteStorage — session & message 的 SQLite 存储 |
| 会话管理 | `agent/session/` | SessionManager 会话生命周期管理 |
| MCP 集成 | `mcp/` | McpManager 运行时管理 + McpRuntimeTool + config + tool_adapter |
| 技能系统 | `skills/` | SkillLoader / SkillStore / SkillInjector / FsSkillRepository / AgentTemplateService / skill_domain / skill_ports |
| 环境管理 | `environment/` | EnvironmentManager — 嵌入式运行时安装、版本解析、manifest 管理 |
| 领域模型 | `domain/` | Agent / Session / Tool 等纯实体定义 |
| 端口接口 | `ports/` | Tool trait / ToolRegistry / SessionStorage / provider traits（embedding / image / speech / video 等） |
| 适配器 | `adapters/` | 端口的具体实现（如 memory storage） |
| 基础设施 | `base/` | ProcessManager 进程管理 |
| 类型 | `types/` | 共享类型定义（如 AcpMessage） |
| 网关 | `gateway/` | API 网关（placeholder） |
| 系统能力 | `system/` | 系统能力抽象（placeholder） |
| 错误 | `error.rs` | EngineError / SkillError 统一错误类型 |
