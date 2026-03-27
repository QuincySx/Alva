# tests
> alva-app-core 集成测试

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| acp_integration | `acp_integration.rs` | ACP 协议集成测试：完整生命周期、即时关闭、状态机转换（使用 echo_agent.py） |
| skill_system_test | `skill_system_test.rs` | Skill 系统集成测试：frontmatter 解析、FS 仓库扫描、MBB 路由、注入策略 |
