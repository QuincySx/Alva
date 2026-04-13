# alva-host-native/src/middleware
> 运行时中间件聚合：本地拥有 checkpoint，从 L3 box 转发 security/plan_mode/compaction

## 地位
`alva-host-native` 的中间件子模块。Phase 2 重构后，**只有 `CheckpointMiddleware` 物理存放在这里**——它属于 host 装配期的持久化关注点。`SecurityMiddleware` / `PlanModeMiddleware` / `CompactionMiddleware` 已经搬到各自的 L3 box（`alva-agent-security` / `alva-agent-context`）拥有，本目录通过 `pub use` 转发它们，让 `crate::middleware::SecurityMiddleware` 等老 callsite 不需要改路径。

## 逻辑
1. `mod.rs` 声明 `checkpoint` 子模块，并从 `alva_agent_security::middleware` / `alva_agent_context::middleware` re-export 三个搬走的中间件。
2. `checkpoint.rs` 实现 `CheckpointMiddleware`：写入工具执行前自动备份文件，从 bus 读取 `CheckpointCallbackRef`。

## 约束
- **新增 host-level middleware** 才放在这里。任何依赖某个 L3 box（security / context / memory / tools / graph）的中间件都应该放进**那个 box 自己的 `middleware/` 子模块**，host-native 只负责再 re-export。
- `mod.rs` 的 re-export 列表保持 `pub use alva_agent_xxx::middleware::Y` 的格式，方便审计哪些中间件来自哪个 box。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Module Root | mod.rs | 声明 `checkpoint` 子模块；从 alva-agent-security / alva-agent-context 转发 SecurityMiddleware / PlanModeMiddleware / CompactionMiddleware |
| CheckpointMiddleware | checkpoint.rs | 工具执行前自动备份文件，从 bus 读取 CheckpointCallbackRef |
