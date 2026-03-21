# agent/orchestrator
> 多 Agent 编排层 —— brain/reviewer/explorer 三角协作

## 地位
实现 srow-core 的多 Agent 编排模式：brain（决策）选择模板并派发任务，reviewer（评审）检查结果，explorer（探索）在评审失败时寻找替代方案。

## 逻辑
`Orchestrator` 持有三个核心 Agent 实例和一个动态执行 Agent 池，通过 `MessageBus` 实现 Agent 间通信。`OrchestratorAgentTemplate` 定义模板蓝图，`predefined_templates` 提供四种内置模板。7 个编排工具（create_agent、send_to_agent 等）注册到 brain 的 ToolRegistry。

## 约束
- 执行 Agent 由 brain Agent 通过 tool_call 创建
- reviewer 当前为占位实现
- MessageBus 为内存实现

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| orchestrator | `orchestrator.rs` | Orchestrator、OrchestratorHandle |
| instance | `instance.rs` | AgentInstance、AgentInstanceStatus |
| template | `template.rs` | OrchestratorAgentTemplate、predefined_templates |
| communication | `communication.rs` | AgentMessage、MessageBus |
| tools | `tools.rs` | 7 个编排工具 + register_orchestration_tools |
