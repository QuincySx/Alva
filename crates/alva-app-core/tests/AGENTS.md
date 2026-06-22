# tests
> alva-app-core 集成测试

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| agent_capabilities | `agent_capabilities.rs` | 内置工具能力回归：MockLanguageModel 确定性 suite + env-gated 真实模型 suite；支持 `ALVA_TEST_REPEAT` 稳定性采样；写完整 viewer 报告和 `latest-agent-summary.json` |
| acp_integration | `acp_integration.rs` | ACP 协议集成测试：完整生命周期、即时关闭、状态机转换（使用 echo_agent.py） |
| skill_system_test | `skill_system_test.rs` | Skill 系统集成测试：frontmatter 解析、FS 仓库扫描、MBB 路由、注入策略 |
