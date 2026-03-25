# skills/skill_ports
> Skill 系统端口定义

## 地位
定义 Skill 系统的抽象接口，包括 Skill 仓库和 MCP 传输层。

## 逻辑
`SkillRepository` trait 抽象 Skill 的 CRUD 操作（list/find/load/install/remove），`McpTransport` trait 抽象 MCP 通信层（connect/disconnect/list_tools/call_tool）。

## 约束
- 仅 trait 定义，不含具体实现
- 所有 trait `Send + Sync + #[async_trait]`

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| skill_repository | `skill_repository.rs` | SkillRepository trait、SkillInstallSource |
| mcp_transport | `mcp_transport.rs` | McpTransport trait |
