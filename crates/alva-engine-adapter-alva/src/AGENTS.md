# alva-engine-adapter-alva
> Alva Hub 的 canonical app-layer adapter：每个 engine session 构造完整 BaseAgent。

## 地位
它实现轻量 `alva-engine-runtime::EngineRuntime` contract，但本身属于 Hub/app 产品层，允许依赖 `alva-app-core`。每个 native engine session 由 `BaseAgentFactory` 独立构造，保留 BaseAgent 的默认 plugin、security、memory、context 和自定义 harness 配方。

## 逻辑
1. `BaseAgentFactory` 在新 session 时构造一个独立 `BaseAgent`；默认 factory 消费 request 的 cwd/system prompt/max turns，复杂产品配方可自定义 factory。
2. `execute()` 将 BaseAgent loop 的 `AgentEvent` 映射为 `RuntimeEvent`，并持有 native session 到 BaseAgent 的映射。
3. 完成后 BaseAgent 变 idle 而不删除；同一 native session 的 `resume_session` 可继续历史，running session 拒绝并发 execute。
4. `cancel()` 和 `respond_permission()` 路由至对应 BaseAgent；修改 tool input 尚不被 Alva permission API 支持。

## 约束
- 不得构造裸 `AgentState` 或直接调用 `run_agent`；必须通过 BaseAgent factory。
- 一个 engine session 只拥有一个 BaseAgent，不能用 `swap_session` 共享 agent 支撑并发。
- 这是 app/product 层 crate，不能被 SDK/kernel/agent crate 反向依赖；Hub core 只依赖 `alva-engine-runtime` contract。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| AlvaAdapter | adapter.rs | EngineRuntime trait 实现，管理 session 生命周期、启动 agent loop 并输出事件流 |
| BaseAgentFactory | factory.rs | 每 session 完整 BaseAgent 组装；default factory 提供 batteries baseline |
| EventMapper | mapping.rs | 有状态事件映射器，将 AgentEvent 转换为 RuntimeEvent，维护 turn 计数和工具名查找表 |
| lib.rs | lib.rs | 模块入口，重导出 AlvaAdapter 与 BaseAgent factory API |
