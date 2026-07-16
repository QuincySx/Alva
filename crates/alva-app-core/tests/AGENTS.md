# tests
> alva-app-core 集成测试

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| agent_capabilities | `agent_capabilities.rs` | 内置工具能力回归：MockLanguageModel 确定性 suite + env-gated 真实模型 suite；支持 `ALVA_TEST_REPEAT` 稳定性采样；写完整 viewer 报告和 `latest-agent-summary.json` |
| escalation_permission_modes | `escalation_permission_modes.rs` | 无端口依赖的五模式升级请求 golden，以及“改文件→测试失败→修复→复测”脚本化闭环 |
| acp_integration | `acp_integration.rs` | ACP 协议集成测试：完整生命周期、即时关闭、状态机转换（使用 echo_agent.py） |
| skill_system_test | `skill_system_test.rs` | Skill 系统集成测试：frontmatter 兼容、FS 仓库、MBB、注入策略、auto/explicit 目录可见性与统一 `skill` 调用 |
