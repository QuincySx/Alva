# alva-engine-adapter-alva
> Alva Hub 的 canonical app-layer adapter：每个 engine session 构造完整 BaseAgent。

## 地位
它实现轻量 `alva-engine-runtime::EngineRuntime` contract，但本身属于 Hub/app 产品层，允许依赖 `alva-app-core`。每个 native engine session 由 `BaseAgentFactory` 独立构造，保留 BaseAgent 的默认 plugin、security、memory、context 和自定义 harness 配方。

## 逻辑
1. `BaseAgentFactory` 在新 session 时构造一个独立 `BaseAgent`，返回 `BuiltAgent`（可选 `approval_rx`）。
2. **`CatalogBaseAgentFactory`（Hub 产品默认）**：`apply_components` 全量 catalog + `ApprovalPlugin` + Skills/MCP 双源路径 + AGENTS.md 注入，与 CLI/Tauri 深度对齐。
3. **`DefaultBaseAgentFactory`**：仅 default plugins，供单测与极简场景。
4. `execute()` 映射 `AgentEvent` → `RuntimeEvent`；`approval_rx` 由 session 持有，每 turn 暂借给当前事件流做 multiplex，结束后归还，避免旧 stream 长驻或截获新 turn 审批。
5. 可选 **`HistoryStore`**：每 turn 结束后把 `BaseAgent::messages()` 以 0700 目录 / 0600 原子文件落盘；拒绝越界符号链接；跨进程 `resume_session` 时重建 agent + `restore_messages`。
6. 同进程 idle BaseAgent 优先；running session 拒绝并发 execute。
7. `cancel()` / `respond_permission()` 路由至对应 BaseAgent；修改 tool input 尚不支持。

## 约束
- 不得构造裸 `AgentState` 或直接调用 `run_agent`；必须通过 BaseAgent factory。
- 一个 engine session 只拥有一个 BaseAgent，不能用 `swap_session` 共享 agent 支撑并发。
- 这是 app/product 层 crate，不能被 SDK/kernel/agent crate 反向依赖；Hub core 只依赖 `alva-engine-runtime` contract。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| AlvaAdapter | adapter.rs | EngineRuntime：session 生命周期、turn-scoped approval 路由、history resume |
| BaseAgentFactory / BuiltAgent | factory.rs | 每 session 组装；Catalog vs Default 两档 |
| HistoryStore | history.rs | 私有、原子、符号链接安全的消息 transcript 落盘 / 加载 |
| EventMapper | mapping.rs | AgentEvent → RuntimeEvent；Completed 聚合最后一条助手消息的全部文本块 |
| lib.rs | lib.rs | 重导出 adapter 与 factory API |
