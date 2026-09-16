# tests
> alva-app-core 集成测试

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| agent_capabilities | `agent_capabilities.rs` | 内置工具能力回归：MockLanguageModel 确定性 suite + env-gated 真实模型 suite；支持 `ALVA_TEST_REPEAT` 稳定性采样；写完整 viewer 报告和 `latest-agent-summary.json` |
| escalation_permission_modes | `escalation_permission_modes.rs` | 无端口依赖的五模式升级请求 golden，以及“改文件→测试失败→修复→复测”脚本化闭环 |
| acp_integration | `acp_integration.rs` | ACP 协议集成测试：完整生命周期、即时关闭、状态机转换（使用 echo_agent.py） |
| skill_system_test | `skill_system_test.rs` | Skill 系统集成测试：frontmatter 兼容、FS 仓库、MBB、注入策略、auto/explicit 目录可见性与统一 `skill` 调用 |
| r48_swap_child_identity | `r48_swap_child_identity.rs` | R48：运行中 child 跨父 swap 保持身份——swap 握手证明曾等待 live turn、真实执行第二个 root turn 并核对新旧记录、查询 child parent id 与后续日志 |
| r10_compaction_swap_chain | `r10_compaction_swap_chain.rs` | R48/R10 组合证据：A→B→C→B 换会话链上压缩 marker 与改写投影落在压缩触发时的当前会话；A 绑定真实首轮会话（先证消息存在）再验无串线 |
| r11_tool_lock_matrix | `r11_tool_lock_matrix.rs` | R48/R11 组合证据：父相对/子绝对 key 双向碰撞、两个真实 child 同资源写互斥（gate 停靠 + active/max-active）、异资源与读读在飞重叠、全局串行化变异必失败 |
