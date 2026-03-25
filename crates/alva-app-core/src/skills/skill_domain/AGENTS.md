# skills/skill_domain
> Skill 系统领域模型

## 地位
定义 Skill 系统的核心业务实体，包括 Skill 元数据、配置、MCP 协议类型和 Agent 模板。

## 逻辑
四个独立的领域实体文件，`skill.rs` 定义 Skill 数据结构和三级加载层次，`skill_config.rs` 定义注入策略，`mcp.rs` 定义 MCP Server 配置和状态，`agent_template.rs` 定义 Agent 模板及其 Skill/MCP 声明集。

## 约束
- 纯数据类型，无 I/O
- 所有类型 Serialize/Deserialize

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| skill | `skill.rs` | SkillMeta、SkillBody、SkillResource、Skill、SkillKind |
| skill_config | `skill_config.rs` | SkillRef、InjectionPolicy（Auto/Explicit/Strict） |
| mcp | `mcp.rs` | McpTransportConfig、McpServerConfig、McpServerState、McpToolInfo |
| agent_template | `agent_template.rs` | AgentTemplate、SkillSet、McpSet、GlobalSkillConfig |
