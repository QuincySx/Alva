# skills
> Skill 系统 —— 可插拔能力扩展框架

## 地位
alva-app-core 的 Skill 子系统，提供三级渐进式加载（元数据/指令/资源）、注入策略、MBB 域名路由、安装/卸载和 Agent 模板实例化。

## 逻辑
`SkillStore` 在启动时扫描文件系统建立内存索引，`SkillLoader` 按需加载 SKILL.md 正文和资源文件，`SkillInjector` 根据注入策略将 Skill 内容注入系统提示词，`AgentTemplateService` 将 AgentTemplate 实例化为运行时配置，`FsSkillRepository` 实现文件系统后端，`SearchSkillsTool`/`UseSkillTool` 暴露给 Agent 的发现/激活 meta-tool。

## 约束
- SKILL.md 使用 YAML frontmatter + Markdown body 格式
- Skill 名称 kebab-case，最长 64 字符
- Bundled Skill 不可卸载

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明所有子模块 |
| skill_domain/ | `skill_domain/` | Skill 领域模型 |
| skill_ports/ | `skill_ports/` | Skill 端口定义 |
| loader | `loader.rs` | SkillLoader：三级渐进式加载 |
| store | `store.rs` | SkillStore：内存索引 + CRUD |
| injector | `injector.rs` | SkillInjector：系统提示词注入 |
| skill_fs | `skill_fs.rs` | FsSkillRepository：文件系统 Skill 仓库实现 |
| agent_template_service | `agent_template_service.rs` | AgentTemplateService：模板实例化 |
| tools | `tools.rs` | SearchSkillsTool、UseSkillTool：Agent meta-tool |
